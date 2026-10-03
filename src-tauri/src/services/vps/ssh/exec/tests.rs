use super::super::{PublicHostKey, TrustedHost};
use super::*;
use crate::app_config::AppType;
use crate::services::vps::credentials::{tests::MemoryCredentialStore, SecretString};
use crate::services::vps::{VpsDocument, VpsServer};
use base64::Engine;
use std::path::Path;
use std::sync::Mutex;

struct Fixture {
    _home: tempfile::TempDir,
    service: VpsService,
    store: Arc<MemoryCredentialStore>,
    host: VpsServer,
    request: ExecRequest,
}

impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let store = Arc::new(MemoryCredentialStore::default());
        let service = VpsService {
            root: home.path().join("VPS space 日本語"),
            credentials: store.clone(),
            fail_write: None,
        };
        let mut host = VpsServer::new("test".into(), "192.0.2.10".into(), "deploy".into());
        host.auth_method = Some(VpsAuthMethod::Password);
        host.apps.claude = true;
        service
            .save_server_with_password(host.clone(), Some("test secret only".into()))
            .unwrap();
        let request = ExecRequest {
            root: service.root.clone(),
            app: AppType::Claude,
            server_id: host.id.clone(),
            command: "printf '%s\\n' 'remote value'; exit 37".into(),
            timeout: Duration::from_secs(10),
        };
        Self {
            _home: home,
            service,
            store,
            host,
            request,
        }
    }

    fn trust(&self) {
        let kind = b"ssh-ed25519";
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(kind.len() as u32).to_be_bytes());
        bytes.extend_from_slice(kind);
        bytes.extend_from_slice(&32u32.to_be_bytes());
        bytes.extend_from_slice(&[7; 32]);
        let key = PublicHostKey {
            kind: "ssh-ed25519".into(),
            encoded: base64::engine::general_purpose::STANDARD.encode(bytes),
        };
        TrustStore {
            root: self.service.root.clone(),
        }
        .add(&self.host.id, &TrustedHost::from_server(&self.host, key))
        .unwrap();
    }

    fn prepare(&self) -> Result<PreparedExec, CliError> {
        prepare(
            &self.request,
            self.store.clone(),
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(3),
        )
    }
}

fn error(result: Result<PreparedExec, CliError>) -> CliError {
    match result {
        Ok(_) => panic!("execution must be rejected"),
        Err(error) => error,
    }
}

#[test]
fn binding_and_confirmed_target_are_required_before_any_vault_access() {
    let mut fixture = Fixture::new();
    assert_eq!(error(fixture.prepare()), CliError::TrustRequired);
    fixture.request.app = AppType::Codex;
    assert_eq!(error(fixture.prepare()), CliError::HostUnavailable);
    fixture.request.app = AppType::Claude;
    fixture.request.server_id = uuid::Uuid::new_v4().to_string();
    assert_eq!(error(fixture.prepare()), CliError::HostUnavailable);
    fixture.request.server_id = fixture.host.id.clone();
    fixture.trust();
    mutate_document(&fixture.service.root, |document| {
        document.servers[0].host = "192.0.2.11".into()
    });
    assert_eq!(error(fixture.prepare()), CliError::TargetChanged);
    assert_eq!(fixture.store.get_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn remote_option_prefixes_cannot_become_local_ssh_options() {
    let mut fixture = Fixture::new();
    fixture.trust();
    for command in ["-V", "-oProxyCommand=never-execute", "-F another-config"] {
        fixture.request.command = command.into();
        let prepared = fixture.prepare().unwrap();
        assert_eq!(prepared.command.args[3], "--");
        assert_eq!(prepared.command.args[4], fixture.host.ssh_alias().as_str());
        assert_eq!(prepared.command.args[5], command);
        assert_eq!(prepared.command.args.len(), 6);
    }
}

#[test]
fn saved_password_reaches_only_private_execution_spec_and_pin_is_fixed() {
    let fixture = Fixture::new();
    fixture.trust();
    let before = snapshot(&fixture.service.root);
    let prepared = fixture.prepare().unwrap();
    let spec = &prepared.command;
    assert_eq!(spec.program, "ssh");
    assert_eq!(spec.password.as_ref().unwrap().expose(), "test secret only");
    assert_eq!(spec.args[0], "-F");
    assert_eq!(spec.args[2], "-T");
    assert_eq!(spec.args[3], "--");
    assert_eq!(spec.args[4], fixture.host.ssh_alias().as_str());
    assert_eq!(spec.args[5], fixture.request.command.as_str());
    assert_eq!(spec.args.len(), 6);
    assert!(!format!("{spec:?}").contains("test secret only"));
    let config = fs::read_to_string(&spec.args[1]).unwrap();
    for required in [
        "StrictHostKeyChecking yes",
        "GlobalKnownHostsFile none",
        "ProxyCommand none",
        "ProxyJump none",
        "PermitLocalCommand no",
        "ControlPath none",
        "ForwardAgent no",
        "ClearAllForwardings yes",
        "NumberOfPasswordPrompts 1",
    ] {
        assert!(config.contains(required), "{required}");
    }
    for name in ["ssh_config", "known_hosts"] {
        assert!(!fs::read_to_string(prepared._directory.path().join(name))
            .unwrap()
            .contains("test secret only"));
    }
    assert_eq!(snapshot(&fixture.service.root), before);
    let temporary = prepared._directory.path().to_owned();
    drop(prepared);
    assert!(!temporary.exists());
}

#[test]
fn missing_and_unavailable_credentials_fail_without_plaintext_fallback() {
    let fixture = Fixture::new();
    fixture.trust();
    fixture.store.fail_get.store(true, Ordering::SeqCst);
    assert_eq!(error(fixture.prepare()), CliError::CredentialsUnavailable);
    fixture.store.fail_get.store(false, Ordering::SeqCst);
    mutate_document(&fixture.service.root, |document| {
        document.password_revisions.clear();
    });
    assert_eq!(error(fixture.prepare()), CliError::PasswordRequired);
}

#[test]
fn trust_is_read_only_and_incomplete_or_modified_files_are_not_repaired() {
    let fixture = Fixture::new();
    fixture.trust();
    let known = fixture.service.root.join("known_hosts");
    fs::remove_file(&known).unwrap();
    let before = snapshot(&fixture.service.root);
    assert_eq!(error(fixture.prepare()), CliError::TrustInvalid);
    assert_eq!(snapshot(&fixture.service.root), before);
    assert!(!known.exists());
    assert_eq!(fixture.store.get_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn private_key_execution_does_not_read_passwords_or_private_key_contents() {
    let mut fixture = Fixture::new();
    fixture.trust();
    fixture.host.auth_method = Some(VpsAuthMethod::PrivateKey);
    fixture.host.identity_file = Some(fixture.service.root.join("nonexistent private key"));
    fixture.service.save_server(fixture.host.clone()).unwrap();
    fixture.store.fail_get.store(true, Ordering::SeqCst);
    let prepared = fixture.prepare().unwrap();
    assert!(prepared.command.password.is_none());
    assert_eq!(fixture.store.get_calls.load(Ordering::SeqCst), 0);
}

fn mutate_document(root: &Path, change: impl FnOnce(&mut VpsDocument)) {
    let path = root.join("servers.json");
    let mut document: VpsDocument = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    change(&mut document);
    crate::config::atomic_write_private(
        &path,
        &crate::config::sorted_json_bytes(&document).unwrap(),
    )
    .unwrap();
}

fn snapshot(root: &Path) -> Vec<Option<Vec<u8>>> {
    [
        "servers.json",
        "ssh-trust.json",
        "known_hosts",
        "ssh_config",
        "clients/claude.json",
    ]
    .iter()
    .map(|name| fs::read(root.join(name)).ok())
    .collect()
}

struct ChangingStore {
    inner: Arc<MemoryCredentialStore>,
    change: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl CredentialStore for ChangingStore {
    fn get(&self, namespace: &str, id: &str) -> anyhow::Result<Option<SecretString>> {
        let result = self.inner.get(namespace, id);
        self.change.lock().unwrap().take().unwrap()();
        result
    }
    fn set(&self, _: &str, _: &str, _: &SecretString) -> anyhow::Result<()> {
        unreachable!()
    }
    fn delete(&self, _: &str, _: &str) -> anyhow::Result<()> {
        unreachable!()
    }
}

#[test]
fn rechecks_binding_target_revision_and_trust_after_vault_read() {
    for change in ["unbind", "user", "revision", "delete", "trust"] {
        let fixture = Fixture::new();
        fixture.trust();
        let root = fixture.service.root.clone();
        let store = Arc::new(ChangingStore {
            inner: fixture.store.clone(),
            change: Mutex::new(Some(Box::new(move || match change {
                "delete" => {
                    fs::remove_file(root.join("servers.json")).unwrap();
                }
                "trust" => {
                    fs::write(root.join("known_hosts"), "modified").unwrap();
                }
                _ => mutate_document(&root, |document| match change {
                    "unbind" => document.servers[0].apps.claude = false,
                    "user" => document.servers[0].user = "someone-else".into(),
                    "revision" => {
                        document.password_revisions.insert(
                            document.servers[0].id.clone(),
                            uuid::Uuid::new_v4().to_string(),
                        );
                    }
                    _ => unreachable!(),
                }),
            }))),
        });
        assert_eq!(
            error(prepare(
                &fixture.request,
                store,
                &AtomicBool::new(false),
                Instant::now() + Duration::from_secs(3)
            )),
            CliError::TargetChanged,
            "{change}"
        );
    }
}

#[test]
fn cancelled_and_expired_preparations_never_read_the_vault() {
    let fixture = Fixture::new();
    fixture.trust();
    assert_eq!(
        error(prepare(
            &fixture.request,
            fixture.store.clone(),
            &AtomicBool::new(true),
            Instant::now() + Duration::from_secs(1)
        )),
        CliError::Cancelled
    );
    assert_eq!(
        error(prepare(
            &fixture.request,
            fixture.store.clone(),
            &AtomicBool::new(false),
            Instant::now()
        )),
        CliError::Timeout
    );
    assert_eq!(fixture.store.get_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn late_vault_result_cannot_start_a_cancelled_execution() {
    let fixture = Fixture::new();
    fixture.trust();
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let store = Arc::new(ChangingStore {
        inner: fixture.store.clone(),
        change: Mutex::new(Some(Box::new(move || flag.store(true, Ordering::SeqCst)))),
    });
    assert_eq!(
        error(prepare(
            &fixture.request,
            store,
            &cancelled,
            Instant::now() + Duration::from_secs(3)
        )),
        CliError::Cancelled
    );
}

// This exact test is also a portable native child fixture. No shell, network or real vault.
#[test]
fn process_fixture() {
    let Ok(mode) = std::env::var("CC_SWITCH_EXEC_TEST_CHILD") else {
        return;
    };
    if mode == "wait" {
        std::thread::sleep(Duration::from_secs(30));
    } else {
        std::process::exit(mode.parse().unwrap());
    }
}

fn child(mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "services::vps::ssh::exec::tests::process_fixture",
            "--nocapture",
        ])
        .env("CC_SWITCH_EXEC_TEST_CHILD", mode);
    command
}

#[test]
fn native_runner_preserves_exit_status_and_bounds_timeout() {
    let cancelled = AtomicBool::new(false);
    for code in [0, 37, 255] {
        assert_eq!(
            run_command(
                child(&code.to_string()),
                None,
                &cancelled,
                Instant::now() + Duration::from_secs(5)
            )
            .unwrap(),
            code
        );
    }
    let before = Instant::now();
    assert_eq!(
        run_command(
            child("wait"),
            None,
            &cancelled,
            before + Duration::from_millis(150)
        ),
        Err(CliError::Timeout)
    );
    assert!(before.elapsed() < Duration::from_secs(5));
}

#[test]
fn native_runner_handles_missing_ssh_and_cancellation_before_spawn() {
    let missing = Command::new("cc-switch-nonexistent-ssh-test-executable");
    assert_eq!(
        run_command(
            missing,
            None,
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(1)
        ),
        Err(CliError::SshNotFound)
    );
    assert_eq!(
        run_command(
            child("wait"),
            None,
            &AtomicBool::new(true),
            Instant::now() + Duration::from_secs(1)
        ),
        Err(CliError::Cancelled)
    );
}
