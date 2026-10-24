use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::json;

use super::*;

/// 全部落在临时目录里：CC Switch 数据目录、假的 auth 目录、假的 keyblob。
struct Fixture {
    _dir: tempfile::TempDir,
    data_dir: PathBuf,
    paths: WorkBuddyPaths,
    running: Arc<AtomicBool>,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("cc-switch");
        let paths = WorkBuddyPaths {
            auth_dir: dir.path().join("CodeBuddyExtension/Data/Public/auth"),
            keyblob: dir.path().join(".workbuddy/keyblob"),
            profile_snapshot: dir
                .path()
                .join(".workbuddy/storage/skeleton/account-snapshot.json"),
            wbipc_discovery: dir.path().join(".workbuddy/wbipc/endpoint.json"),
        };
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&paths.auth_dir).unwrap();
        std::fs::create_dir_all(paths.keyblob.parent().unwrap()).unwrap();
        Self {
            _dir: dir,
            data_dir,
            paths,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    fn manager(&self) -> WorkBuddyAccountManager {
        let running = Arc::clone(&self.running);
        WorkBuddyAccountManager::with_paths(
            self.data_dir.clone(),
            self.paths.clone(),
            Arc::new(move || running.load(Ordering::SeqCst)),
        )
    }

    /// 模拟 WorkBuddy 登着 `uid`：写 `.info`、观测文件和 keyblob。
    fn sign_in(&self, uid: &str, generation: u32) {
        std::fs::write(self.paths.info(), info_doc(uid, generation)).unwrap();
        for entry in std::fs::read_dir(&self.paths.auth_dir).unwrap().flatten() {
            if is_observation_name(&entry.file_name().to_string_lossy()) {
                std::fs::remove_file(entry.path()).unwrap();
            }
        }
        std::fs::write(
            self.paths
                .auth_dir
                .join("workbuddy-desktop.info.abcdef0123456789.observation.json"),
            format!("{{\"owner\":\"{uid}\",\"generation\":{generation}}}"),
        )
        .unwrap();
        std::fs::write(&self.paths.keyblob, format!("keyblob-{uid}-{generation}")).unwrap();
        let _ = std::fs::remove_file(self.paths.logout_marker());
    }

    /// 模拟 WorkBuddy 写的明文登录画像（`account-snapshot.json`）。
    fn write_profile(&self, uid: &str, nickname: &str, edition: &str) {
        std::fs::create_dir_all(self.paths.profile_snapshot.parent().unwrap()).unwrap();
        std::fs::write(
            &self.paths.profile_snapshot,
            serde_json::to_vec(&json!({
                "primary": {
                    "version": 1,
                    "uid": uid,
                    "nickname": nickname,
                    "type": "personal",
                    "editionType": edition,
                }
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn info(&self) -> String {
        std::fs::read_to_string(self.paths.info()).unwrap()
    }

    fn keyblob(&self) -> String {
        std::fs::read_to_string(&self.paths.keyblob).unwrap()
    }

    fn observation(&self) -> String {
        std::fs::read_to_string(
            self.paths
                .auth_dir
                .join("workbuddy-desktop.info.abcdef0123456789.observation.json"),
        )
        .unwrap()
    }
}

fn envelope() -> Value {
    json!({"$wbEncrypted": 1, "envelope": "opaque"})
}

fn info_doc(uid: &str, generation: u32) -> String {
    serde_json::to_string_pretty(&json!({
        "account": {
            "uid": uid,
            "type": "personal",
            "nickname": envelope(),
            "phoneNumber": envelope(),
        },
        "auth": {
            "accessToken": envelope(),
            "refreshToken": envelope(),
            "expiresAt": 4_102_444_800_000_i64 + i64::from(generation),
            "refreshExpiresAt": 4_102_444_800_000_i64,
        },
        "accounts": [],
        "allAccounts": [],
    }))
    .unwrap()
}

#[test]
fn reads_only_plaintext_identity_fields() {
    let doc: Value = serde_json::from_str(&info_doc("u-1", 0)).unwrap();
    let parsed = identity(&doc).unwrap();
    assert_eq!(parsed.id, "u-1");
    assert_eq!(parsed.kind.as_deref(), Some("personal"));
    // 昵称是加密信封，读不出来就当没有，界面显示 uid
    assert_eq!(parsed.nickname, None);
    assert_eq!(parsed.refresh_expires_at, Some(4_102_444_800));

    let encrypted_uid = json!({"account": {"uid": envelope()}});
    assert!(identity(&encrypted_uid).is_none());
    let uin_only = json!({"account": {"uid": envelope(), "uin": 12345}});
    assert_eq!(identity(&uin_only).unwrap().id, "uin:12345");
    assert_eq!(
        plain_timestamp(Some(&json!(1_700_000_000))),
        Some(1_700_000_000)
    );
    assert_eq!(
        plain_timestamp(Some(&json!("1700000000000"))),
        Some(1_700_000_000)
    );
}

#[test]
fn fills_nickname_and_plan_from_the_plaintext_profile() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    fixture.sign_in("u-1", 0);
    // 画像匹配当前登录：昵称和版型补进来
    fixture.write_profile("u-1", "Ada", "pro");

    let account = manager.import_current().unwrap().account;
    assert_eq!(account.label(), "Ada");
    assert_eq!(account.plan.as_deref(), Some("pro"));

    // 切到一个画像对不上的号：u-1 的昵称/版型作为缓存保留，不被清掉
    fixture.sign_in("u-2", 0);
    fixture.write_profile("u-2", "Bob", "free");
    let status = manager.status().unwrap();
    let u1 = status.accounts.iter().find(|a| a.id == "u-1").unwrap();
    assert_eq!(u1.nickname.as_deref(), Some("Ada"));
    assert_eq!(u1.plan.as_deref(), Some("pro"));
    let u2 = status.accounts.iter().find(|a| a.id == "u-2").unwrap();
    assert_eq!(u2.label(), "Bob");
    assert_eq!(u2.plan.as_deref(), Some("free"));

    // 画像 uid 对不上当前登录时忽略（不会把别人的昵称安到当前号上）
    fixture.write_profile("someone-else", "Eve", "team");
    let again = manager.status().unwrap();
    let u2 = again.accounts.iter().find(|a| a.id == "u-2").unwrap();
    assert_eq!(u2.label(), "Bob");
}

#[test]
fn observation_names_are_scoped_to_workbuddy_desktop() {
    assert!(is_observation_name(
        "workbuddy-desktop.info.5d63bdd69323e571.observation.json"
    ));
    assert!(!is_observation_name(
        "workbuddy-desktop-ai.info.4e3ccffb96908f46.observation.json"
    ));
    assert!(!is_observation_name(
        "workbuddy-desktop.info.observation.json"
    ));
    assert!(!is_observation_name(
        "workbuddy-desktop.info.../x.observation.json"
    ));
}

#[test]
fn detects_workbuddy_processes_but_not_look_alikes() {
    assert!(is_workbuddy_process(
        "/Applications/WorkBuddy.app/Contents/MacOS/Electron"
    ));
    assert!(is_workbuddy_process(
        "/Applications/WorkBuddy.app/Contents/Frameworks/WorkBuddy Helper.app/Contents/MacOS/WorkBuddy Helper"
    ));
    assert!(is_workbuddy_process("/opt/WorkBuddy/workbuddy"));
    assert!(!is_workbuddy_process(
        "/tmp/target/bundle/macos/CC Switch Dev WorkBuddy.app/Contents/MacOS/cc-switch"
    ));
    assert!(!is_workbuddy_process(
        "/Applications/CodeBuddy.app/Contents/MacOS/Electron"
    ));
}

#[test]
fn import_requires_a_signed_in_workbuddy() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    assert!(manager.import_current().is_err());

    fixture.sign_in("u-1", 0);
    std::fs::write(fixture.paths.logout_marker(), "2026-01-01T00:00:00.000Z").unwrap();
    assert!(manager.import_current().is_err(), "登出标记在时视为没登录");
    assert!(!manager.live_state().signed_in);
}

#[test]
fn imports_the_current_account_with_its_whole_bundle() {
    let fixture = Fixture::new();
    fixture.sign_in("u-1", 0);
    let manager = fixture.manager();

    let outcome = manager.import_current().unwrap();
    assert!(outcome.created);
    assert_eq!(outcome.account.label(), "u-1");
    assert!(!outcome.account.expired);
    let again = manager.import_current().unwrap();
    assert!(!again.created);

    let status = manager.status().unwrap();
    assert_eq!(status.accounts.len(), 1);
    assert_eq!(status.current_account_id.as_deref(), Some("u-1"));

    // 账号表 0600、能重新加载
    let store_path = fixture.data_dir.join(STORE_FILENAME);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&store_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let reloaded = fixture.manager();
    let data = reloaded.lock().accounts.get("u-1").cloned().unwrap();
    assert_eq!(
        decode(&data.bundle.info).unwrap(),
        info_doc("u-1", 0).as_bytes()
    );
    assert_eq!(data.bundle.observations.len(), 1);
    assert_eq!(
        decode(data.bundle.keyblob.as_deref().unwrap()).unwrap(),
        b"keyblob-u-1-0"
    );
}

#[test]
fn status_auto_absorbs_the_current_login_and_keeps_it_fresh() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    fixture.sign_in("u-1", 0);

    // 没有手动收录，光读 status 就把当前登录收了进来
    let status = manager.status().unwrap();
    assert_eq!(status.accounts.len(), 1);
    assert_eq!(status.current_account_id.as_deref(), Some("u-1"));

    // WorkBuddy 刷新 token（换了 keyblob/.info 的字节）后，再读 status 把快照更新成最新
    fixture.sign_in("u-1", 5);
    manager.status().unwrap();
    let reloaded = fixture.manager();
    let data = reloaded.lock().accounts.get("u-1").cloned().unwrap();
    assert_eq!(
        decode(&data.bundle.info).unwrap(),
        info_doc("u-1", 5).as_bytes()
    );
    assert_eq!(
        decode(data.bundle.keyblob.as_deref().unwrap()).unwrap(),
        b"keyblob-u-1-5"
    );
}

#[test]
fn removed_account_is_not_auto_absorbed_until_re_imported() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    fixture.sign_in("u-1", 0);
    manager.status().unwrap();

    // 删掉后，即便 WorkBuddy 还登着它，自动收录也不把它加回来
    manager.remove("u-1").unwrap();
    let status = manager.status().unwrap();
    assert!(status.accounts.is_empty());
    assert_eq!(status.current_account_id, None);

    // 「删除」标记要持久化：重开 manager 仍然不自动加回来
    let reopened = fixture.manager();
    assert!(reopened.status().unwrap().accounts.is_empty());

    // 手动「收录当前登录」是明确意愿，解除标记后又能自动保持
    reopened.import_current().unwrap();
    assert_eq!(
        reopened.status().unwrap().current_account_id.as_deref(),
        Some("u-1")
    );
}

#[test]
fn switching_away_recaptures_even_a_removed_live_login() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    fixture.sign_in("u-1", 0);
    manager.import_current().unwrap();
    fixture.sign_in("u-2", 0);
    manager.status().unwrap();

    // 用户删掉了当前登着的 u-2，但它仍是 WorkBuddy 的 live 登录
    manager.remove("u-2").unwrap();
    assert!(!manager
        .status()
        .unwrap()
        .accounts
        .iter()
        .any(|account| account.id == "u-2"));

    // 切到 u-1 时要把被删却仍在用的 u-2 收回来，免得这份登录丢掉
    manager.switch_to("u-1").unwrap();
    assert!(manager
        .status()
        .unwrap()
        .accounts
        .iter()
        .any(|account| account.id == "u-2"));
    manager.switch_to("u-2").unwrap();
    assert_eq!(fixture.info(), info_doc("u-2", 0));
}

#[test]
fn unrecognized_login_is_stashed_not_imported() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.paths.info(),
        serde_json::to_vec(&json!({"account": {"uid": envelope()}})).unwrap(),
    )
    .unwrap();
    let manager = fixture.manager();
    assert!(manager.import_current().is_err());
    assert!(fixture.data_dir.join(STASH_FILENAME).exists());
    assert!(manager.status().unwrap().accounts.is_empty());
    let live = manager.live_state();
    assert!(live.signed_in && !live.recognized);
}

#[test]
fn switch_captures_current_then_restores_target_group() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    fixture.sign_in("u-1", 0);
    manager.import_current().unwrap();
    fixture.sign_in("u-2", 0);
    manager.import_current().unwrap();

    // WorkBuddy 刷新过 u-2 的 token、轮换了 keyblob
    fixture.sign_in("u-2", 7);

    manager.switch_to("u-1").unwrap();
    assert_eq!(fixture.info(), info_doc("u-1", 0));
    assert_eq!(fixture.keyblob(), "keyblob-u-1-0");
    assert!(fixture.observation().contains("u-1"));
    assert_eq!(
        manager.status().unwrap().current_account_id.as_deref(),
        Some("u-1")
    );

    // 切回 u-2 时用的是切走前收回的最新那组
    manager.switch_to("u-2").unwrap();
    assert_eq!(fixture.info(), info_doc("u-2", 7));
    assert_eq!(fixture.keyblob(), "keyblob-u-2-7");
}

#[test]
fn switch_clears_the_logout_marker() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    fixture.sign_in("u-1", 0);
    manager.import_current().unwrap();
    manager.prepare_new_login().unwrap();
    assert!(fixture.paths.logout_marker().exists());
    // `.info` 原样留着，只是 WorkBuddy 会无视它
    assert_eq!(fixture.info(), info_doc("u-1", 0));
    assert_eq!(manager.status().unwrap().current_account_id, None);

    manager.switch_to("u-1").unwrap();
    assert!(!fixture.paths.logout_marker().exists());
    assert_eq!(
        manager.status().unwrap().current_account_id.as_deref(),
        Some("u-1")
    );
}

#[test]
fn switch_refuses_while_workbuddy_runs_or_holds_the_lock() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    fixture.sign_in("u-1", 0);
    manager.import_current().unwrap();
    fixture.sign_in("u-2", 0);
    manager.import_current().unwrap();

    fixture.running.store(true, Ordering::SeqCst);
    assert!(manager.switch_to("u-1").is_err());
    assert!(manager.prepare_new_login().is_err());
    assert_eq!(fixture.info(), info_doc("u-2", 0), "运行中不能写任何文件");
    fixture.running.store(false, Ordering::SeqCst);

    std::fs::create_dir_all(fixture.paths.lock_dir()).unwrap();
    assert!(manager.switch_to("u-1").is_err());
    std::fs::remove_dir(fixture.paths.lock_dir()).unwrap();

    manager.switch_to("u-1").unwrap();
}

#[test]
fn switching_away_from_an_unknown_login_keeps_it() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    fixture.sign_in("u-1", 0);
    manager.import_current().unwrap();
    // 用户在 WorkBuddy 里登了一个还没收录的号
    fixture.sign_in("u-9", 0);

    manager.switch_to("u-1").unwrap();
    let status = manager.status().unwrap();
    assert!(status.accounts.iter().any(|account| account.id == "u-9"));
    manager.switch_to("u-9").unwrap();
    assert_eq!(fixture.info(), info_doc("u-9", 0));
}

#[test]
fn expired_accounts_cannot_be_switched_to() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let doc = json!({
        "account": {"uid": "old"},
        "auth": {"refreshExpiresAt": 1_000_000_000_000_i64},
    });
    std::fs::write(fixture.paths.info(), serde_json::to_vec(&doc).unwrap()).unwrap();
    assert!(manager.import_current().unwrap().account.expired);
    fixture.sign_in("u-1", 0);
    assert!(manager.switch_to("old").is_err());
    assert_eq!(fixture.info(), info_doc("u-1", 0));
}

#[test]
fn removing_only_drops_cc_switch_copies() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    fixture.sign_in("u-1", 0);
    manager.import_current().unwrap();
    manager.remove("u-1").unwrap();
    assert!(manager.remove("u-1").is_err());
    assert_eq!(fixture.info(), info_doc("u-1", 0));
    assert!(!fixture.paths.logout_marker().exists());
    assert_eq!(fixture.keyblob(), "keyblob-u-1-0");
}

#[test]
fn restore_rejects_tampered_observation_names() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let bundle = Bundle {
        info: STANDARD.encode(b"{}"),
        observations: BTreeMap::from([(
            "../../escape.observation.json".to_string(),
            STANDARD.encode(b"x"),
        )]),
        keyblob: None,
    };
    assert!(manager.restore(&bundle).is_err());
    assert!(!fixture.paths.info().exists());
}

// ---------- 额度（WBIPC） ----------

#[test]
fn parses_the_resource_summary_like_workbuddy_does() {
    let body = json!({
        "code": 0,
        "msg": "",
        "data": {
            "Packages": [
                {"CycleTotalCapacity": "1000", "CycleRemainCapacity": "250.5", "CycleUsedCapacity": "749.5"},
                {"CycleTotalCapacity": 500, "CycleRemainCapacity": 500, "CycleUsedCapacity": "0"},
                {"CycleTotalCapacity": "-3", "CycleRemainCapacity": "NaN", "CycleUsedCapacity": null},
            ],
            "SubscriptionPackageName": " 体验版 ",
            "IsPaidUser": false,
        }
    });
    let snapshot = quota::parse_summary(&serde_json::to_vec(&body).unwrap(), 42).expect("parses");
    assert_eq!(
        snapshot,
        WorkBuddyQuotaSnapshot {
            plan_name: Some("体验版".into()),
            is_paid: false,
            total: 1500.0,
            remaining: 750.5,
            used: 749.5,
            queried_at: 42,
        }
    );

    let refused = json!({"code": 11001, "msg": "invalid", "data": null});
    assert!(matches!(
        quota::parse_summary(&serde_json::to_vec(&refused).unwrap(), 0),
        Err(quota::FetchError::Failed(_))
    ));
}

/// 已知答案：和 WorkBuddy 自带的 `workbuddy_ipc` 客户端算法逐字节一致（同一算法对真实 WorkBuddy 握手通过）。
#[test]
fn wbipc_handshake_proofs_match_workbuddy() {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use hmac::Mac as _;
    let proof = |server| {
        URL_SAFE_NO_PAD.encode(
            quota::proof_mac("kat-ticket", server, "/tmp/wbipc/b-1.sock", "cn", "sn")
                .finalize()
                .into_bytes(),
        )
    };
    assert_eq!(proof(true), "sILCVa9XnCRcCgWrcPty_T1RVIw0TmYoWGVHAG8z0z8");
    assert_eq!(proof(false), "RkKbgsulra7aFlNdWWSUW-q-dfOf4maFzdlBN1z5nt0");
    assert_eq!(quota::ticket_id("kat-ticket"), "a5c716b74d187c0e");
}

#[test]
fn quota_cache_survives_re_captures_of_the_login() {
    let fx = Fixture::new();
    fx.sign_in("u-1", 1);
    let manager = fx.manager();
    manager.import_current().unwrap();
    let snapshot = WorkBuddyQuotaSnapshot {
        plan_name: Some("体验版".into()),
        is_paid: false,
        total: 10.0,
        remaining: 4.0,
        used: 6.0,
        queried_at: 1,
    };
    assert!(manager.save_quota("u-1", snapshot.clone()).unwrap());
    // WorkBuddy 刷新了 token：重新收回副本时额度缓存不能丢
    fx.sign_in("u-1", 2);
    manager.status().unwrap();
    let reloaded = fx.manager();
    assert_eq!(
        reloaded.quota_context("u-1").unwrap().cached,
        Some(snapshot)
    );
}

#[cfg(unix)]
mod wbipc {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;

    pub struct FakeBroker {
        /// 收到的帧（按顺序）
        pub frames: Arc<Mutex<Vec<Value>>>,
        pub task: tokio::task::JoinHandle<()>,
    }

    /// 起一个按 WorkBuddy 协议握手的假 broker，并写好发现文件。`server_ticket` 和发现文件里的
    /// ticket 不同时，它就证明不了自己。
    pub fn start(fx: &Fixture, ticket: &str, server_ticket: &str, body: Value) -> FakeBroker {
        let socket = fx.data_dir.join("b-test.sock");
        let endpoint = socket.to_string_lossy().to_string();
        std::fs::create_dir_all(fx.paths.wbipc_discovery.parent().unwrap()).unwrap();
        std::fs::write(
            &fx.paths.wbipc_discovery,
            serde_json::to_vec(&json!({"endpoint": endpoint, "ticket": ticket})).unwrap(),
        )
        .unwrap();
        let listener = UnixListener::bind(&socket).unwrap();
        let frames = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&frames);
        let server_ticket = server_ticket.to_string();
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read_half, mut writer) = tokio::io::split(stream);
            let mut lines = BufReader::new(read_half).lines();
            let mut client_nonce = String::new();
            let server_nonce = "server-nonce".to_string();
            while let Ok(Some(line)) = lines.next_line().await {
                let frame: Value = serde_json::from_str(&line).unwrap();
                seen.lock().unwrap().push(frame.clone());
                let reply = match frame.get("type").and_then(Value::as_str) {
                    Some("session_hello") => {
                        client_nonce = frame["client_nonce"].as_str().unwrap().to_string();
                        let proof = URL_SAFE.encode(
                            quota::proof_mac(
                                &server_ticket,
                                true,
                                &endpoint,
                                &client_nonce,
                                &server_nonce,
                            )
                            .finalize()
                            .into_bytes(),
                        );
                        json!({
                            "type": "session_challenge",
                            "protocol": 1,
                            "server_nonce": server_nonce,
                            "server_proof": proof,
                        })
                    }
                    Some("session_prove") => {
                        let expected = URL_SAFE.encode(
                            quota::proof_mac(
                                &server_ticket,
                                false,
                                &endpoint,
                                &client_nonce,
                                &server_nonce,
                            )
                            .finalize()
                            .into_bytes(),
                        );
                        assert_eq!(frame["client_proof"], json!(expected));
                        json!({"type": "session_hello_ack", "pipes": ["wb.request"]})
                    }
                    _ => match frame["method"].as_str() {
                        Some("broker/GetPipe") => json!({
                            "jsonrpc": "2.0",
                            "id": frame["id"],
                            "result": {"channel": "c:wb.request", "methods": ["http.fetch"]},
                        }),
                        Some("c:wb.request/http.fetch") => json!({
                            "jsonrpc": "2.0",
                            "id": frame["id"],
                            "result": {
                                "status": 200,
                                "headers": {"content-type": "application/json"},
                                "body_b64": STANDARD.encode(serde_json::to_vec(&body).unwrap()),
                            },
                        }),
                        _ => continue,
                    },
                };
                let mut out = serde_json::to_vec(&reply).unwrap();
                out.push(b'\n');
                writer.write_all(&out).await.unwrap();
            }
        });
        FakeBroker { frames, task }
    }

    const URL_SAFE: base64::engine::GeneralPurpose =
        base64::engine::general_purpose::URL_SAFE_NO_PAD;

    use hmac::Mac as _;

    fn summary() -> Value {
        json!({
            "code": 0,
            "data": {
                "Packages": [{"CycleTotalCapacity": "100", "CycleRemainCapacity": "40", "CycleUsedCapacity": "60"}],
                "SubscriptionPackageName": "体验版",
                "IsPaidUser": false,
            }
        })
    }

    #[tokio::test]
    async fn queries_the_current_account_through_wbipc_and_caches_it() {
        let fx = Fixture::new();
        fx.sign_in("u-1", 1);
        fx.running.store(true, Ordering::SeqCst);
        let manager = Arc::new(fx.manager());
        manager.import_current().unwrap();
        let broker = start(&fx, "the-ticket", "the-ticket", summary());

        let quota = account_quota(Arc::clone(&manager), "u-1".into(), false)
            .await
            .unwrap();
        assert!(quota.current);
        assert_eq!(quota.unavailable, None);
        let snapshot = quota.snapshot.expect("fresh snapshot");
        assert_eq!(snapshot.plan_name.as_deref(), Some("体验版"));
        assert_eq!(
            (snapshot.total, snapshot.remaining, snapshot.used),
            (100.0, 40.0, 60.0)
        );
        broker.task.await.unwrap();

        let frames = broker.frames.lock().unwrap().clone();
        // ticket 本身从不上线，只发 sha256 前缀
        assert!(frames
            .iter()
            .all(|frame| !frame.to_string().contains("the-ticket")));
        assert_eq!(
            frames[0]["ticket_id"],
            json!(quota::ticket_id("the-ticket"))
        );
        // 只有一次后端请求：空 body 的额度汇总查询
        let fetches: Vec<&Value> = frames
            .iter()
            .filter(|frame| frame["method"] == json!("c:wb.request/http.fetch"))
            .collect();
        assert_eq!(fetches.len(), 1);
        assert_eq!(fetches[0]["params"]["method"], json!("POST"));
        assert_eq!(fetches[0]["params"]["path"], json!(quota::SUMMARY_PATH));
        assert_eq!(
            fetches[0]["params"]["body_b64"],
            json!(STANDARD.encode("{}"))
        );

        // WorkBuddy 退出后：缓存还在，强制刷新说明是因为没在运行
        fx.running.store(false, Ordering::SeqCst);
        let reloaded = Arc::new(fx.manager());
        let cached = account_quota(Arc::clone(&reloaded), "u-1".into(), true)
            .await
            .unwrap();
        assert_eq!(cached.unavailable, Some(QuotaUnavailable::NotRunning));
        assert_eq!(cached.snapshot, Some(snapshot.clone()));
        // 不强制时 5 分钟内直接用缓存
        let within_ttl = account_quota(reloaded, "u-1".into(), false).await.unwrap();
        assert_eq!(within_ttl.unavailable, None);
        assert_eq!(within_ttl.snapshot, Some(snapshot));
    }

    #[tokio::test]
    async fn refuses_an_endpoint_that_cannot_prove_the_ticket() {
        let fx = Fixture::new();
        fx.sign_in("u-1", 1);
        fx.running.store(true, Ordering::SeqCst);
        let manager = Arc::new(fx.manager());
        manager.import_current().unwrap();
        let broker = start(&fx, "the-ticket", "someone-else", summary());

        let quota = account_quota(manager, "u-1".into(), true).await.unwrap();
        assert_eq!(quota.unavailable, Some(QuotaUnavailable::Failed));
        assert!(quota.snapshot.is_none());
        broker.task.await.unwrap();
        // 对面证明不了自己：只发了 hello，没有 prove，更没有请求
        let frames = broker.frames.lock().unwrap().clone();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0]["type"], json!("session_hello"));
    }
}

#[tokio::test]
async fn other_accounts_only_get_their_cached_quota() {
    let fx = Fixture::new();
    fx.running.store(true, Ordering::SeqCst);
    fx.sign_in("u-1", 1);
    let manager = Arc::new(fx.manager());
    manager.import_current().unwrap();
    fx.sign_in("u-2", 1);
    manager.import_current().unwrap();
    // 没有发现文件：要是去连了也只会是 not_running，这里应当根本不查
    let quota = account_quota(Arc::clone(&manager), "u-1".into(), true)
        .await
        .unwrap();
    assert!(!quota.current);
    assert_eq!(quota.unavailable, Some(QuotaUnavailable::NotCurrent));
    assert!(quota.snapshot.is_none());
    assert!(account_quota(manager, "missing".into(), false)
        .await
        .is_err());
}

#[tokio::test]
async fn current_account_without_running_workbuddy_reports_not_running() {
    let fx = Fixture::new();
    fx.sign_in("u-1", 1);
    let manager = Arc::new(fx.manager());
    manager.import_current().unwrap();
    let quota = account_quota(manager, "u-1".into(), false).await.unwrap();
    assert!(quota.current);
    assert_eq!(quota.unavailable, Some(QuotaUnavailable::NotRunning));
    assert!(quota.snapshot.is_none());
}

// ---------- 重新登录 ----------

#[test]
fn reauth_back_into_the_same_account_replaces_its_expired_snapshot() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    fixture.write_profile("u-1", "Alice", "pro");
    let expired = json!({
        "account": {"uid": "u-1", "type": "personal"},
        "auth": {"refreshExpiresAt": 1_000_000_000_000_i64},
    });
    std::fs::write(fixture.paths.info(), serde_json::to_vec(&expired).unwrap()).unwrap();
    let before = manager.import_current().unwrap().account;
    assert!(before.expired);
    assert!(manager.prepare_reauth("missing").is_err());

    manager.prepare_reauth("u-1").unwrap();
    // 走「登录另一个账号」：只在本机写登出标记，`.info` 原样留着
    assert!(fixture.paths.logout_marker().exists());
    assert_eq!(manager.live_state().reauth, None);

    // 用户在 WorkBuddy 里重新登录了同一个号；画像暂时读不到时也保留原来的昵称
    std::fs::remove_file(&fixture.paths.profile_snapshot).unwrap();
    fixture.sign_in("u-1", 3);
    let live = manager.live_state();
    let id = live
        .reauth
        .as_ref()
        .map(|outcome| outcome.id)
        .unwrap_or_default();
    assert_eq!(
        live.reauth,
        Some(WorkBuddyReauthOutcome {
            id,
            target_id: "u-1".into(),
            target_login: "Alice".into(),
            account_id: "u-1".into(),
            login: "Alice".into(),
            same_account: true,
            created: false,
        })
    );
    assert!(live.imported);
    // 结果留一会儿：别处先读到了也不会把它吞掉，界面按 id 只提示一次
    assert_eq!(
        manager.live_state().reauth.map(|outcome| outcome.id),
        Some(id)
    );

    let status = manager.status().unwrap();
    assert_eq!(status.accounts.len(), 1);
    let after = &status.accounts[0];
    assert_eq!(after.id, "u-1");
    assert_eq!(after.nickname.as_deref(), Some("Alice"));
    assert_eq!(after.added_at, before.added_at);
    assert!(!after.expired);
    let store = manager.lock();
    let saved = &store.accounts["u-1"];
    assert_eq!(saved.bundle.info, STANDARD.encode(info_doc("u-1", 3)));
    assert!(store.dismissed.is_empty());
}

#[test]
fn reauth_into_another_account_adds_it_and_keeps_the_target() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    fixture.sign_in("u-1", 0);
    manager.import_current().unwrap();

    manager.prepare_reauth("u-1").unwrap();
    fixture.sign_in("u-3", 0);
    // 自动收录（状态刷新）先收下了新登录，结果仍然报给界面
    let status = manager.status().unwrap();
    assert_eq!(status.current_account_id.as_deref(), Some("u-3"));
    assert_eq!(status.accounts.len(), 2);
    let outcome = manager.live_state().reauth.unwrap();
    assert!(!outcome.same_account);
    assert!(outcome.created);
    assert_eq!(outcome.target_id, "u-1");
    assert_eq!(outcome.account_id, "u-3");
    // 原账号的快照不动
    let store = manager.lock();
    assert_eq!(
        store.accounts["u-1"].bundle.info,
        STANDARD.encode(info_doc("u-1", 0))
    );
}

#[test]
fn reauth_while_workbuddy_runs_waits_for_a_login_inside_workbuddy() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    fixture.sign_in("u-1", 0);
    manager.import_current().unwrap();
    fixture.sign_in("u-2", 0);
    manager.import_current().unwrap();

    fixture.running.store(true, Ordering::SeqCst);
    assert!(manager.prepare_reauth("u-1").is_err());
    assert!(
        !fixture.paths.logout_marker().exists(),
        "运行中不写登出标记"
    );
    // 当前那个号自己刷新 token 不算新登录
    fixture.sign_in("u-2", 1);
    assert_eq!(manager.live_state().reauth, None);

    // 用户在 WorkBuddy 里退出、重新登录目标账号
    fixture.sign_in("u-1", 4);
    let outcome = manager.live_state().reauth.unwrap();
    assert!(outcome.same_account);
    assert_eq!(
        manager.lock().accounts["u-1"].bundle.info,
        STANDARD.encode(info_doc("u-1", 4))
    );
}

#[test]
fn plain_new_login_and_removal_cancel_a_pending_reauth() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    fixture.sign_in("u-1", 0);
    manager.import_current().unwrap();

    manager.prepare_reauth("u-1").unwrap();
    manager.prepare_new_login().unwrap();
    fixture.sign_in("u-3", 0);
    assert_eq!(manager.live_state().reauth, None);

    manager.prepare_reauth("u-1").unwrap();
    manager.remove("u-1").unwrap();
    fixture.sign_in("u-4", 0);
    assert_eq!(manager.live_state().reauth, None);
}
