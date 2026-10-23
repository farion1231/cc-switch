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

    manager.import_current().unwrap();
    assert_eq!(manager.remove_all().unwrap(), vec!["u-1".to_string()]);
    assert!(manager.status().unwrap().accounts.is_empty());
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
