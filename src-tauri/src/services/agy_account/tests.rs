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

    /// agy 1.5 自己写回钥匙串的格式：没有 id_token，只有 id_token_email 和 writer_id。
    fn agy15_credential(sub: &str, access: &str) -> Value {
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
        // agy 续过的令牌照样收回来，查额度直接用
        assert_eq!(
            manager.access_token("alice").await.unwrap(),
            "renewed-by-agy15"
        );
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
        assert_eq!(manager.live_account_id().await.as_deref(), Some("alice"));
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
        assert_eq!(manager.live_account_id().await, None);

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
        // 第一次经带 id_token 的旧格式或 userinfo 认出是谁时，顺手把邮箱补进 dismissed_emails
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

    #[tokio::test]
    async fn stored_refreshed_token_is_not_overwritten_by_stale_keychain_login() {
        let (_dir, login, manager) = manager();
        add(&manager, "alice").await;
        // 钥匙串里还是过期的旧令牌，CC Switch 查额度时已经续期并存在自己的副本里
        set_live(&login, &agy15_credential("alice", "expired-in-keychain"));
        {
            let mut store = manager.store.lock().await;
            let cred = &mut store.accounts.get_mut("alice").unwrap().credential;
            cred["token"]["access_token"] = json!("refreshed-by-cc-switch");
            cred["token"]["expiry"] = json!(in_an_hour());
        }
        assert_eq!(
            manager
                .status()
                .await
                .unwrap()
                .current_account_id
                .as_deref(),
            Some("alice")
        );
        assert_eq!(
            manager.access_token("alice").await.unwrap(),
            "refreshed-by-cc-switch"
        );
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

/// Same rule as WorkBuddy: listing accounts adopts the live login and saves, and
/// that save must not destroy an account file it could not read.
#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
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
