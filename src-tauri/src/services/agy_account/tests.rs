use std::sync::Mutex as StdMutex;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::json;

use super::*;

#[derive(Default)]
struct MemoryLogin {
    value: StdMutex<Option<String>>,
    writes: StdMutex<usize>,
}

impl AgyLoginStore for MemoryLogin {
    fn read(&self) -> Result<Option<String>, AppError> {
        Ok(self.value.lock().unwrap().clone())
    }
    fn write(&self, value: &str) -> Result<(), AppError> {
        *self.value.lock().unwrap() = Some(value.to_string());
        *self.writes.lock().unwrap() += 1;
        Ok(())
    }
}

fn id_token(sub: &str, email: &str) -> String {
    format!(
        "e30.{}.sig",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({"sub": sub, "email": email})).unwrap())
    )
}

fn credential(sub: &str, access: &str) -> Value {
    json!({
        "token": {"access_token": access, "token_type": "Bearer", "refresh_token": format!("refresh-{sub}"),
                  "expiry": "2026-10-21T13:41:43.657824+08:00", "expires_in": 3599},
        "auth_method": "consumer",
        "id_token": id_token(sub, &format!("{sub}@example.com")),
    })
}

fn set_live(login: &MemoryLogin, credential: &Value) {
    *login.value.lock().unwrap() = Some(keychain::encode(credential).unwrap());
}

fn live_sub(login: &MemoryLogin) -> Option<String> {
    let raw = login.value.lock().unwrap().clone()?;
    let credential = keychain::decode(&raw).ok()?;
    oauth::identity(credential["id_token"].as_str()?)?.sub
}

fn manager() -> (tempfile::TempDir, Arc<MemoryLogin>, AgyAccountManager) {
    let dir = tempfile::tempdir().unwrap();
    let login = Arc::new(MemoryLogin::default());
    let manager = AgyAccountManager::with_login_store(dir.path().to_path_buf(), login.clone());
    (dir, login, manager)
}

async fn add(manager: &AgyAccountManager, sub: &str) {
    let mut store = manager.store.lock().await;
    let credential = credential(sub, "fresh");
    let claims = oauth::identity(credential["id_token"].as_str().unwrap()).unwrap();
    upsert(&mut store, credential, &claims);
    manager.persist(&store).unwrap();
}

// 以下测试只在能切号的平台（macOS）上有意义：其他平台 status 不读钥匙串、切换直接报不支持。
#[cfg(target_os = "macos")]
mod switching {
    use super::*;

    #[tokio::test]
    async fn status_collects_the_login_agy_already_has() {
        let (_dir, login, manager) = manager();
        set_live(&login, &credential("alice", "a1"));
        let status = manager.status().await.unwrap();
        assert_eq!(status.current_account_id.as_deref(), Some("alice"));
        assert_eq!(status.accounts.len(), 1);
        assert_eq!(
            status.accounts[0].email.as_deref(),
            Some("alice@example.com")
        );
        assert_eq!(*login.writes.lock().unwrap(), 0, "查看状态不写钥匙串");
    }

    #[tokio::test]
    async fn switching_writes_the_target_and_keeps_the_displaced_login() {
        let (dir, login, manager) = manager();
        add(&manager, "bob").await;
        // agy 登着一个 CC Switch 还没见过的号
        set_live(&login, &credential("alice", "a1"));

        manager.switch_to("bob").await.unwrap();
        assert_eq!(live_sub(&login).as_deref(), Some("bob"));

        // 被顶掉的 alice 收录了，能切回去
        let reopened = AgyAccountManager::with_login_store(dir.path().to_path_buf(), login.clone());
        let ids: Vec<_> = reopened.list().await.into_iter().map(|a| a.id).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"alice".to_string()));
        reopened.switch_to("alice").await.unwrap();
        assert_eq!(live_sub(&login).as_deref(), Some("alice"));
    }

    #[tokio::test]
    async fn switching_away_adopts_tokens_agy_refreshed() {
        let (_dir, login, manager) = manager();
        add(&manager, "alice").await;
        add(&manager, "bob").await;
        manager.switch_to("alice").await.unwrap();
        // agy 续期后把新的 access token 写回钥匙串
        set_live(&login, &credential("alice", "renewed"));
        manager.switch_to("bob").await.unwrap();
        let store = manager.store.lock().await;
        assert_eq!(
            store.accounts["alice"].credential["token"]["access_token"],
            "renewed"
        );
    }

    #[tokio::test]
    async fn switching_to_the_current_login_writes_nothing() {
        let (_dir, login, manager) = manager();
        set_live(&login, &credential("alice", "a1"));
        manager.status().await.unwrap();
        manager.switch_to("alice").await.unwrap();
        assert_eq!(*login.writes.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn removing_the_live_account_keeps_it_out_until_switched_away() {
        let (_dir, login, manager) = manager();
        add(&manager, "bob").await;
        set_live(&login, &credential("alice", "a1"));
        manager.status().await.unwrap();
        manager.remove("alice").await.unwrap();
        // 删掉后查看状态不会又收回来，钥匙串也没动
        let status = manager.status().await.unwrap();
        assert_eq!(status.current_account_id, None);
        assert_eq!(status.accounts.len(), 1);
        assert_eq!(live_sub(&login).as_deref(), Some("alice"));
        // 切走时照样收录，不丢登录
        manager.switch_to("bob").await.unwrap();
        assert_eq!(manager.list().await.len(), 2);
    }

    #[tokio::test]
    async fn unrecognized_login_is_stashed_before_overwrite() {
        let (dir, login, manager) = manager();
        add(&manager, "bob").await;
        *login.value.lock().unwrap() = Some("something-else".into());
        manager.switch_to("bob").await.unwrap();
        let stash: Value =
            serde_json::from_slice(&std::fs::read(dir.path().join(STASH_FILENAME)).unwrap())
                .unwrap();
        assert_eq!(stash["raw"], "something-else");
        assert_eq!(live_sub(&login).as_deref(), Some("bob"));
    }

    #[tokio::test]
    async fn accounts_needing_reauth_cannot_be_switched_to() {
        let (_dir, _login, manager) = manager();
        add(&manager, "bob").await;
        manager
            .store
            .lock()
            .await
            .accounts
            .get_mut("bob")
            .unwrap()
            .requires_reauth = true;
        assert!(manager.switch_to("bob").await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn account_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, _login, manager) = manager();
        add(&manager, "bob").await;
        let mode = std::fs::metadata(dir.path().join(STORE_FILENAME))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[tokio::test]
    async fn quota_uses_the_token_agy_refreshed_for_the_live_account() {
        let (_dir, login, manager) = manager();
        add(&manager, "alice").await;
        let mut live = credential("alice", "renewed-by-agy");
        live["token"]["expiry"] = json!(in_an_hour());
        set_live(&login, &live);
        assert_eq!(
            manager.access_token("alice").await.unwrap(),
            "renewed-by-agy"
        );
        assert_eq!(*login.writes.lock().unwrap(), 0, "查额度不写钥匙串");
    }

    #[tokio::test]
    async fn following_cards_query_the_account_agy_is_signed_in_with() {
        let (_dir, login, manager) = manager();
        assert_eq!(
            manager.live_account_id().await,
            None,
            "agy 没登录时改问 agy"
        );

        add(&manager, "alice").await;
        set_live(&login, &credential("bob", "b1"));
        // 还没收录的号也收进来，之后直接查
        assert_eq!(manager.live_account_id().await.as_deref(), Some("bob"));
        assert_eq!(manager.list().await.len(), 2);

        // 在 CC Switch 里删掉的号不再收回来，改问 agy
        manager.remove("bob").await.unwrap();
        assert_eq!(manager.live_account_id().await, None);
        assert_eq!(*login.writes.lock().unwrap(), 0);
    }

    /// 真机钥匙串冒烟：读写一条测试专用的条目（不是 agy 那条），确认 `security -i` 写入、
    /// 原地更新、读回都正常。默认不跑：`cargo test agy_account -- --ignored`。
    #[test]
    #[ignore]
    fn real_keychain_round_trip_on_a_test_item() {
        let store = keychain::SystemKeychain::named("cc-switch-agy-selftest", "probe");
        let first = keychain::encode(&credential("probe", "one")).unwrap();
        store.write(&first).unwrap();
        assert_eq!(store.read().unwrap().as_deref(), Some(first.as_str()));
        let second = keychain::encode(&credential("probe", "two")).unwrap();
        store.write(&second).unwrap();
        assert_eq!(store.read().unwrap().as_deref(), Some(second.as_str()));
        let _ = std::process::Command::new("/usr/bin/security")
            .args([
                "delete-generic-password",
                "-s",
                "cc-switch-agy-selftest",
                "-a",
                "probe",
            ])
            .output();
    }
}

#[tokio::test]
async fn removing_from_cc_switch_never_touches_the_keychain() {
    let (_dir, login, manager) = manager();
    add(&manager, "bob").await;
    set_live(&login, &credential("bob", "b1"));
    manager.remove("bob").await.unwrap();
    assert!(manager.list().await.is_empty());
    assert_eq!(*login.writes.lock().unwrap(), 0);
    assert_eq!(live_sub(&login).as_deref(), Some("bob"));
}

#[tokio::test]
async fn unknown_login_flow_reports_an_error() {
    let (_dir, _login, manager) = manager();
    assert!(manager.poll_login("nope").await.is_err());
    assert!(!manager.cancel_login("nope").await);
}

fn in_an_hour() -> String {
    (chrono::Local::now() + chrono::Duration::hours(1))
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, false)
}

#[tokio::test]
async fn quota_token_comes_from_the_stored_copy_while_fresh() {
    let (_dir, _login, manager) = manager();
    add(&manager, "bob").await;
    manager
        .store
        .lock()
        .await
        .accounts
        .get_mut("bob")
        .unwrap()
        .credential["token"]["expiry"] = json!(in_an_hour());
    assert_eq!(manager.access_token("bob").await.unwrap(), "fresh");
}

#[tokio::test]
async fn quota_token_is_refused_for_unknown_or_expired_accounts() {
    let (_dir, _login, manager) = manager();
    assert!(matches!(
        manager.access_token("nobody").await,
        Err(AccountQuotaError::NotFound)
    ));
    add(&manager, "bob").await;
    manager
        .store
        .lock()
        .await
        .accounts
        .get_mut("bob")
        .unwrap()
        .requires_reauth = true;
    assert!(matches!(
        manager.access_token("bob").await,
        Err(AccountQuotaError::NeedsReauth)
    ));
}
