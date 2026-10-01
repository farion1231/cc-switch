//! Explicit, cancellable OpenSSH probes with locally confirmed host-key pins.
//! User SSH configuration is never loaded and remote output is never executed.

use super::credentials::{self, CredentialStore, OsCredentialStore, SecretString};
use super::{
    check_directory, read_regular_file, render_ssh_config, ssh_identity_path, VpsAuthMethod,
    VpsServer, VpsService,
};
pub mod askpass;
use crate::config::{atomic_write_private, sorted_json_bytes};
use anyhow::{anyhow, bail, Context, Result};
use base64::{
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
    Engine,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::{Duration, Instant};

const TEST_TIMEOUT: Duration = Duration::from_secs(20);
const CONFIRMATION_LIFETIME: Duration = Duration::from_secs(300);
const MAX_OUTPUT: u64 = 64 * 1024;
const MAX_TRUST_FILE: u64 = 1024 * 1024;
const MAX_PENDING: usize = 128;
const MAX_ACTIVE: usize = 8;
const KNOWN_HOSTS_HEADER: &str =
    "# CC Switch VPS host keys; managed by explicit fingerprint confirmation.\n";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum VpsConnectionStatus {
    Success,
    HostKeyConfirmationRequired,
    HostKeyChanged,
    TargetChanged,
    AuthenticationFailed,
    CredentialsUnavailable,
    PasswordRequired,
    SshNotFound,
    Timeout,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VpsConnectionTestResult {
    pub status: VpsConnectionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmation_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl VpsConnectionTestResult {
    fn new(status: VpsConnectionStatus, message: impl Into<String>) -> Self {
        Self {
            status,
            fingerprint: None,
            confirmation_token: None,
            message: Some(message.into()),
        }
    }
}

#[derive(Debug, Clone)]
struct CommandSpec {
    program: &'static str,
    args: Vec<OsString>,
    password: Option<Arc<SecretString>>,
}

#[derive(Debug)]
struct ProcessOutput {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[derive(Debug)]
enum ProcessFailure {
    NotFound(&'static str),
    Timeout,
    Cancelled,
    OutputLimit,
    Io,
}

trait ProcessRunner: Send + Sync {
    fn run(
        &self,
        spec: &CommandSpec,
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<ProcessOutput, ProcessFailure>;
}

struct OpenSshRunner;

struct ReapedChild(Child);

impl Drop for ReapedChild {
    fn drop(&mut self) {
        // Also runs on I/O errors, cancellation and timeout; never leave a child unreaped.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl ProcessRunner for OpenSshRunner {
    fn run(
        &self,
        spec: &CommandSpec,
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<ProcessOutput, ProcessFailure> {
        if cancelled.load(Ordering::SeqCst) {
            return Err(ProcessFailure::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(ProcessFailure::Timeout);
        }
        // Anonymous files avoid pipe-reader threads that can hang after a process is killed.
        // Their sizes are bounded while polling, and their handles are discarded after the probe.
        let mut stdout = tempfile::tempfile().map_err(|_| ProcessFailure::Io)?;
        let mut stderr = tempfile::tempfile().map_err(|_| ProcessFailure::Io)?;
        let mut command = Command::new(spec.program);
        command
            .args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::from(
                stdout.try_clone().map_err(|_| ProcessFailure::Io)?,
            ))
            .stderr(Stdio::from(
                stderr.try_clone().map_err(|_| ProcessFailure::Io)?,
            ))
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .env("SSH_ASKPASS_REQUIRE", "never")
            .env_remove("SSH_ASKPASS")
            .env_remove("DISPLAY");
        askpass::clear_environment(&mut command);
        let mut broker = spec
            .password
            .as_ref()
            .map(|password| askpass::Broker::new(password.clone()))
            .transpose()
            .map_err(|_| ProcessFailure::Io)?;
        if let Some(broker) = &broker {
            broker
                .configure(&mut command)
                .map_err(|_| ProcessFailure::Io)?;
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let child = command.spawn().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ProcessFailure::NotFound(spec.program)
            } else {
                ProcessFailure::Io
            }
        })?;
        let mut child = ReapedChild(child);
        loop {
            if cancelled.load(Ordering::SeqCst) {
                return Err(ProcessFailure::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(ProcessFailure::Timeout);
            }
            if let Some(broker) = &mut broker {
                broker
                    .poll(cancelled, deadline)
                    .map_err(|_| ProcessFailure::Io)?;
            }
            if stdout.metadata().map_err(|_| ProcessFailure::Io)?.len() > MAX_OUTPUT
                || stderr.metadata().map_err(|_| ProcessFailure::Io)?.len() > MAX_OUTPUT
            {
                return Err(ProcessFailure::OutputLimit);
            }
            if let Some(status) = child.0.try_wait().map_err(|_| ProcessFailure::Io)? {
                return Ok(ProcessOutput {
                    code: status.code(),
                    stdout: read_process_output(&mut stdout)?,
                    stderr: read_process_output(&mut stderr)?,
                });
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn read_process_output(file: &mut fs::File) -> Result<Vec<u8>, ProcessFailure> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| ProcessFailure::Io)?;
    let mut bytes = Vec::new();
    file.take(MAX_OUTPUT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ProcessFailure::Io)?;
    if bytes.len() as u64 > MAX_OUTPUT {
        return Err(ProcessFailure::OutputLimit);
    }
    Ok(bytes)
}

fn version_command() -> CommandSpec {
    CommandSpec {
        program: "ssh",
        password: None,
        args: vec!["-V".into()],
    }
}

fn scan_command(server: &VpsServer) -> CommandSpec {
    CommandSpec {
        program: "ssh-keyscan",
        password: None,
        args: vec![
            "-T".into(),
            "5".into(),
            "-p".into(),
            server.port.to_string().into(),
            "-t".into(),
            "ed25519,ecdsa,rsa".into(),
            server.host.clone().into(),
        ],
    }
}

fn probe_config(server: &VpsServer, key: &PublicHostKey, known_hosts: &Path) -> Result<String> {
    let mut config = render_ssh_config(std::slice::from_ref(server))?;
    config.push_str(&format!(
        "\nHost *\n    UserKnownHostsFile \"{}\"\n    GlobalKnownHostsFile none\n    HostKeyAlias {}\n    HostKeyAlgorithms {}\n    KnownHostsCommand none\n    ProxyCommand none\n    ProxyJump none\n    PermitLocalCommand no\n    ControlMaster no\n    ControlPath none\n    UpdateHostKeys no\n    VerifyHostKeyDNS no\n    CheckHostIP no\n    ConnectionAttempts 1\n    ConnectTimeout 10\n    ServerAliveInterval 5\n    ServerAliveCountMax 1\n    ForwardAgent no\n    ForwardX11 no\n    ClearAllForwardings yes\n    RequestTTY no\n    LogLevel ERROR\n",
        ssh_identity_path(known_hosts)?, server.ssh_alias(), key.host_key_algorithms(),
    ));
    Ok(config)
}

fn probe_command(config: &Path, server: &VpsServer) -> CommandSpec {
    CommandSpec {
        program: "ssh",
        password: None,
        args: vec![
            "-F".into(),
            config.as_os_str().into(),
            "-T".into(),
            "-n".into(),
            server.ssh_alias().into(),
            "true".into(),
        ],
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PublicHostKey {
    kind: String,
    encoded: String,
}

impl PublicHostKey {
    fn decoded(&self) -> Result<Vec<u8>> {
        if self.encoded.len() > 16384 {
            bail!("SSH host key is too large");
        }
        let bytes = STANDARD
            .decode(&self.encoded)
            .context("Invalid SSH public key encoding")?;
        if STANDARD.encode(&bytes) != self.encoded {
            bail!("Noncanonical SSH public key encoding");
        }
        let mut rest = bytes.as_slice();
        if ssh_string(&mut rest)? != self.kind.as_bytes() {
            bail!("SSH public key type mismatch");
        }
        match self.kind.as_str() {
            "ssh-ed25519" => {
                if ssh_string(&mut rest)?.len() != 32 {
                    bail!("Invalid Ed25519 host key");
                }
            }
            "ecdsa-sha2-nistp256" | "ecdsa-sha2-nistp384" | "ecdsa-sha2-nistp521" => {
                let curve = self.kind.strip_prefix("ecdsa-sha2-").unwrap();
                if ssh_string(&mut rest)? != curve.as_bytes() {
                    bail!("Invalid ECDSA host key curve");
                }
                let point = ssh_string(&mut rest)?;
                let length = match curve {
                    "nistp256" => 65,
                    "nistp384" => 97,
                    _ => 133,
                };
                if point.len() != length || point.first() != Some(&4) {
                    bail!("Invalid ECDSA host key point");
                }
            }
            "ssh-rsa" => {
                let exponent = ssh_string(&mut rest)?;
                let modulus = ssh_string(&mut rest)?;
                if exponent.is_empty()
                    || exponent.len() > 8
                    || exponent.iter().all(|b| *b == 0)
                    || !(256..=1025).contains(&modulus.len())
                    || modulus.first().is_none_or(|b| b & 0x80 != 0)
                {
                    bail!("Unsupported RSA host key size");
                }
                let significant = modulus
                    .iter()
                    .skip_while(|byte| **byte == 0)
                    .copied()
                    .collect::<Vec<_>>();
                if significant.len() < 256
                    || (significant.len() == 256 && significant[0] & 0x80 == 0)
                {
                    bail!("RSA host keys must be at least 2048 bits");
                }
            }
            _ => bail!("Unsupported SSH host key type"),
        }
        if !rest.is_empty() {
            bail!("Trailing SSH host key data");
        }
        Ok(bytes)
    }

    fn fingerprint(&self) -> Result<String> {
        Ok(format!(
            "SHA256:{} ({})",
            STANDARD_NO_PAD.encode(Sha256::digest(self.decoded()?)),
            self.kind
        ))
    }

    fn host_key_algorithms(&self) -> &str {
        if self.kind == "ssh-rsa" {
            "rsa-sha2-512,rsa-sha2-256"
        } else {
            &self.kind
        }
    }

    fn known_hosts_line(&self, alias: &str) -> String {
        // OpenSSH uses HostKeyAlias verbatim, even with a non-default Port.
        format!("{alias} {} {}\n", self.kind, self.encoded)
    }
}

fn ssh_string<'a>(rest: &mut &'a [u8]) -> Result<&'a [u8]> {
    if rest.len() < 4 {
        bail!("Truncated SSH public key");
    }
    let size = u32::from_be_bytes(rest[..4].try_into().unwrap()) as usize;
    *rest = &rest[4..];
    if size > rest.len() {
        bail!("Truncated SSH public key field");
    }
    let (field, remaining) = rest.split_at(size);
    *rest = remaining;
    Ok(field)
}

fn parse_scan(bytes: &[u8]) -> Result<PublicHostKey> {
    let text = std::str::from_utf8(bytes).context("Invalid SSH keyscan output")?;
    let mut keys = BTreeMap::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let fields: Vec<_> = line.split_ascii_whitespace().collect();
        if fields.len() != 3 {
            bail!("Unexpected SSH keyscan output");
        }
        let key = PublicHostKey {
            kind: fields[1].into(),
            encoded: fields[2].into(),
        };
        key.decoded()?;
        if keys.get(&key.kind).is_some_and(|existing| existing != &key) {
            bail!("The host returned conflicting public keys");
        }
        keys.insert(key.kind.clone(), key);
    }
    for kind in [
        "ssh-ed25519",
        "ecdsa-sha2-nistp256",
        "ecdsa-sha2-nistp384",
        "ecdsa-sha2-nistp521",
        "ssh-rsa",
    ] {
        if let Some(key) = keys.remove(kind) {
            return Ok(key);
        }
    }
    bail!("The host returned no supported public key")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TrustedHost {
    host: String,
    port: u16,
    key: PublicHostKey,
}

impl TrustedHost {
    fn from_server(server: &VpsServer, key: PublicHostKey) -> Self {
        Self {
            host: server.host.clone(),
            port: server.port,
            key,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TrustDocument {
    version: u32,
    hosts: BTreeMap<String, TrustedHost>,
    records_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_known_hosts_hash: Option<String>,
}

impl TrustDocument {
    fn empty() -> Self {
        let hosts = BTreeMap::new();
        Self {
            version: 1,
            records_hash: hash_bytes(b"{}"),
            hosts,
            previous_known_hosts_hash: None,
        }
    }

    fn refresh_receipt(&mut self) -> Result<()> {
        self.records_hash = hash_bytes(&sorted_json_bytes(&self.hosts)?);
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        if self.version != 1 || self.records_hash != hash_bytes(&sorted_json_bytes(&self.hosts)?) {
            bail!("VPS SSH trust data was modified or has an unsupported version");
        }
        if self
            .previous_known_hosts_hash
            .as_ref()
            .is_some_and(|hash| !valid_hash(hash))
        {
            bail!("Invalid VPS SSH trust recovery data");
        }
        for (id, record) in &self.hosts {
            let mut server =
                VpsServer::new("SSH trust".into(), record.host.clone(), "unused".into());
            server.id = id.clone();
            server.port = record.port;
            server.validate()?;
            record.key.decoded()?;
        }
        Ok(())
    }

    fn known_hosts(&self) -> Vec<u8> {
        let mut output = KNOWN_HOSTS_HEADER.to_string();
        for (id, record) in &self.hosts {
            output.push_str(&record.key.known_hosts_line(&format!("cc-switch-vps-{id}")));
        }
        output.into_bytes()
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn trust_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct TrustStore {
    root: PathBuf,
}

type LoadedTrust = (TrustDocument, Option<Vec<u8>>, Option<Vec<u8>>);

impl TrustStore {
    fn check_root(&self) -> Result<()> {
        if !self.root.is_absolute()
            || self
                .root
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            bail!("Invalid VPS SSH trust directory");
        }
        check_directory(&self.root)
    }

    fn read(&self, name: &str) -> Result<Option<Vec<u8>>> {
        self.check_root()?;
        let path = self.root.join(name);
        if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.len() > MAX_TRUST_FILE) {
            bail!("VPS SSH trust file exceeds the size limit");
        }
        read_regular_file(&path)
    }

    fn replace(&self, name: &str, expected: Option<&[u8]>, bytes: &[u8]) -> Result<()> {
        if self.read(name)?.as_deref() != expected {
            bail!("VPS SSH trust file changed during confirmation; not overwritten");
        }
        atomic_write_private(&self.root.join(name), bytes).context("Write VPS SSH trust file")
    }

    fn load_unlocked(&self) -> Result<LoadedTrust> {
        let source = self.read("ssh-trust.json")?;
        let known = self.read("known_hosts")?;
        let Some(source_bytes) = source.as_deref() else {
            if known.is_some() {
                bail!("VPS known_hosts is not owned by SSH trust management");
            }
            return Ok((TrustDocument::empty(), None, None));
        };
        let mut document: TrustDocument =
            serde_json::from_slice(source_bytes).context("Parse VPS SSH trust data")?;
        document.validate()?;
        let expected = document.known_hosts();
        if let Some(known_bytes) = &known {
            if known_bytes != &expected
                && document.previous_known_hosts_hash.as_deref()
                    != Some(hash_bytes(known_bytes).as_str())
            {
                bail!("VPS known_hosts was modified; not overwritten");
            }
        }
        if known.as_deref() != Some(expected.as_slice())
            || document.previous_known_hosts_hash.is_some()
        {
            // Recover an explicitly confirmed pin after an interrupted write, never a scanned key.
            if known.as_deref() != Some(expected.as_slice()) {
                self.replace("known_hosts", known.as_deref(), &expected)?;
            }
            document.previous_known_hosts_hash = None;
            let completed = sorted_json_bytes(&document)?;
            self.replace("ssh-trust.json", Some(source_bytes), &completed)?;
            return Ok((document, Some(completed), Some(expected)));
        }
        Ok((document, source, known))
    }

    fn load(&self) -> Result<TrustDocument> {
        let _guard = trust_lock()
            .lock()
            .map_err(|_| anyhow!("VPS SSH trust lock is poisoned"))?;
        Ok(self.load_unlocked()?.0)
    }

    fn add(&self, id: &str, host: &TrustedHost) -> Result<()> {
        let _guard = trust_lock()
            .lock()
            .map_err(|_| anyhow!("VPS SSH trust lock is poisoned"))?;
        let (mut document, source, known) = self.load_unlocked()?;
        if let Some(existing) = document.hosts.get(id) {
            if existing == host {
                return Ok(());
            }
            bail!("VPS host trust changed after scanning; refusing to replace the confirmed key");
        }
        document.hosts.insert(id.into(), host.clone());
        document.refresh_receipt()?;
        document.previous_known_hosts_hash = known.as_deref().map(hash_bytes);
        let intent = sorted_json_bytes(&document)?;
        let next_known = document.known_hosts();
        self.replace("ssh-trust.json", source.as_deref(), &intent)?;
        self.replace("known_hosts", known.as_deref(), &next_known)?;
        document.previous_known_hosts_hash = None;
        self.replace(
            "ssh-trust.json",
            Some(&intent),
            &sorted_json_bytes(&document)?,
        )
    }
}

#[derive(Clone)]
struct Confirmation {
    request_id: String,
    server_id: String,
    root: PathBuf,
    host: TrustedHost,
    expires_at: Instant,
}

#[derive(Default)]
struct Requests {
    shutting_down: bool,
    active: HashMap<String, Arc<AtomicBool>>,
    cancelled: HashMap<String, Instant>,
    confirmations: HashMap<String, Confirmation>,
}

impl Requests {
    fn prune(&mut self) {
        let now = Instant::now();
        self.cancelled.retain(|_, expires| *expires > now);
        self.confirmations
            .retain(|_, confirmation| confirmation.expires_at > now);
    }
}

struct Inner {
    requests: Mutex<Requests>,
    runner: Arc<dyn ProcessRunner>,
    credentials: Arc<dyn CredentialStore>,
    root_override: Option<PathBuf>,
}

/// Register one shared instance in Tauri; no background work starts on construction.
#[derive(Clone)]
pub struct VpsSshState {
    inner: Arc<Inner>,
}

impl Default for VpsSshState {
    fn default() -> Self {
        Self {
            inner: Arc::new(Inner {
                requests: Mutex::new(Requests::default()),
                runner: Arc::new(OpenSshRunner),
                credentials: Arc::new(OsCredentialStore),
                root_override: None,
            }),
        }
    }
}

struct ActiveRequest {
    inner: Arc<Inner>,
    id: String,
    cancelled: Arc<AtomicBool>,
}

impl Drop for ActiveRequest {
    fn drop(&mut self) {
        if let Ok(mut requests) = self.inner.requests.lock() {
            if requests
                .active
                .get(&self.id)
                .is_some_and(|flag| Arc::ptr_eq(flag, &self.cancelled))
            {
                requests.active.remove(&self.id);
            }
        }
    }
}

impl VpsSshState {
    fn root(&self) -> PathBuf {
        self.inner
            .root_override
            .clone()
            .unwrap_or_else(|| VpsService::new().root().to_path_buf())
    }

    #[cfg(test)]
    fn for_test(root: PathBuf, runner: Arc<dyn ProcessRunner>) -> Self {
        Self {
            inner: Arc::new(Inner {
                requests: Mutex::new(Requests::default()),
                runner,
                credentials: Arc::new(credentials::tests::MemoryCredentialStore::default()),
                root_override: Some(root),
            }),
        }
    }

    pub async fn test_connection(
        &self,
        server: VpsServer,
        request_id: String,
        password: Option<String>,
    ) -> VpsConnectionTestResult {
        self.test_connection_with_timeout(server, request_id, password, TEST_TIMEOUT)
            .await
    }

    async fn test_connection_with_timeout(
        &self,
        server: VpsServer,
        request_id: String,
        password: Option<String>,
        timeout: Duration,
    ) -> VpsConnectionTestResult {
        let state = self.clone();
        let password = SecretString::provided(password);
        let worker_id = request_id.clone();
        let cancellation = Arc::new(AtomicBool::new(false));
        let worker_cancellation = cancellation.clone();
        let mut worker = tokio::task::spawn_blocking(move || {
            state.test_blocking_with_flag(server, worker_id, password, worker_cancellation)
        });
        let deadline = Instant::now() + timeout;
        let mut poll = tokio::time::interval(Duration::from_millis(20));
        loop {
            tokio::select! {
                biased;
                result = &mut worker => return result.unwrap_or_else(|_| {
                    VpsConnectionTestResult::new(VpsConnectionStatus::Failed, "SSH test worker failed.")
                }),
                _ = poll.tick() => {
                    let cancelled = self.inner.requests.lock().is_ok_and(|requests| {
                        requests.cancelled.contains_key(&request_id)
                            || requests.active.get(&request_id).is_some_and(|flag| flag.load(Ordering::SeqCst))
                    });
                    if cancelled || Instant::now() >= deadline {
                        // OS credential calls cannot be forcibly interrupted. Return promptly, but
                        // keep the worker's active slot until it ends; its flag prevents any SSH
                        // spawn after a delayed credential result and bounds concurrent workers.
                        cancellation.store(true, Ordering::SeqCst);
                        let _ = self.cancel_test(&request_id);
                        return process_failure(if cancelled { ProcessFailure::Cancelled } else { ProcessFailure::Timeout });
                    }
                }
            }
        }
    }

    #[cfg(test)]
    fn test_blocking(&self, server: VpsServer, request_id: String) -> VpsConnectionTestResult {
        self.test_blocking_with_password(server, request_id, None)
    }

    #[cfg(test)]
    fn test_blocking_with_password(
        &self,
        server: VpsServer,
        request_id: String,
        password: Option<SecretString>,
    ) -> VpsConnectionTestResult {
        self.test_blocking_with_flag(
            server,
            request_id,
            password,
            Arc::new(AtomicBool::new(false)),
        )
    }

    fn test_blocking_with_flag(
        &self,
        server: VpsServer,
        request_id: String,
        password: Option<SecretString>,
        cancelled: Arc<AtomicBool>,
    ) -> VpsConnectionTestResult {
        if server.validate().is_err() || validate_request_id(&request_id).is_err() {
            return VpsConnectionTestResult::new(
                VpsConnectionStatus::Failed,
                "Invalid SSH test parameters.",
            );
        }
        {
            let Ok(mut requests) = self.inner.requests.lock() else {
                return VpsConnectionTestResult::new(
                    VpsConnectionStatus::Failed,
                    "SSH request state is unavailable.",
                );
            };
            requests.prune();
            if requests.shutting_down
                || cancelled.load(Ordering::SeqCst)
                || requests.cancelled.remove(&request_id).is_some()
            {
                return VpsConnectionTestResult::new(
                    VpsConnectionStatus::Cancelled,
                    "SSH test cancelled.",
                );
            }
            if requests.active.contains_key(&request_id) || requests.active.len() >= MAX_ACTIVE {
                return VpsConnectionTestResult::new(
                    VpsConnectionStatus::Failed,
                    "SSH test is already active or the concurrency limit was reached.",
                );
            }
            requests
                .active
                .insert(request_id.clone(), cancelled.clone());
        }
        let active = ActiveRequest {
            inner: self.inner.clone(),
            id: request_id,
            cancelled,
        };
        let result = self.perform_test(&server, &active, password);
        if active.cancelled.load(Ordering::SeqCst) {
            VpsConnectionTestResult::new(VpsConnectionStatus::Cancelled, "SSH test cancelled.")
        } else {
            result
        }
    }

    fn perform_test(
        &self,
        server: &VpsServer,
        active: &ActiveRequest,
        supplied: Option<SecretString>,
    ) -> VpsConnectionTestResult {
        let store = TrustStore { root: self.root() };
        let trust = match store.load() {
            Ok(trust) => trust,
            Err(_) => return VpsConnectionTestResult::new(
                VpsConnectionStatus::Failed,
                "SSH trust files are invalid, modified or inaccessible; they were not replaced.",
            ),
        };
        if trust
            .hosts
            .get(&server.id)
            .is_some_and(|pin| pin.host != server.host || pin.port != server.port)
        {
            return VpsConnectionTestResult::new(VpsConnectionStatus::TargetChanged, "The host or port differs from the confirmed target. Restore it or create a separate host and confirm its fingerprint.");
        }
        let deadline = Instant::now() + TEST_TIMEOUT;
        let run = |spec: &CommandSpec| self.inner.runner.run(spec, &active.cancelled, deadline);
        match run(&version_command()) {
            Ok(output) if output.code == Some(0) => {}
            Ok(_) => {
                return VpsConnectionTestResult::new(
                    VpsConnectionStatus::Failed,
                    "OpenSSH could not be started.",
                )
            }
            Err(error) => return process_failure(error),
        }
        let key = match trust.hosts.get(&server.id) {
            Some(trusted) => trusted.key.clone(),
            None => {
                let output = match run(&scan_command(server)) {
                    Ok(output) if output.code == Some(0) => output,
                    Ok(_) => {
                        return VpsConnectionTestResult::new(
                            VpsConnectionStatus::Failed,
                            "The SSH host key could not be retrieved.",
                        )
                    }
                    Err(error) => return process_failure(error),
                };
                let key = match parse_scan(&output.stdout) {
                    Ok(key) => key,
                    Err(_) => return VpsConnectionTestResult::new(
                        VpsConnectionStatus::Failed,
                        "The SSH host returned invalid, unsupported or conflicting public keys.",
                    ),
                };
                let fingerprint = match key.fingerprint() {
                    Ok(fingerprint) => fingerprint,
                    Err(_) => {
                        return VpsConnectionTestResult::new(
                            VpsConnectionStatus::Failed,
                            "The SSH public key could not be fingerprinted.",
                        )
                    }
                };
                let Ok(mut requests) = self.inner.requests.lock() else {
                    return VpsConnectionTestResult::new(
                        VpsConnectionStatus::Failed,
                        "SSH request state is unavailable.",
                    );
                };
                requests.prune();
                if active.cancelled.load(Ordering::SeqCst) {
                    return VpsConnectionTestResult::new(
                        VpsConnectionStatus::Cancelled,
                        "SSH test cancelled.",
                    );
                }
                if requests.confirmations.len() >= MAX_PENDING {
                    return VpsConnectionTestResult::new(
                        VpsConnectionStatus::Failed,
                        "Too many pending SSH fingerprint confirmations.",
                    );
                }
                let token = uuid::Uuid::new_v4().to_string();
                requests.confirmations.insert(
                    token.clone(),
                    Confirmation {
                        request_id: active.id.clone(),
                        server_id: server.id.clone(),
                        root: store.root,
                        host: TrustedHost::from_server(server, key),
                        expires_at: Instant::now() + CONFIRMATION_LIFETIME,
                    },
                );
                return VpsConnectionTestResult {
                    status: VpsConnectionStatus::HostKeyConfirmationRequired,
                    fingerprint: Some(fingerprint), confirmation_token: Some(token),
                    message: Some("Verify this fingerprint through a trusted source before confirming. Scanning a key does not prove the host's identity.".into()),
                };
            }
        };
        if active.cancelled.load(Ordering::SeqCst) {
            return process_failure(ProcessFailure::Cancelled);
        }
        if Instant::now() >= deadline {
            return process_failure(ProcessFailure::Timeout);
        }
        // Only a confirmed host-key pin reaches credential lookup. OpenSSH verifies that pin
        // again before requesting a password; key scanning never receives a secret.
        let password = if server.auth_method == Some(VpsAuthMethod::Password) {
            let password = match supplied {
                Some(password) => Some(password),
                None => match credentials::read_password(
                    &store.root,
                    server,
                    self.inner.credentials.as_ref(),
                    &active.cancelled,
                ) {
                    Ok(password) => password,
                    Err(error) if error.is::<credentials::PasswordTargetChanged>() => {
                        return VpsConnectionTestResult::new(
                            VpsConnectionStatus::TargetChanged,
                            "The connection details do not match the saved password's target. Reload the host before retrying.",
                        );
                    }
                    Err(_) => return VpsConnectionTestResult::new(
                        VpsConnectionStatus::CredentialsUnavailable,
                        "The OS credential store is unavailable; no plaintext fallback is used.",
                    ),
                },
            };
            let Some(password) = password.filter(|password| !password.expose().is_empty()) else {
                return VpsConnectionTestResult::new(
                    VpsConnectionStatus::PasswordRequired,
                    "Supply a password or save one in the OS credential store first.",
                );
            };
            if password.validate().is_err() {
                return VpsConnectionTestResult::new(
                    VpsConnectionStatus::Failed,
                    "The password cannot be used by OpenSSH askpass.",
                );
            }
            Some(Arc::new(password))
        } else {
            None
        };
        if active.cancelled.load(Ordering::SeqCst) {
            return process_failure(ProcessFailure::Cancelled);
        }
        if Instant::now() >= deadline {
            return process_failure(ProcessFailure::Timeout);
        }
        let prepared = (|| -> Result<(tempfile::TempDir, CommandSpec)> {
            store.check_root()?;
            let temp = tempfile::Builder::new()
                .prefix(".ssh-probe-")
                .tempdir_in(&store.root)?;
            let known_hosts = temp.path().join("known_hosts");
            let config = temp.path().join("ssh_config");
            atomic_write_private(
                &known_hosts,
                key.known_hosts_line(&server.ssh_alias()).as_bytes(),
            )?;
            atomic_write_private(
                &config,
                probe_config(server, &key, &known_hosts)?.as_bytes(),
            )?;
            let mut command = probe_command(&config, server);
            command.password = password;
            Ok((temp, command))
        })();
        let (_temp, command) = match prepared {
            Ok(prepared) => prepared,
            Err(_) => {
                return VpsConnectionTestResult::new(
                    VpsConnectionStatus::Failed,
                    "The private SSH test configuration could not be prepared.",
                )
            }
        };
        match run(&command) {
            Ok(output) => classify_probe(output),
            Err(error) => process_failure(error),
        }
    }

    /// Stop only probes owned by this state before the application exits or restarts.
    /// No window APIs are used, so restart cleanup can run on the event-loop thread.
    pub fn shutdown(&self) {
        {
            let mut requests = self
                .inner
                .requests
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            requests.shutting_down = true;
            requests.confirmations.clear();
            for flag in requests.active.values() {
                flag.store(true, Ordering::SeqCst);
            }
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if self
                .inner
                .requests
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .active
                .is_empty()
            {
                return;
            }
            if Instant::now() >= deadline {
                log::warn!("VPS probe shutdown did not finish within the cleanup deadline");
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn cancel_test(&self, request_id: &str) -> Result<(), String> {
        validate_request_id(request_id)?;
        let mut requests = self
            .inner
            .requests
            .lock()
            .map_err(|_| "SSH request state is unavailable".to_string())?;
        requests.prune();
        if let Some(cancelled) = requests.active.get(request_id) {
            cancelled.store(true, Ordering::SeqCst);
        }
        requests
            .confirmations
            .retain(|_, confirmation| confirmation.request_id != request_id);
        if requests.cancelled.len() >= MAX_PENDING && !requests.cancelled.contains_key(request_id) {
            return Err("Too many pending SSH cancellation requests".into());
        }
        // Covers a cancel IPC that arrives before the blocking test worker starts.
        requests
            .cancelled
            .insert(request_id.into(), Instant::now() + CONFIRMATION_LIFETIME);
        Ok(())
    }

    pub fn confirm_host_key(&self, confirmation_token: &str) -> Result<(), String> {
        validate_request_id(confirmation_token)?;
        let mut requests = self
            .inner
            .requests
            .lock()
            .map_err(|_| "SSH request state is unavailable".to_string())?;
        requests.prune();
        let confirmation = requests
            .confirmations
            .get(confirmation_token)
            .cloned()
            .ok_or_else(|| {
                "SSH fingerprint confirmation expired or was cancelled; test the connection again"
                    .to_string()
            })?;
        if self.root() != confirmation.root {
            return Err(
                "VPS data directory changed after scanning; test the connection again".into(),
            );
        }
        // Serialize confirmation with cancellation. No request mutex is held during network I/O.
        TrustStore { root: confirmation.root }.add(&confirmation.server_id, &confirmation.host)
            .map_err(|_| "SSH trust could not be saved, or changed since scanning; existing host keys were not replaced".to_string())?;
        requests.confirmations.remove(confirmation_token);
        Ok(())
    }
}

fn validate_request_id(id: &str) -> Result<(), String> {
    if uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id) {
        Ok(())
    } else {
        Err("Invalid SSH request or confirmation identifier".into())
    }
}

fn process_failure(error: ProcessFailure) -> VpsConnectionTestResult {
    match error {
        ProcessFailure::NotFound(tool) => VpsConnectionTestResult::new(
            VpsConnectionStatus::SshNotFound,
            format!(
                "Required OpenSSH tool '{tool}' was not found in this application's environment."
            ),
        ),
        ProcessFailure::Timeout => {
            VpsConnectionTestResult::new(VpsConnectionStatus::Timeout, "SSH test timed out.")
        }
        ProcessFailure::Cancelled => {
            VpsConnectionTestResult::new(VpsConnectionStatus::Cancelled, "SSH test cancelled.")
        }
        ProcessFailure::OutputLimit => VpsConnectionTestResult::new(
            VpsConnectionStatus::Failed,
            "SSH test output exceeded the safety limit.",
        ),
        ProcessFailure::Io => VpsConnectionTestResult::new(
            VpsConnectionStatus::Failed,
            "The SSH process could not be started or monitored.",
        ),
    }
}

fn classify_probe(output: ProcessOutput) -> VpsConnectionTestResult {
    if output.code == Some(0) {
        return VpsConnectionTestResult::new(
            VpsConnectionStatus::Success,
            "The SSH probe completed successfully.",
        );
    }
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    if stderr.contains("host identification has changed")
        || stderr.contains("host key verification failed")
        || stderr.contains("offending") && stderr.contains("host key")
    {
        VpsConnectionTestResult::new(VpsConnectionStatus::HostKeyChanged, "The host key does not match the confirmed fingerprint. Connection was blocked; verify the change independently.")
    } else if stderr.contains("permission denied")
        || stderr.contains("authentication failed")
        || stderr.contains("no supported authentication methods")
    {
        VpsConnectionTestResult::new(
            VpsConnectionStatus::AuthenticationFailed,
            "SSH authentication failed. Check the selected password, private key or SSH user certificate.",
        )
    } else if stderr.contains("connection timed out") || stderr.contains("operation timed out") {
        VpsConnectionTestResult::new(VpsConnectionStatus::Timeout, "SSH test timed out.")
    } else {
        VpsConnectionTestResult::new(
            VpsConnectionStatus::Failed,
            "The SSH probe failed. Check the host, port, key reference and network access.",
        )
    }
}

#[cfg(test)]
#[path = "ssh/tests.rs"]
mod tests;
