use super::*;
use std::collections::VecDeque;

fn server() -> VpsServer {
    let mut server = VpsServer::new("test".into(), "192.0.2.10".into(), "deploy".into());
    server.port = 2222;
    server
}

fn public_key(byte: u8) -> PublicHostKey {
    let mut bytes = Vec::new();
    let kind = b"ssh-ed25519";
    bytes.extend_from_slice(&(kind.len() as u32).to_be_bytes());
    bytes.extend_from_slice(kind);
    bytes.extend_from_slice(&32u32.to_be_bytes());
    bytes.extend_from_slice(&[byte; 32]);
    PublicHostKey {
        kind: "ssh-ed25519".into(),
        encoded: STANDARD.encode(bytes),
    }
}

fn success(stdout: impl Into<Vec<u8>>) -> Result<ProcessOutput, ProcessFailure> {
    Ok(ProcessOutput {
        code: Some(0),
        stdout: stdout.into(),
        stderr: Vec::new(),
    })
}

fn scan(key: &PublicHostKey) -> Result<ProcessOutput, ProcessFailure> {
    success(format!("[192.0.2.10]:2222 {} {}\n", key.kind, key.encoded).into_bytes())
}

#[derive(Default)]
struct MockRunner {
    results: Mutex<VecDeque<Result<ProcessOutput, ProcessFailure>>>,
    calls: Mutex<Vec<CommandSpec>>,
    configs: Mutex<Vec<String>>,
    known_hosts: Mutex<Vec<String>>,
}

impl MockRunner {
    fn push(&self, result: Result<ProcessOutput, ProcessFailure>) {
        self.results.lock().unwrap().push_back(result);
    }
}

impl ProcessRunner for MockRunner {
    fn run(
        &self,
        spec: &CommandSpec,
        _: &AtomicBool,
        _: Instant,
    ) -> Result<ProcessOutput, ProcessFailure> {
        self.calls.lock().unwrap().push(spec.clone());
        if let Some(position) = spec.args.iter().position(|arg| arg == "-F") {
            let path = PathBuf::from(&spec.args[position + 1]);
            self.configs
                .lock()
                .unwrap()
                .push(fs::read_to_string(&path).unwrap());
            self.known_hosts
                .lock()
                .unwrap()
                .push(fs::read_to_string(path.with_file_name("known_hosts")).unwrap());
        }
        self.results
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected process invocation")
    }
}

fn setup() -> (tempfile::TempDir, VpsSshState, Arc<MockRunner>) {
    let home = tempfile::tempdir().unwrap();
    let runner = Arc::new(MockRunner::default());
    let state = VpsSshState::for_test(home.path().join("vps with spaces 日本語"), runner.clone());
    (home, state, runner)
}

fn request() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn first_confirmation(
    state: &VpsSshState,
    runner: &MockRunner,
    server: &VpsServer,
    key: &PublicHostKey,
) -> String {
    runner.push(success(Vec::new()));
    runner.push(scan(key));
    let result = state.test_blocking(server.clone(), request());
    assert_eq!(
        result.status,
        VpsConnectionStatus::HostKeyConfirmationRequired
    );
    assert_eq!(
        result.fingerprint.as_deref(),
        Some(key.fingerprint().unwrap().as_str())
    );
    result.confirmation_token.unwrap()
}

#[test]
fn scan_uses_arguments_and_the_requested_port_without_shell_interpolation() {
    let server = server();
    let spec = scan_command(&server);
    assert_eq!(spec.program, "ssh-keyscan");
    assert_eq!(
        spec.args,
        [
            "-T",
            "5",
            "-p",
            "2222",
            "-t",
            "ed25519,ecdsa,rsa",
            "192.0.2.10"
        ]
        .map(OsString::from)
    );
}

#[test]
fn first_contact_requires_confirmation_before_authentication() {
    let (_home, state, runner) = setup();
    let server = server();
    let key = public_key(1);
    let token = first_confirmation(&state, &runner, &server, &key);
    assert_eq!(runner.calls.lock().unwrap().len(), 2);
    assert!(!state.root().join("known_hosts").exists());
    state.confirm_host_key(&token).unwrap();
    assert!(
        state.confirm_host_key(&token).is_err(),
        "tokens are consumed once"
    );
    let known = fs::read_to_string(state.root().join("known_hosts")).unwrap();
    assert!(known.contains(&format!(
        "{} {} {}\n",
        server.ssh_alias(),
        key.kind,
        key.encoded
    )));
    assert!(
        !known.contains(&format!("[{}]:2222", server.ssh_alias())),
        "HostKeyAlias is used verbatim by OpenSSH"
    );
    runner.push(success(Vec::new()));
    runner.push(success(Vec::new()));
    assert_eq!(
        state.test_blocking(server, request()).status,
        VpsConnectionStatus::Success
    );
}

#[test]
fn trust_survives_restart_but_no_password_or_key_contents_are_stored() {
    let (_home, state, runner) = setup();
    let mut server = server();
    server.identity_file = Some(state.root().parent().unwrap().join("private key 秘密"));
    let token = first_confirmation(&state, &runner, &server, &public_key(2));
    state.confirm_host_key(&token).unwrap();
    let saved = fs::read_to_string(state.root().join("ssh-trust.json")).unwrap();
    assert!(!saved.contains("private key"));
    assert!(!saved.contains("deploy"));
    let reopened = VpsSshState::for_test(state.root(), runner.clone());
    runner.push(success(Vec::new()));
    runner.push(success(Vec::new()));
    assert_eq!(
        reopened.test_blocking(server, request()).status,
        VpsConnectionStatus::Success
    );
}

#[test]
fn fixed_probe_uses_private_config_and_does_not_run_user_commands() {
    let (_home, state, runner) = setup();
    let mut server = server();
    server.identity_file = Some(
        state
            .root()
            .parent()
            .unwrap()
            .join("key with spaces 日本語"),
    );
    let token = first_confirmation(&state, &runner, &server, &public_key(3));
    state.confirm_host_key(&token).unwrap();
    runner.push(success(Vec::new()));
    runner.push(success(Vec::new()));
    let alias = server.ssh_alias();
    assert_eq!(
        state.test_blocking(server, request()).status,
        VpsConnectionStatus::Success
    );
    let calls = runner.calls.lock().unwrap();
    let probe = calls.last().unwrap();
    assert_eq!(probe.program, "ssh");
    assert!(
        PathBuf::from(&probe.args[1]).starts_with(state.root()),
        "probe configuration must stay inside the injected temporary root"
    );
    assert_eq!(
        &probe.args[probe.args.len() - 2..],
        [OsString::from(alias), OsString::from("true")]
    );
    let configs = runner.configs.lock().unwrap();
    let config = configs.last().unwrap();
    for setting in [
        "StrictHostKeyChecking yes",
        "GlobalKnownHostsFile none",
        "KnownHostsCommand none",
        "ProxyCommand none",
        "ProxyJump none",
        "PermitLocalCommand no",
        "ControlPath none",
        "BatchMode yes",
        "ForwardAgent no",
        "ClearAllForwardings yes",
        "UpdateHostKeys no",
        "Port 2222",
    ] {
        assert!(config.contains(setting), "missing {setting}");
    }
    assert!(config.contains("key with spaces 日本語"));
    assert!(!config.contains("StrictHostKeyChecking no"));
    assert!(!state.root().join("servers.json").exists());
    assert!(!state.root().join("ssh_config").exists());
}

#[test]
fn changed_key_is_blocked_without_a_new_confirmation_token() {
    let (_home, state, runner) = setup();
    let server = server();
    let token = first_confirmation(&state, &runner, &server, &public_key(4));
    state.confirm_host_key(&token).unwrap();
    let before = fs::read(state.root().join("known_hosts")).unwrap();
    runner.push(success(Vec::new()));
    runner.push(Ok(ProcessOutput {
        code: Some(255),
        stdout: Vec::new(),
        stderr: b"REMOTE HOST IDENTIFICATION HAS CHANGED!".to_vec(),
    }));
    let result = state.test_blocking(server, request());
    assert_eq!(result.status, VpsConnectionStatus::HostKeyChanged);
    assert!(result.confirmation_token.is_none());
    assert_eq!(fs::read(state.root().join("known_hosts")).unwrap(), before);
}

#[test]
fn stale_or_expired_confirmation_never_overwrites_an_existing_pin() {
    let (_home, state, runner) = setup();
    let server = server();
    let first = first_confirmation(&state, &runner, &server, &public_key(5));
    let second = first_confirmation(&state, &runner, &server, &public_key(6));
    state.confirm_host_key(&first).unwrap();
    let before = fs::read(state.root().join("known_hosts")).unwrap();
    assert!(state.confirm_host_key(&second).is_err());
    assert_eq!(fs::read(state.root().join("known_hosts")).unwrap(), before);
    let other = VpsServer::new("another".into(), "192.0.2.11".into(), "deploy".into());
    let expired = first_confirmation(&state, &runner, &other, &public_key(7));
    state
        .inner
        .requests
        .lock()
        .unwrap()
        .confirmations
        .get_mut(&expired)
        .unwrap()
        .expires_at = Instant::now() - Duration::from_secs(1);
    assert!(state.confirm_host_key(&expired).is_err());
}

#[test]
fn cancellation_before_start_or_after_scan_cannot_be_ignored() {
    let (_home, state, runner) = setup();
    let id = request();
    state.cancel_test(&id).unwrap();
    assert_eq!(
        state.test_blocking(server(), id).status,
        VpsConnectionStatus::Cancelled
    );
    assert!(runner.calls.lock().unwrap().is_empty());
    runner.push(success(Vec::new()));
    runner.push(scan(&public_key(8)));
    let id = request();
    let result = state.test_blocking(server(), id.clone());
    state.cancel_test(&id).unwrap();
    assert!(state
        .confirm_host_key(&result.confirmation_token.unwrap())
        .is_err());
}

#[test]
fn missing_tools_timeout_and_output_limit_have_distinct_safe_results() {
    for (failure, expected) in [
        (
            ProcessFailure::NotFound("ssh"),
            VpsConnectionStatus::SshNotFound,
        ),
        (ProcessFailure::Timeout, VpsConnectionStatus::Timeout),
        (ProcessFailure::Cancelled, VpsConnectionStatus::Cancelled),
        (ProcessFailure::OutputLimit, VpsConnectionStatus::Failed),
        (ProcessFailure::Io, VpsConnectionStatus::Failed),
    ] {
        let (_home, state, runner) = setup();
        runner.push(Err(failure));
        assert_eq!(state.test_blocking(server(), request()).status, expected);
    }
}

#[test]
fn authentication_errors_do_not_echo_untrusted_stderr() {
    let (_home, state, runner) = setup();
    let server = server();
    let token = first_confirmation(&state, &runner, &server, &public_key(9));
    state.confirm_host_key(&token).unwrap();
    runner.push(success(Vec::new()));
    runner.push(Ok(ProcessOutput {
        code: Some(255),
        stdout: b"untrusted output".to_vec(),
        stderr: b"Permission denied (publickey). \x1b[31m do something else".to_vec(),
    }));
    let result = state.test_blocking(server, request());
    assert_eq!(result.status, VpsConnectionStatus::AuthenticationFailed);
    let message = result.message.unwrap();
    assert!(!message.contains("do something else"));
    assert!(!message.contains('\x1b'));
}

#[test]
fn invalid_parameters_never_start_a_process() {
    let (_home, state, runner) = setup();
    let mut invalid = server();
    invalid.host = "-oProxyCommand=bad".into();
    assert_eq!(
        state.test_blocking(invalid, request()).status,
        VpsConnectionStatus::Failed
    );
    assert_eq!(
        state.test_blocking(server(), "not-a-uuid".into()).status,
        VpsConnectionStatus::Failed
    );
    assert!(runner.calls.lock().unwrap().is_empty());
}

#[test]
fn malformed_or_conflicting_keyscan_output_cannot_be_confirmed() {
    for output in [
        "host ssh-ed25519 not-base64\n".to_string(),
        "# comment only\n".into(),
        format!(
            "host ssh-ed25519 {}\nhost ssh-ed25519 {}\n",
            public_key(1).encoded,
            public_key(2).encoded
        ),
    ] {
        let (_home, state, runner) = setup();
        runner.push(success(Vec::new()));
        runner.push(success(output.into_bytes()));
        let result = state.test_blocking(server(), request());
        assert_eq!(result.status, VpsConnectionStatus::Failed);
        assert!(result.confirmation_token.is_none());
    }
}

#[test]
fn manual_trust_changes_and_unowned_known_hosts_are_preserved() {
    let (_home, state, runner) = setup();
    fs::create_dir_all(state.root()).unwrap();
    fs::write(state.root().join("known_hosts"), b"user owned").unwrap();
    assert_eq!(
        state.test_blocking(server(), request()).status,
        VpsConnectionStatus::Failed
    );
    assert_eq!(
        fs::read(state.root().join("known_hosts")).unwrap(),
        b"user owned"
    );
    assert!(runner.calls.lock().unwrap().is_empty());
    fs::remove_file(state.root().join("known_hosts")).unwrap();
    let token = first_confirmation(&state, &runner, &server(), &public_key(10));
    state.confirm_host_key(&token).unwrap();
    fs::write(state.root().join("known_hosts"), b"user changed").unwrap();
    assert_eq!(
        state.test_blocking(server(), request()).status,
        VpsConnectionStatus::Failed
    );
    assert_eq!(
        fs::read(state.root().join("known_hosts")).unwrap(),
        b"user changed"
    );
}

#[test]
fn json_trust_edits_without_matching_receipt_are_rejected() {
    let (_home, state, runner) = setup();
    let server = server();
    let token = first_confirmation(&state, &runner, &server, &public_key(11));
    state.confirm_host_key(&token).unwrap();
    let path = state.root().join("ssh-trust.json");
    let mut json: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    json["hosts"][&server.id]["port"] = serde_json::json!(42);
    fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    assert_eq!(
        state.test_blocking(server, request()).status,
        VpsConnectionStatus::Failed
    );
}

#[test]
fn interrupted_known_hosts_write_recovers_only_recognized_content() {
    let (_home, state, runner) = setup();
    let server = server();
    let token = first_confirmation(&state, &runner, &server, &public_key(12));
    state.confirm_host_key(&token).unwrap();
    let store = TrustStore { root: state.root() };
    let (mut document, _, old_known) = store.load_unlocked().unwrap();
    let other = VpsServer::new("other".into(), "192.0.2.11".into(), "deploy".into());
    document.hosts.insert(
        other.id.clone(),
        TrustedHost::from_server(&other, public_key(13)),
    );
    document.refresh_receipt().unwrap();
    document.previous_known_hosts_hash = Some(hash_bytes(old_known.as_deref().unwrap()));
    fs::write(
        store.root.join("ssh-trust.json"),
        sorted_json_bytes(&document).unwrap(),
    )
    .unwrap();
    store.load().unwrap();
    assert!(fs::read_to_string(store.root.join("known_hosts"))
        .unwrap()
        .contains(&other.ssh_alias()));
    let document: TrustDocument =
        serde_json::from_slice(&fs::read(store.root.join("ssh-trust.json")).unwrap()).unwrap();
    assert!(document.previous_known_hosts_hash.is_none());
}

#[test]
fn active_cancellation_reaches_the_runner_and_releases_the_request() {
    struct WaitingRunner(Arc<(Mutex<bool>, std::sync::Condvar)>);
    impl ProcessRunner for WaitingRunner {
        fn run(
            &self,
            _: &CommandSpec,
            cancelled: &AtomicBool,
            deadline: Instant,
        ) -> Result<ProcessOutput, ProcessFailure> {
            let (entered, changed) = &*self.0;
            *entered.lock().unwrap() = true;
            changed.notify_all();
            while !cancelled.load(Ordering::SeqCst) {
                if Instant::now() >= deadline {
                    return Err(ProcessFailure::Timeout);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(ProcessFailure::Cancelled)
        }
    }
    let home = tempfile::tempdir().unwrap();
    let entered = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let state = VpsSshState::for_test(
        home.path().join("vps"),
        Arc::new(WaitingRunner(entered.clone())),
    );
    let worker_state = state.clone();
    let id = request();
    let worker_id = id.clone();
    let worker = std::thread::spawn(move || worker_state.test_blocking(server(), worker_id));
    let (flag, changed) = &*entered;
    let (started, _) = changed
        .wait_timeout_while(flag.lock().unwrap(), Duration::from_secs(2), |started| {
            !*started
        })
        .unwrap();
    assert!(*started, "mock process was not reached");
    drop(started);
    state.cancel_test(&id).unwrap();
    assert_eq!(
        worker.join().unwrap().status,
        VpsConnectionStatus::Cancelled
    );
    assert!(state.inner.requests.lock().unwrap().active.is_empty());
}

fn fixture_command(spin: bool) -> CommandSpec {
    #[cfg(windows)]
    {
        CommandSpec {
            program: "cmd.exe",
            args: vec![
                "/D".into(),
                "/C".into(),
                if spin {
                    "for /L %i in (1,1,1000000000) do @rem fixture".into()
                } else {
                    "echo output&echo error 1>&2&exit /b 7".into()
                },
            ],
        }
    }
    #[cfg(not(windows))]
    {
        CommandSpec {
            program: "sh",
            args: vec![
                "-c".into(),
                if spin {
                    "while :; do :; done".into()
                } else {
                    "printf output; printf error >&2; exit 7".into()
                },
            ],
        }
    }
}

#[test]
fn native_runner_captures_exit_status_and_output_from_a_local_substitute() {
    let output = OpenSshRunner
        .run(
            &fixture_command(false),
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(output.code, Some(7));
    assert!(String::from_utf8(output.stdout).unwrap().contains("output"));
    assert!(String::from_utf8(output.stderr).unwrap().contains("error"));
}

#[test]
fn native_runner_times_out_and_reaps_a_local_substitute() {
    let result = OpenSshRunner.run(
        &fixture_command(true),
        &AtomicBool::new(false),
        Instant::now() + Duration::from_millis(100),
    );
    assert!(matches!(result, Err(ProcessFailure::Timeout)));
}

#[test]
fn native_runner_cancels_and_reaps_a_local_substitute() {
    let flag = Arc::new(AtomicBool::new(false));
    let worker_flag = flag.clone();
    let worker = std::thread::spawn(move || {
        OpenSshRunner.run(
            &fixture_command(true),
            &worker_flag,
            Instant::now() + Duration::from_secs(2),
        )
    });
    std::thread::sleep(Duration::from_millis(100));
    flag.store(true, Ordering::SeqCst);
    assert!(matches!(
        worker.join().unwrap(),
        Err(ProcessFailure::Cancelled)
    ));
}

#[test]
fn process_output_capture_rejects_oversized_data() {
    use std::io::Write;
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&vec![b'x'; MAX_OUTPUT as usize + 1])
        .unwrap();
    assert!(matches!(
        read_process_output(&mut file),
        Err(ProcessFailure::OutputLimit)
    ));
}

#[cfg(unix)]
#[test]
fn trust_symlinks_are_not_followed() {
    let (home, state, runner) = setup();
    let external = home.path().join("external");
    fs::create_dir_all(&external).unwrap();
    std::os::unix::fs::symlink(&external, state.root()).unwrap();
    assert_eq!(
        state.test_blocking(server(), request()).status,
        VpsConnectionStatus::Failed
    );
    assert!(runner.calls.lock().unwrap().is_empty());
    assert!(fs::read_dir(&external).unwrap().next().is_none());
}
