//! Execute one remote command with a pinned target and a single-use password capability.
//! This is separate from connection probes: standard streams are inherited, not captured/bounded.
use super::{askpass, probe_config, CommandSpec, ReapedChild, TrustStore};
use crate::services::vps::{
    cli::{CliError, ExecRequest},
    credentials::{self, CredentialStore, OsCredentialStore},
    VpsAuthMethod, VpsService,
};
use std::fs;
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

struct PreparedExec {
    // The configuration and pin must outlive the SSH child; neither contains a password.
    _directory: tempfile::TempDir,
    command: CommandSpec,
}

fn check_cancelled(cancelled: &AtomicBool, deadline: Instant) -> Result<(), CliError> {
    if cancelled.load(Ordering::SeqCst) {
        Err(CliError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(CliError::Timeout)
    } else {
        Ok(())
    }
}

fn prepare(
    request: &ExecRequest,
    store: Arc<dyn CredentialStore>,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<PreparedExec, CliError> {
    check_cancelled(cancelled, deadline)?;
    super::check_directory(&request.root).map_err(|_| CliError::DataInvalid)?;
    // Keep the native path spelling for OpenSSH (Windows canonical paths use a verbatim
    // prefix), and recheck its canonical identity after credential access.
    let canonical_root = fs::canonicalize(&request.root).map_err(|_| CliError::DataInvalid)?;
    let service = VpsService {
        root: request.root.clone(),
        credentials: store,
        #[cfg(test)]
        fail_write: None,
    };
    let (document, host_bytes) = service.read_document().map_err(|_| CliError::DataInvalid)?;
    if host_bytes.is_none() {
        return Err(CliError::DataInvalid);
    }
    let server = document
        .servers
        .iter()
        .find(|server| server.id == request.server_id && server.apps.is_enabled_for(&request.app))
        .ok_or(CliError::HostUnavailable)?;
    let trust = TrustStore {
        root: service.root.clone(),
    };
    let (pins, trust_bytes, known_bytes) = trust.read_only().map_err(|_| CliError::TrustInvalid)?;
    let pin = pins.hosts.get(&server.id).ok_or(CliError::TrustRequired)?;
    if pin.host != server.host || pin.port != server.port {
        return Err(CliError::TargetChanged);
    }
    check_cancelled(cancelled, deadline)?;
    // Use the immutable revision from this document, not a second read of mutable live state.
    // No vault access occurs until binding and confirmed host-key checks have passed.
    let password = if server.auth_method == Some(VpsAuthMethod::Password) {
        let password = credentials::password_from_document(
            service.root(),
            &document,
            &server.id,
            service.credentials.as_ref(),
        )
        .map_err(|_| CliError::CredentialsUnavailable)?
        .ok_or(CliError::PasswordRequired)?;
        password
            .validate()
            .map_err(|_| CliError::PasswordRequired)?;
        Some(Arc::new(password))
    } else {
        None
    };
    check_cancelled(cancelled, deadline)?;
    let directory = tempfile::Builder::new()
        .prefix(".ssh-exec-")
        .tempdir_in(service.root())
        .map_err(|_| CliError::PrepareFailed)?;
    let known_hosts = directory.path().join("known_hosts");
    let config = directory.path().join("ssh_config");
    crate::config::atomic_write_private(
        &known_hosts,
        pin.key.known_hosts_line(&server.ssh_alias()).as_bytes(),
    )
    .map_err(|_| CliError::PrepareFailed)?;
    crate::config::atomic_write_private(
        &config,
        probe_config(server, &pin.key, &known_hosts)
            .map_err(|_| CliError::PrepareFailed)?
            .as_bytes(),
    )
    .map_err(|_| CliError::PrepareFailed)?;
    // These are different processes: the service's in-process mutex is not a cross-process lock.
    // Recheck both snapshots after a possibly slow vault call, without repairing/writing live files.
    if fs::canonicalize(service.root()).map_err(|_| CliError::TargetChanged)? != canonical_root
        || service
            .read_document()
            .map_err(|_| CliError::TargetChanged)?
            .1
            != host_bytes
    {
        return Err(CliError::TargetChanged);
    }
    let (_, current_trust, current_known) =
        trust.read_only().map_err(|_| CliError::TargetChanged)?;
    if current_trust != trust_bytes || current_known != known_bytes {
        return Err(CliError::TargetChanged);
    }
    check_cancelled(cancelled, deadline)?;
    Ok(PreparedExec {
        _directory: directory,
        command: CommandSpec {
            program: "ssh",
            args: vec![
                "-F".into(),
                config.into_os_string(),
                "-T".into(),
                // OpenSSH also parses options after the destination unless explicitly ended.
                "--".into(),
                server.ssh_alias().into(),
                request.command.clone().into(),
            ],
            password,
        },
    })
}

fn run(prepared: PreparedExec, cancelled: &AtomicBool, deadline: Instant) -> Result<i32, CliError> {
    let mut command = Command::new(prepared.command.program);
    command.args(&prepared.command.args);
    run_command(command, prepared.command.password, cancelled, deadline)
}

fn run_command(
    mut command: Command,
    password: Option<Arc<credentials::SecretString>>,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<i32, CliError> {
    check_cancelled(cancelled, deadline)?;
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .env("SSH_ASKPASS_REQUIRE", "never")
        .env_remove("SSH_ASKPASS")
        .env_remove("DISPLAY");
    askpass::clear_environment(&mut command);
    let mut broker = password
        .map(askpass::Broker::new)
        .transpose()
        .map_err(|_| CliError::PrepareFailed)?;
    if let Some(broker) = &broker {
        broker
            .configure(&mut command)
            .map_err(|_| CliError::PrepareFailed)?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // An attached console is inherited normally; a pipe-only caller gets no new window.
        if unsafe { windows_sys::Win32::System::Console::GetConsoleCP() } == 0 {
            command.creation_flags(0x08000000);
        }
    }
    check_cancelled(cancelled, deadline)?;
    let child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            CliError::SshNotFound
        } else {
            CliError::ExecutionFailed
        }
    })?;
    let mut child = ReapedChild(child);
    loop {
        check_cancelled(cancelled, deadline)?;
        if let Some(broker) = &mut broker {
            broker
                .poll(cancelled, deadline)
                .map_err(|_| CliError::ExecutionFailed)?;
        }
        if let Some(status) = child.0.try_wait().map_err(|_| CliError::ExecutionFailed)? {
            if let Some(code) = status.code() {
                return Ok(code);
            }
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                return Ok(128 + status.signal().unwrap_or(1));
            }
            #[cfg(not(unix))]
            return Err(CliError::ExecutionFailed);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub(in crate::services::vps) fn execute(
    request: ExecRequest,
    cancelled: Arc<AtomicBool>,
) -> Result<i32, CliError> {
    let deadline = Instant::now() + request.timeout;
    let prepared = prepare(&request, Arc::new(OsCredentialStore), &cancelled, deadline)?;
    run(prepared, &cancelled, deadline)
}

#[cfg(test)]
mod tests;
