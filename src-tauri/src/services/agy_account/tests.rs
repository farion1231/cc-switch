use std::sync::Mutex as StdMutex;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::json;

use super::*;

#[derive(Default)]
struct MemoryLogin {
    value: StdMutex<Option<String>>,
    writes: StdMutex<usize>,
    locked: StdMutex<bool>,
    post_write_read_failures: StdMutex<usize>,
}

impl AgyLoginStore for MemoryLogin {
    fn read(&self) -> Result<Option<String>, AppError> {
        if *self.locked.lock().unwrap() {
            return Err(AppError::Config("test keychain is locked".into()));
        }
        let mut failures = self.post_write_read_failures.lock().unwrap();
        if *failures > 0 && *self.writes.lock().unwrap() > 0 {
            *failures -= 1;
            return Err(AppError::Config("test keychain readback failed".into()));
        }
        Ok(self.value.lock().unwrap().clone())
    }
    fn read_for_switch(&self) -> Result<Option<String>, AppError> {
        *self.locked.lock().unwrap() = false;
        self.read()
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
    identity::identity(credential["id_token"].as_str()?)?.sub
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
    let claims = identity::identity(credential["id_token"].as_str().unwrap()).unwrap();
    upsert(&mut store, credential, &claims);
    manager.persist(&store).unwrap();
}

// 以下测试只在支持切号的平台上有意义。
#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
mod switching {
    use super::*;

    use crate::app_config::AppType;
    use crate::database::Database;
    use crate::provider::{AuthBinding, AuthBindingSource, Provider, ProviderMeta};
    use crate::services::provider::agy_direct::save_from_editor;
    use crate::store::AppState;

    fn official_card(account: &str) -> Provider {
        let mut provider =
            Provider::with_id("official".into(), "Antigravity".into(), json!({}), None);
        provider.category = Some("official".into());
        provider.meta = Some(ProviderMeta {
            auth_binding: Some(AuthBinding {
                source: AuthBindingSource::ManagedAccount,
                auth_provider: Some("agy_oauth".into()),
                account_id: Some(account.into()),
            }),
            ..Default::default()
        });
        provider
    }

    #[test]
    fn saving_a_new_provider_binding_changes_the_live_login() {
        let (_dir, login, manager) = manager();
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        set_live(&login, &credential("alice", "a1"));
        tauri::async_runtime::block_on(add(&manager, "bob"));
        state
            .db
            .save_provider(&AppType::Agy, &official_card("alice"))
            .unwrap();

        let mut card = official_card("bob");
        save_from_editor(&state, &manager, card.clone()).unwrap();
        assert_eq!(live_sub(&login).as_deref(), Some("bob"));
        let status = tauri::async_runtime::block_on(manager.status()).unwrap();
        assert_eq!(status.current_account_id.as_deref(), Some("bob"));

        tauri::async_runtime::block_on(manager.switch_to("alice")).unwrap();
        card.name = "Renamed card".into();
        save_from_editor(&state, &manager, card.clone()).unwrap();
        assert_eq!(live_sub(&login).as_deref(), Some("alice"));

        card.meta = None;
        save_from_editor(&state, &manager, card).unwrap();
        assert_eq!(live_sub(&login).as_deref(), Some("alice"));
    }

    #[test]
    fn a_missing_bound_account_leaves_the_login_and_provider_unchanged() {
        let (_dir, login, manager) = manager();
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        set_live(&login, &credential("alice", "a1"));
        let previous = official_card("alice");
        state.db.save_provider(&AppType::Agy, &previous).unwrap();

        let card = official_card("missing");
        assert!(save_from_editor(&state, &manager, card.clone()).is_err());
        let saved = state
            .db
            .get_provider_by_id(&card.id, &AppType::Agy)
            .unwrap()
            .unwrap();
        assert_eq!(
            saved
                .meta
                .unwrap()
                .managed_account_id_for("agy_oauth")
                .as_deref(),
            Some("alice")
        );
        assert_eq!(live_sub(&login).as_deref(), Some("alice"));

        let mut new_card = card;
        new_card.id = "new".into();
        assert!(save_from_editor(&state, &manager, new_card).is_err());
        assert!(state
            .db
            .get_provider_by_id("new", &AppType::Agy)
            .unwrap()
            .is_none());
    }

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

    #[test]
    fn cloud_snapshot_waits_for_a_failed_binding_save_to_roll_back() {
        use crate::services::sync_protocol::sync_mutex;

        struct CancelUnlock;

        impl AgyLoginStore for CancelUnlock {
            fn read(&self) -> Result<Option<String>, AppError> {
                assert!(sync_mutex().try_lock().is_err());
                Err(AppError::Message("keychain unlock cancelled".into()))
            }

            fn write(&self, _: &str) -> Result<(), AppError> {
                unreachable!("a cancelled unlock must not write")
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let manager =
            AgyAccountManager::with_login_store(dir.path().to_path_buf(), Arc::new(CancelUnlock));
        tauri::async_runtime::block_on(add(&manager, "bob"));
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        state
            .db
            .save_provider(&AppType::Agy, &official_card("alice"))
            .unwrap();
        let before = state.db.export_sql_string_for_sync().unwrap();

        let error = save_from_editor(&state, &manager, official_card("bob")).unwrap_err();
        assert_eq!(error.to_string(), "keychain unlock cancelled");

        let _sync_guard = sync_mutex().blocking_lock();
        assert_eq!(state.db.export_sql_string_for_sync().unwrap(), before);
    }

    #[tokio::test]
    async fn status_leaves_a_locked_keychain_alone_but_switch_can_unlock_it() {
        let (_dir, login, manager) = manager();
        set_live(&login, &credential("alice", "a1"));
        add(&manager, "bob").await;
        *login.locked.lock().unwrap() = true;

        let status = manager.status().await.unwrap();
        assert!(status.current_account_id.is_none());
        assert_eq!(status.accounts.len(), 1);
        assert!(*login.locked.lock().unwrap());

        manager.switch_to("bob").await.unwrap();
        assert_eq!(live_sub(&login).as_deref(), Some("bob"));
        assert!(manager
            .list()
            .await
            .iter()
            .any(|account| account.id == "alice"));
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

    #[test]
    fn saving_a_binding_recovers_from_a_transient_readback_failure() {
        let (_dir, login, manager) = manager();
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        set_live(&login, &credential("alice", "a1"));
        tauri::async_runtime::block_on(add(&manager, "bob"));
        *login.post_write_read_failures.lock().unwrap() = 1;

        let card = official_card("bob");
        save_from_editor(&state, &manager, card.clone()).unwrap();

        let saved = state
            .db
            .get_provider_by_id(&card.id, &AppType::Agy)
            .unwrap()
            .unwrap();
        assert_eq!(
            saved
                .meta
                .unwrap()
                .managed_account_id_for("agy_oauth")
                .as_deref(),
            Some("bob")
        );
        assert_eq!(live_sub(&login).as_deref(), Some("bob"));
        assert_eq!(*login.writes.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn persistent_readback_failure_reports_that_the_login_is_unverified() {
        let (_dir, login, manager) = manager();
        set_live(&login, &credential("alice", "a1"));
        add(&manager, "bob").await;
        *login.post_write_read_failures.lock().unwrap() = 2;

        let error = manager.switch_to("bob").await.unwrap_err();
        assert!(matches!(
            error,
            AppError::Localized { key, .. } if key == "agy.account.write_verify_failed"
        ));
        assert_eq!(live_sub(&login).as_deref(), Some("bob"));
        assert_eq!(*login.writes.lock().unwrap(), 1);
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
    async fn following_cards_query_the_account_agy_is_signed_in_with() {
        let (_dir, login, manager) = manager();
        assert_eq!(manager.current_account_id().await, None, "agy 没登录");

        add(&manager, "alice").await;
        set_live(&login, &credential("bob", "b1"));
        // 还没收录的号也收进来
        assert_eq!(manager.current_account_id().await.as_deref(), Some("bob"));
        assert_eq!(manager.list().await.len(), 2);

        // 在 CC Switch 里删掉的号不再收回来
        manager.remove("bob").await.unwrap();
        assert_eq!(manager.current_account_id().await, None);
        assert_eq!(*login.writes.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn adding_an_account_waits_for_a_new_login_in_agy() {
        let (_dir, login, manager) = manager();
        set_live(&login, &credential("alice", "a1"));
        let start = manager.start_login().await.unwrap();
        // agy 原来登着的号先收进来，用户在 agy 里 /logout 也不会丢
        assert_eq!(manager.list().await.len(), 1);
        // 钥匙串没变、或已登出：继续等
        assert!(manager.poll_login(&start.flow_id).await.unwrap().is_none());
        *login.value.lock().unwrap() = None;
        assert!(manager.poll_login(&start.flow_id).await.unwrap().is_none());

        // 在 agy 里登录了新号：收录，这次登录结束
        set_live(&login, &agy15_credential("bob", "b1"));
        let account = manager.poll_login(&start.flow_id).await.unwrap().unwrap();
        assert_eq!(account.id, "email:bob@example.com");
        assert_eq!(account.email.as_deref(), Some("bob@example.com"));
        assert_eq!(manager.list().await.len(), 2);
        assert!(manager.poll_login(&start.flow_id).await.is_err());
        assert_eq!(*login.writes.lock().unwrap(), 0, "加号不写钥匙串");
    }

    #[tokio::test]
    async fn signing_in_again_with_the_same_account_replaces_its_copy() {
        let (_dir, login, manager) = manager();
        add(&manager, "alice").await;
        set_live(&login, &credential("alice", "a1"));
        let start = manager.start_login().await.unwrap();
        let mut again = agy15_credential("alice", "a2");
        again["token"]["refresh_token"] = json!("new-refresh");
        set_live(&login, &again);
        let account = manager.poll_login(&start.flow_id).await.unwrap().unwrap();
        assert_eq!(account.id, "alice", "按邮箱对回原来的号");
        let store = manager.store.lock().await;
        assert_eq!(store.accounts.len(), 1);
        assert_eq!(
            store.accounts["alice"].credential["token"]["refresh_token"],
            "new-refresh"
        );
    }

    #[tokio::test]
    async fn quota_is_remembered_per_account() {
        let dir = tempfile::tempdir().unwrap();
        let login = Arc::new(MemoryLogin::default());
        let manager = AgyAccountManager::with_login_store(dir.path().to_path_buf(), login.clone());
        add(&manager, "alice").await;
        assert!(manager.last_quota("alice").await.is_none());
        let quota = SubscriptionQuota::not_found("agy");
        manager.record_quota("alice", &quota).await;
        manager.record_quota("nobody", &quota).await;
        // 存在账号文件里，重启后还在
        let reopened = AgyAccountManager::with_login_store(dir.path().to_path_buf(), login);
        assert_eq!(
            reopened
                .last_quota("alice")
                .await
                .map(|q| q.tool)
                .as_deref(),
            Some("agy")
        );
        assert_eq!(reopened.list().await.len(), 1);
    }

    /// agy 1.5 自己写回钥匙串的格式：没有 id_token，只有 id_token_email 和 writer_id。
    pub(super) fn agy15_credential(sub: &str, access: &str) -> Value {
        let mut credential = credential(sub, access);
        let object = credential.as_object_mut().unwrap();
        object.remove("id_token");
        object.insert("id_token_email".into(), json!(format!("{sub}@example.com")));
        object.insert("writer_id".into(), json!("w1"));
        credential
    }

    #[tokio::test]
    async fn agy15_login_is_recognized_by_its_refresh_token() {
        let (_dir, login, manager) = manager();
        add(&manager, "alice").await;
        add(&manager, "bob").await;
        let mut live = agy15_credential("alice", "renewed-by-agy15");
        live["token"]["expiry"] = json!(in_an_hour());
        set_live(&login, &live);

        let status = manager.status().await.unwrap();
        assert_eq!(status.current_account_id.as_deref(), Some("alice"));
        assert_eq!(status.accounts.len(), 2, "不会多出一个认不出的号");
        // agy 续过的令牌照样收回来
        let store = manager.store.lock().await;
        assert_eq!(
            store.accounts["alice"].credential["token"]["access_token"],
            "renewed-by-agy15"
        );
        drop(store);
        assert_eq!(*login.writes.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn agy15_login_falls_back_to_the_email() {
        let (_dir, login, manager) = manager();
        add(&manager, "alice").await;
        // 在 agy 里重新登录过：refresh token 换了，邮箱没变（大小写不同也认）
        let mut live = agy15_credential("alice", "a2");
        live["token"]["refresh_token"] = json!("another-refresh");
        live["id_token_email"] = json!("Alice@Example.com");
        set_live(&login, &live);
        assert_eq!(manager.current_account_id().await.as_deref(), Some("alice"));
        let store = manager.store.lock().await;
        assert_eq!(
            store.accounts["alice"].credential["token"]["refresh_token"],
            "another-refresh"
        );
    }

    #[tokio::test]
    async fn switching_away_from_an_agy15_login_does_not_stash_it() {
        let (dir, login, manager) = manager();
        add(&manager, "alice").await;
        add(&manager, "bob").await;
        set_live(&login, &agy15_credential("alice", "a2"));
        manager.switch_to("bob").await.unwrap();
        assert_eq!(live_sub(&login).as_deref(), Some("bob"));
        assert!(!dir.path().join(STASH_FILENAME).exists());
        // 切回 alice 时写的是 agy 写回的那份（新格式），核对照样通过
        manager.switch_to("alice").await.unwrap();
        let raw = login.value.lock().unwrap().clone().unwrap();
        assert_eq!(
            keychain::decode(&raw).unwrap()["id_token_email"],
            "alice@example.com"
        );
    }

    #[tokio::test]
    async fn removing_an_agy15_live_account_keeps_it_out_and_recovers_it_on_switch() {
        let (dir, login, manager) = manager();
        add(&manager, "alice").await;
        add(&manager, "bob").await;
        set_live(&login, &agy15_credential("alice", "a2"));

        manager.remove("alice").await.unwrap();
        assert_eq!(*login.writes.lock().unwrap(), 0, "删账号不动钥匙串");
        // agy 1.5 的钥匙串条目没有 id_token，删掉后依然靠 dismissed_emails 本地识别，
        // 不会发网请求，也不会重新塞回列表
        let status = manager.status().await.unwrap();
        assert_eq!(status.current_account_id, None);
        assert_eq!(status.accounts.len(), 1);
        assert_eq!(manager.current_account_id().await, None);

        // 切走时照样收录回 accounts，不会当成不认识的登录存进 stash
        manager.switch_to("bob").await.unwrap();
        assert!(!dir.path().join(STASH_FILENAME).exists());
        assert_eq!(manager.list().await.len(), 2);
    }

    #[tokio::test]
    async fn legacy_dismissed_account_backfills_dismissed_email_once_recognized() {
        let (_dir, login, manager) = manager();
        // 模拟旧版删过的账号：只有 dismissed 里的 sub，没有 dismissed_emails
        {
            let mut store = manager.store.lock().await;
            store.dismissed.push("alice".into());
            manager.persist(&store).unwrap();
        }
        // 第一次经带 id_token 的旧格式认出是谁时，顺手把邮箱补进 dismissed_emails
        set_live(&login, &credential("alice", "a1"));
        let status = manager.status().await.unwrap();
        assert_eq!(status.current_account_id, None);
        assert!(status.accounts.is_empty());
        {
            let store = manager.store.lock().await;
            assert_eq!(
                store.dismissed_emails.get("alice").map(String::as_str),
                Some("alice@example.com")
            );
        }
        // 之后 agy 1.5 改写成不带 id_token 的新格式，直接靠补齐的 dismissed_emails 本地识别
        set_live(&login, &agy15_credential("alice", "a2"));
        let status = manager.status().await.unwrap();
        assert_eq!(status.current_account_id, None);
        assert!(status.accounts.is_empty());
    }

    /// 真机钥匙串冒烟：读写一条测试专用的条目（不是 agy 那条），确认写入（macOS 经 `security -i`，
    /// Windows 经凭据管理器）、原地更新、读回都正常。默认不跑：`cargo test agy_account -- --ignored`。
    #[test]
    #[ignore]
    #[cfg(any(target_os = "macos", windows))]
    fn real_keychain_round_trip_on_a_test_item() {
        let store = keychain::SystemKeychain::named("cc-switch-agy-selftest", "probe");
        let first = keychain::encode(&credential("probe", "one")).unwrap();
        store.write(&first).unwrap();
        assert_eq!(store.read().unwrap().as_deref(), Some(first.as_str()));
        let second = keychain::encode(&credential("probe", "two")).unwrap();
        store.write(&second).unwrap();
        assert_eq!(store.read().unwrap().as_deref(), Some(second.as_str()));
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("/usr/bin/security")
            .args([
                "delete-generic-password",
                "-s",
                "cc-switch-agy-selftest",
                "-a",
                "probe",
            ])
            .output();
        #[cfg(windows)]
        let _ = std::process::Command::new("cmdkey")
            .arg("/delete:cc-switch-agy-selftest:probe")
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
async fn an_unreadable_account_file_is_kept_instead_of_overwritten() {
    let (dir, login, manager) = manager();
    add(&manager, "alice").await;
    add(&manager, "bob").await;
    let store_path = dir.path().join(STORE_FILENAME);
    let mut unreadable = std::fs::read(&store_path).unwrap();
    unreadable.extend_from_slice(b"\n}");
    std::fs::write(&store_path, &unreadable).unwrap();
    set_live(&login, &credential("carol", "c1"));

    let reopened = AgyAccountManager::with_login_store(dir.path().to_path_buf(), login.clone());
    reopened.status().await.unwrap();

    assert!(
        crate::test_support::dir_holds_file_with(dir.path(), &unreadable),
        "the unreadable account file was overwritten"
    );
}

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
mod running_guard_and_scope {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn test_env(
        cli: bool,
        desktop: bool,
    ) -> (
        tempfile::TempDir,
        tempfile::TempDir,
        Arc<MemoryLogin>,
        AgyAccountManager,
        Arc<AtomicBool>,
        Arc<AtomicBool>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let desktop_dir = tempfile::tempdir().unwrap();
        let login = Arc::new(MemoryLogin::default());
        let cli_flag = Arc::new(AtomicBool::new(cli));
        let desktop_flag = Arc::new(AtomicBool::new(desktop));
        let cli_clone = Arc::clone(&cli_flag);
        let desktop_clone = Arc::clone(&desktop_flag);
        let manager = AgyAccountManager::with_test_env(
            dir.path().to_path_buf(),
            login.clone(),
            Arc::new(move || AgyRunningState {
                cli: cli_clone.load(Ordering::Relaxed),
                desktop: desktop_clone.load(Ordering::Relaxed),
            }),
            Some(desktop_dir.path().to_path_buf()),
        );
        (dir, desktop_dir, login, manager, cli_flag, desktop_flag)
    }

    #[tokio::test]
    async fn switch_blocks_while_desktop_cli_or_both_are_running_and_preserves_scope() {
        let (_dir, desktop_dir, login, manager, cli_flag, desktop_flag) = test_env(false, true);
        set_live(&login, &credential("alice", "a1"));
        add(&manager, "bob").await;
        let scope_file = desktop_dir
            .path()
            .join(keychain::DESKTOP_CREDENTIAL_SCOPE_FILENAME);
        std::fs::write(&scope_file, "scope-alice").unwrap();

        // Desktop running -> blocks with running_desktop, keychain and scope untouched.
        let err = manager.switch_to("bob").await.unwrap_err();
        assert!(err.to_string().contains("Antigravity"), "{err}");
        assert_eq!(live_sub(&login).as_deref(), Some("alice"));
        assert!(scope_file.exists());

        // CLI running -> blocks with running_cli.
        desktop_flag.store(false, Ordering::Relaxed);
        cli_flag.store(true, Ordering::Relaxed);
        let err = manager.switch_to("bob").await.unwrap_err();
        assert!(err.to_string().contains("agy CLI"), "{err}");
        assert_eq!(live_sub(&login).as_deref(), Some("alice"));
        assert!(scope_file.exists());

        // Both running -> blocks with running_both.
        desktop_flag.store(true, Ordering::Relaxed);
        let err = manager.switch_to("bob").await.unwrap_err();
        assert!(err.to_string().contains("Antigravity"), "{err}");
        assert!(err.to_string().contains("agy CLI"), "{err}");
        assert_eq!(live_sub(&login).as_deref(), Some("alice"));
        assert!(scope_file.exists());

        // No-op switch to already-active account succeeds even while running.
        manager.switch_to("alice").await.unwrap();
        assert!(scope_file.exists());
    }

    #[tokio::test]
    async fn start_login_blocks_while_desktop_cli_or_both_are_running() {
        let (_dir, _desktop_dir, login, manager, cli_flag, desktop_flag) = test_env(false, true);
        set_live(&login, &credential("alice", "a1"));

        let err = manager.start_login().await.unwrap_err();
        assert!(matches!(
            err,
            AppError::Localized { key, .. } if key == "agy.account.login_running_desktop"
        ));

        desktop_flag.store(false, Ordering::Relaxed);
        cli_flag.store(true, Ordering::Relaxed);
        let err = manager.start_login().await.unwrap_err();
        assert!(matches!(
            err,
            AppError::Localized { key, .. } if key == "agy.account.login_running_cli"
        ));

        desktop_flag.store(true, Ordering::Relaxed);
        let err = manager.start_login().await.unwrap_err();
        assert!(matches!(
            err,
            AppError::Localized { key, .. } if key == "agy.account.login_running_both"
        ));

        desktop_flag.store(false, Ordering::Relaxed);
        cli_flag.store(false, Ordering::Relaxed);
        assert!(manager.start_login().await.is_ok());
    }

    #[tokio::test]
    async fn switch_clears_desktop_scope_and_syncs_fallback_only_after_verified_write() {
        let dir = tempfile::tempdir().unwrap();
        let desktop_dir = tempfile::tempdir().unwrap();
        let fallback_file = dir.path().join(keychain::STANDALONE_TOKEN_FILENAME);
        let raw_keychain = Arc::new(MemoryLogin::default());
        let login = Arc::new(keychain::AgyLogin::with_keychain(
            fallback_file.clone(),
            raw_keychain.clone(),
        ));
        let manager = AgyAccountManager::with_test_env(
            dir.path().to_path_buf(),
            login,
            Arc::new(AgyRunningState::default),
            Some(desktop_dir.path().to_path_buf()),
        );

        let alice_cred = credential("alice", "a1");
        let bob_cred = credential("bob", "fresh");
        set_live(&raw_keychain, &alice_cred);
        std::fs::write(&fallback_file, serde_json::to_vec(&alice_cred).unwrap()).unwrap();
        add(&manager, "bob").await;
        let scope_file = desktop_dir
            .path()
            .join(keychain::DESKTOP_CREDENTIAL_SCOPE_FILENAME);
        std::fs::write(&scope_file, "scope-alice").unwrap();

        // If post-write verification fails, scope file and fallback file stay untouched.
        *raw_keychain.post_write_read_failures.lock().unwrap() = 2;
        assert!(manager.switch_to("bob").await.is_err());
        assert!(scope_file.exists());
        assert_eq!(
            keychain::decode(&std::fs::read_to_string(&fallback_file).unwrap()).unwrap(),
            alice_cred
        );

        // Reset back to alice and perform a clean verified switch to bob -> scope cleared & fallback synced.
        set_live(&raw_keychain, &alice_cred);
        manager.switch_to("bob").await.unwrap();
        assert_eq!(live_sub(&raw_keychain).as_deref(), Some("bob"));
        assert!(!scope_file.exists());
        assert_eq!(
            keychain::decode(&std::fs::read_to_string(&fallback_file).unwrap()).unwrap(),
            bob_cred
        );
    }

    #[tokio::test]
    async fn poll_login_same_account_with_same_refresh_token_completes_after_seeing_logged_out() {
        let (_dir, desktop_dir, login, manager, _cli_flag, _desktop_flag) = test_env(false, false);
        let scope_file = desktop_dir
            .path()
            .join(keychain::DESKTOP_CREDENTIAL_SCOPE_FILENAME);
        std::fs::write(&scope_file, "scope-alice").unwrap();

        set_live(&login, &credential("alice", "a1"));
        let start = manager.start_login().await.unwrap();
        assert!(manager.has_pending_login().await);

        // Still signed in with the baseline refresh token -> keep polling.
        assert!(manager.poll_login(&start.flow_id).await.unwrap().is_none());

        // User runs /logout -> LiveLogin::None recorded as saw_logged_out.
        *login.value.lock().unwrap() = None;
        assert!(manager.poll_login(&start.flow_id).await.unwrap().is_none());

        // User signs back into the same Google account returning the same refresh_token -> completes!
        set_live(&login, &credential("alice", "a2"));
        let account = manager.poll_login(&start.flow_id).await.unwrap().unwrap();
        assert_eq!(account.id, "alice");
        assert!(!scope_file.exists());
        assert!(!manager.has_pending_login().await);
    }

    #[tokio::test]
    async fn identify_prefers_exact_sub_match_and_falls_back_to_id_token_email_without_duplicates()
    {
        let (_dir, _desktop_dir, login, manager, _cli, _desktop) = test_env(false, false);

        // Existing account stored by email:<address> (e.g. from agy 1.5 without id_token).
        set_live(&login, &switching::agy15_credential("alice", "a1"));
        let status = manager.status().await.unwrap();
        assert_eq!(
            status.current_account_id.as_deref(),
            Some("email:alice@example.com")
        );

        // Re-signing in with a credential that has id_token (sub + email) matches existing email account
        // via id_token.email fallback without creating a duplicate account.
        let mut with_id_token = credential("alice-sub", "a2");
        with_id_token["id_token"] = json!(id_token("alice-sub", "alice@example.com"));
        set_live(&login, &with_id_token);
        let status = manager.status().await.unwrap();
        assert_eq!(status.accounts.len(), 1);
        assert_eq!(
            status.current_account_id.as_deref(),
            Some("email:alice@example.com")
        );

        // Conversely, when an account is stored under its `sub` key and another account shares email
        // or no email match is needed, exact `sub` match takes priority over `email:` heuristic.
        let mut store = manager.store.lock().await;
        let sub_cred = credential("bob-sub", "b1");
        let bob_claims = identity::identity(sub_cred["id_token"].as_str().unwrap()).unwrap();
        upsert(&mut store, sub_cred, &bob_claims);
        // Simulate an email-keyed entry with the same email inserted earlier.
        let mut dup_email_cred = switching::agy15_credential("bob-sub", "b0");
        dup_email_cred["token"]["refresh_token"] = json!("refresh-bob-old");
        let dup_claims = identity::IdentityClaims {
            sub: Some(identity::email_account_id("bob-sub@example.com")),
            email: Some("bob-sub@example.com".into()),
            name: None,
            picture: None,
        };
        upsert(&mut store, dup_email_cred, &dup_claims);
        drop(store);

        let mut fresh_bob = credential("bob-sub", "b2");
        fresh_bob["token"]["refresh_token"] = json!("refresh-bob-old");
        set_live(&login, &fresh_bob);
        let status = manager.status().await.unwrap();
        assert_eq!(status.current_account_id.as_deref(), Some("bob-sub"));
    }

    #[test]
    fn process_classification_handles_windows_and_unix_processes() {
        let tasklist = r#""System Idle Process","0","Services","0","8 K"
"agy.exe","1024","Console","1","45,000 K"
"Antigravity.exe","2048","Console","1","220,000 K"
"language_server_windows_x64.exe","4096","Console","1","180,000 K"
"language_server.exe","8192","Console","1","160,000 K"
"#;
        let entries = parse_windows_tasklist_entries(tasklist);
        assert_eq!(
            entries,
            vec![
                ("System Idle Process".to_string(), 0),
                ("agy.exe".to_string(), 1024),
                ("Antigravity.exe".to_string(), 2048),
                ("language_server_windows_x64.exe".to_string(), 4096),
                ("language_server.exe".to_string(), 8192),
            ]
        );
        assert_eq!(
            classify_windows_image_name("agy.exe"),
            WindowsProcessKind::Cli
        );
        assert_eq!(
            classify_windows_image_name("antigravity.EXE"),
            WindowsProcessKind::Desktop
        );
        assert_eq!(
            classify_windows_image_name("language_server_windows_x64.exe"),
            WindowsProcessKind::LanguageServer
        );
        assert_eq!(
            classify_windows_image_name("language_server.exe"),
            WindowsProcessKind::LanguageServer
        );
        assert_eq!(
            classify_windows_image_name("node.exe"),
            WindowsProcessKind::Other
        );

        assert_eq!(
            classify_unix_process_lines(
                ["/Applications/Antigravity-IDE.app/Contents/MacOS/Antigravity"].into_iter()
            ),
            AgyRunningState {
                cli: false,
                desktop: false
            }
        );
        assert_eq!(
            classify_unix_process_lines(
                ["/Applications/Antigravity.app/Contents/MacOS/Electron"].into_iter()
            ),
            AgyRunningState {
                cli: false,
                desktop: true
            }
        );
        assert_eq!(
            classify_unix_process_lines(
                ["/Users/me/.windsurf/bin/language_server_macos_arm64"].into_iter()
            ),
            AgyRunningState {
                cli: false,
                desktop: false
            }
        );
        assert_eq!(
            classify_unix_process_lines(
                [
                    "/usr/local/bin/agy",
                    "/Users/me/.gemini/antigravity/bin/language_server_macos_arm64"
                ]
                .into_iter()
            ),
            AgyRunningState {
                cli: true,
                desktop: true
            }
        );

        // Linux ps -axo args= lines with CLI flags and IDE paths.
        assert_eq!(
            classify_unix_process_lines(
                [
                    "/usr/bin/agy --model gemini-3.1",
                    "/opt/antigravity/language_server_linux_x64 --port 1234"
                ]
                .into_iter()
            ),
            AgyRunningState {
                cli: true,
                desktop: true
            }
        );
        assert_eq!(
            classify_unix_process_lines(
                [
                    "/home/u/.gemini/antigravity-ide/bin/language_server_linux_x64 --port 1",
                    "/Applications/Antigravity IDE.app/Contents/MacOS/Antigravity"
                ]
                .into_iter()
            ),
            AgyRunningState {
                cli: false,
                desktop: false
            }
        );

        // Windows Desktop executable path checks (legacy language_server_windows*.exe & new language_server.exe).
        assert!(is_antigravity_desktop_language_server_path(
            r"C:\Users\me\AppData\Local\Programs\Antigravity\resources\app\bin\language_server_windows_x64.exe"
        ));
        assert!(is_antigravity_desktop_language_server_path(
            r"C:\Users\me\AppData\Local\Programs\Antigravity\resources\app\extensions\antigravity\bin\language_server.exe"
        ));
        assert!(!is_antigravity_desktop_language_server_path(
            r"C:\Users\me\AppData\Local\Programs\Windsurf\language_server.exe"
        ));
        assert!(!is_antigravity_desktop_language_server_path(
            r"C:\Users\me\AppData\Local\Programs\Windsurf\language_server_windows_x64.exe"
        ));
        assert!(!is_antigravity_desktop_language_server_path(
            r"C:\Users\me\.gemini\antigravity-ide\bin\language_server_windows_x64.exe"
        ));
        assert!(!is_antigravity_desktop_language_server_path(
            r"C:\Users\me\.gemini\antigravity-ide\bin\language_server.exe"
        ));
    }

    #[test]
    fn saving_new_provider_binding_rolls_back_db_when_antigravity_is_running() {
        use crate::app_config::AppType;
        use crate::database::Database;
        use crate::provider::{AuthBinding, AuthBindingSource, Provider, ProviderMeta};
        use crate::services::provider::agy_direct::save_from_editor;
        use crate::store::AppState;

        let (_dir, _desktop_dir, login, manager, _cli, _desktop) = test_env(false, true);
        set_live(&login, &credential("alice", "a1"));
        tauri::async_runtime::block_on(add(&manager, "bob"));

        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        let mut alice_card =
            Provider::with_id("official".into(), "Antigravity".into(), json!({}), None);
        alice_card.category = Some("official".into());
        alice_card.meta = Some(ProviderMeta {
            auth_binding: Some(AuthBinding {
                source: AuthBindingSource::ManagedAccount,
                auth_provider: Some("agy_oauth".into()),
                account_id: Some("alice".into()),
            }),
            ..Default::default()
        });
        state.db.save_provider(&AppType::Agy, &alice_card).unwrap();

        let mut bob_card = alice_card.clone();
        bob_card
            .meta
            .as_mut()
            .unwrap()
            .auth_binding
            .as_mut()
            .unwrap()
            .account_id = Some("bob".into());
        assert!(save_from_editor(&state, &manager, bob_card).is_err());

        // DB row must remain bound to alice and keychain must stay on alice.
        let persisted = state
            .db
            .get_provider_by_id("official", &AppType::Agy)
            .unwrap()
            .unwrap();
        assert_eq!(
            persisted
                .meta
                .as_ref()
                .and_then(|m| m.managed_account_id_for("agy_oauth"))
                .as_deref(),
            Some("alice")
        );
        assert_eq!(live_sub(&login).as_deref(), Some("alice"));
    }
}
