use std::sync::Mutex as StdMutex;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::json;

use super::*;

#[derive(Default)]
struct MemoryLogin {
    value: StdMutex<Option<String>>,
    reads: StdMutex<usize>,
    writes: StdMutex<usize>,
    locked: StdMutex<bool>,
    post_write_read_failures: StdMutex<usize>,
}

impl AgyLoginStore for MemoryLogin {
    fn read(&self) -> Result<Option<String>, AppError> {
        *self.reads.lock().unwrap() += 1;
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
        assert_eq!(*login.reads.lock().unwrap(), 1);
        assert_eq!(*login.writes.lock().unwrap(), 0, "查额度不写钥匙串");
    }

    #[tokio::test]
    async fn fresh_stored_token_skips_keychain_read() {
        let (_dir, login, manager) = manager();
        add(&manager, "alice").await;
        manager
            .store
            .lock()
            .await
            .accounts
            .get_mut("alice")
            .unwrap()
            .credential["token"]["expiry"] = json!(in_an_hour());
        let mut live = credential("alice", "renewed-by-agy");
        live["token"]["expiry"] = json!(in_an_hour());
        set_live(&login, &live);

        assert_eq!(manager.access_token("alice").await.unwrap(), "fresh");
        assert_eq!(*login.reads.lock().unwrap(), 0, "副本有效时不读钥匙串");

        // 账号尚未收录但钥匙串里正登着它（如冷启动云同步卡片抢在 status() 前查额度）：仍读钥匙串收回
        let mut unimported_bob = credential("bob", "live-bob");
        unimported_bob["id_token"] = json!(format!(
            "e30.{}.sig",
            URL_SAFE_NO_PAD.encode(
                serde_json::to_vec(&json!({
                    "sub": "bob",
                    "email": "bob@example.com",
                    "picture": "https://lh3.googleusercontent.com/a/bob=s96-c"
                }))
                .unwrap()
            )
        ));
        unimported_bob["token"]["expiry"] = json!(in_an_hour());
        set_live(&login, &unimported_bob);
        assert_eq!(manager.access_token("bob").await.unwrap(), "live-bob");
        assert_eq!(*login.reads.lock().unwrap(), 1);

        // 删掉的账号即使钥匙串里还登着它，也不会被重新收回
        manager.remove("bob").await.unwrap();
        assert!(matches!(
            manager.access_token("bob").await,
            Err(AccountQuotaError::NotFound)
        ));
        assert_eq!(*login.reads.lock().unwrap(), 2);
    }

    #[tokio::test]
    async fn expired_or_reauth_account_reads_keychain_to_recover_live_login() {
        let (_dir, login, manager) = manager();

        // 1. 副本令牌已过期：读一次钥匙串收回 agy 续过的令牌；之后副本已新鲜，不再读钥匙串
        add(&manager, "alice").await;
        let mut live_alice = credential("alice", "renewed-by-agy");
        live_alice["token"]["expiry"] = json!(in_an_hour());
        set_live(&login, &live_alice);
        assert_eq!(
            manager.access_token("alice").await.unwrap(),
            "renewed-by-agy"
        );
        assert_eq!(*login.reads.lock().unwrap(), 1);
        assert_eq!(
            manager.access_token("alice").await.unwrap(),
            "renewed-by-agy"
        );
        assert_eq!(*login.reads.lock().unwrap(), 1);

        // 2. 账号被标为 requires_reauth：读钥匙串收回用户在 agy 里重新登录的结果
        add(&manager, "bob").await;
        {
            let mut store = manager.store.lock().await;
            let bob = store.accounts.get_mut("bob").unwrap();
            bob.requires_reauth = true;
            bob.credential["token"]["expiry"] = json!(in_an_hour());
        }
        let mut relogged_bob = credential("bob", "reauthed-by-agy");
        relogged_bob["token"]["refresh_token"] = json!("refresh-bob-new");
        relogged_bob["token"]["expiry"] = json!(in_an_hour());
        set_live(&login, &relogged_bob);
        assert_eq!(
            manager.access_token("bob").await.unwrap(),
            "reauthed-by-agy"
        );
        assert_eq!(*login.reads.lock().unwrap(), 2);
        assert!(!manager.store.lock().await.accounts["bob"].requires_reauth);

        // 3. 账号仍处于 requires_reauth 且钥匙串里不是它的新登录：读一次后仍返回 NeedsReauth
        add(&manager, "charlie").await;
        manager
            .store
            .lock()
            .await
            .accounts
            .get_mut("charlie")
            .unwrap()
            .requires_reauth = true;
        assert!(matches!(
            manager.access_token("charlie").await,
            Err(AccountQuotaError::NeedsReauth)
        ));
        assert_eq!(*login.reads.lock().unwrap(), 3);

        // 4. 账号处于 requires_reauth 且钥匙串里仍是同一份旧凭据：不误清 requires_reauth
        set_live(&login, &credential("charlie", "fresh"));
        assert!(matches!(
            manager.access_token("charlie").await,
            Err(AccountQuotaError::NeedsReauth)
        ));
        assert_eq!(*login.reads.lock().unwrap(), 4);
        assert!(manager.store.lock().await.accounts["charlie"].requires_reauth);
    }

    #[tokio::test]
    async fn quota_with_backfill_recovers_reauth_account_from_keychain_and_backfills_picture() {
        let (_dir, login, manager) = manager();
        add(&manager, "alice").await;
        {
            let mut store = manager.store.lock().await;
            let alice = store.accounts.get_mut("alice").unwrap();
            alice.requires_reauth = true;
            alice.authenticated_at = 100;
            alice.picture = None;
        }
        let mut relogged = agy15_credential("alice", "reauthed-by-agy");
        relogged["token"]["refresh_token"] = json!("refresh-alice-new");
        relogged["token"]["expiry"] = json!(in_an_hour());
        set_live(&login, &relogged);

        let manager_ref = &manager;
        let result = manager
            .quota_with_backfill(
                "alice",
                |token| async move {
                    assert_eq!(token, "reauthed-by-agy");
                    Ok(quota::AccountQuota {
                        tiers: Vec::new(),
                        plan: Some("Pro".into()),
                    })
                },
                move |id, token| async move {
                    assert_eq!(
                        manager_ref.quota_permits.available_permits(),
                        MAX_CONCURRENT_QUOTA_QUERIES - 1
                    );
                    manager_ref
                        .backfill_missing_picture_with(&id, || async move {
                            assert_eq!(token, "reauthed-by-agy");
                            Some(oauth::IdentityClaims {
                                sub: Some("alice".into()),
                                email: None,
                                name: Some("Alice".into()),
                                picture: Some(
                                    "https://lh3.googleusercontent.com/a-/recovered=s96-c".into(),
                                ),
                            })
                        })
                        .await;
                },
            )
            .await;
        assert!(result.is_ok());
        assert_eq!(*login.reads.lock().unwrap(), 1);
        assert_eq!(
            manager.quota_permits.available_permits(),
            MAX_CONCURRENT_QUOTA_QUERIES
        );
        {
            let store = manager.store.lock().await;
            let alice = &store.accounts["alice"];
            assert!(!alice.requires_reauth);
            assert!(alice.authenticated_at > 100);
            assert_eq!(
                alice.picture.as_deref(),
                Some("https://lh3.googleusercontent.com/a-/recovered=s96-c")
            );
        }

        // 恢复且已补齐头像后，再次查额度走新鲜副本快路径（0 次钥匙串读取，也不再补查 userinfo）
        let second = manager
            .quota_with_backfill(
                "alice",
                |token| async move {
                    assert_eq!(token, "reauthed-by-agy");
                    Ok(quota::AccountQuota {
                        tiers: Vec::new(),
                        plan: Some("Pro".into()),
                    })
                },
                move |id, _token| async move {
                    manager_ref
                        .backfill_missing_picture_with(&id, || async {
                            panic!("userinfo should not be fetched once picture is populated");
                        })
                        .await;
                },
            )
            .await;
        assert!(second.is_ok());
        assert_eq!(*login.reads.lock().unwrap(), 1);
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

#[tokio::test]
async fn quota_limits_concurrent_upstream_requests() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let (_dir, login, manager) = manager();
    let total = MAX_CONCURRENT_QUOTA_QUERIES * 2;
    let ids: Vec<String> = (0..total).map(|index| format!("acc-{index}")).collect();
    for id in &ids {
        add(&manager, id).await;
        let mut store = manager.store.lock().await;
        let account = store.accounts.get_mut(id).unwrap();
        account.credential["token"]["expiry"] = json!(in_an_hour());
        // 已有头像时跳过锁外 OIDC userinfo 补查，避免并发测试触发真实外网请求
        account.picture = Some("https://lh3.googleusercontent.com/a/public=s96-c".into());
    }

    let in_flight = Arc::new(AtomicUsize::new(0));
    let max_in_flight = Arc::new(AtomicUsize::new(0));
    let (entered_tx, mut entered_rx) = tokio::sync::watch::channel(0usize);
    let (release_tx, release_rx) = tokio::sync::watch::channel(false);

    let queries = futures::future::join_all(ids.iter().enumerate().map(|(index, id)| {
        let manager = &manager;
        let in_flight = Arc::clone(&in_flight);
        let max_in_flight = Arc::clone(&max_in_flight);
        let entered_tx = entered_tx.clone();
        let release_rx = release_rx.clone();
        async move {
            manager
                .quota_with(id, move |_token| {
                    let in_flight = Arc::clone(&in_flight);
                    let max_in_flight = Arc::clone(&max_in_flight);
                    let entered_tx = entered_tx.clone();
                    let mut release_rx = release_rx.clone();
                    async move {
                        let current = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                        max_in_flight.fetch_max(current, Ordering::SeqCst);
                        entered_tx.send_modify(|count| *count += 1);
                        let _ = release_rx.wait_for(|released| *released).await;
                        in_flight.fetch_sub(1, Ordering::SeqCst);
                        if index == 0 {
                            Err(quota::QuotaError::Network("boom".into()))
                        } else {
                            Ok(quota::AccountQuota {
                                tiers: Vec::new(),
                                plan: Some("Free".into()),
                            })
                        }
                    }
                })
                .await
        }
    }));

    let driver = async {
        entered_rx
            .wait_for(|&count| count >= MAX_CONCURRENT_QUOTA_QUERIES)
            .await
            .unwrap();
        // 给排队中的其余任务足够调度时间，确认它们都被信号量挡在外面
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(
            in_flight.load(Ordering::SeqCst),
            MAX_CONCURRENT_QUOTA_QUERIES
        );
        assert_eq!(
            max_in_flight.load(Ordering::SeqCst),
            MAX_CONCURRENT_QUOTA_QUERIES
        );
        let _ = release_tx.send(true);
    };

    let ((), results) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(driver, queries)
    })
    .await
    .expect("quota concurrency test timed out");

    assert_eq!(in_flight.load(Ordering::SeqCst), 0);
    assert_eq!(*entered_rx.borrow(), total);
    assert_eq!(
        max_in_flight.load(Ordering::SeqCst),
        MAX_CONCURRENT_QUOTA_QUERIES
    );
    assert!(matches!(results[0], Err(AccountQuotaError::Failed(_))));
    assert!(results[1..].iter().all(|result| result.is_ok()));
    assert_eq!(
        manager.quota_permits.available_permits(),
        MAX_CONCURRENT_QUOTA_QUERIES
    );
    assert_eq!(*login.reads.lock().unwrap(), 0);

    // 取消进行中的查额度任务也不应泄漏信号量许可
    let (cancel_entered_tx, cancel_entered_rx) = tokio::sync::oneshot::channel();
    let mut cancel_entered_tx = Some(cancel_entered_tx);
    let mut pending_query = Box::pin(manager.quota_with(&ids[0], move |_token| {
        if let Some(tx) = cancel_entered_tx.take() {
            let _ = tx.send(());
        }
        std::future::pending::<Result<quota::AccountQuota, quota::QuotaError>>()
    }));
    tokio::select! {
        _ = &mut pending_query => panic!("pending query should not finish"),
        _ = cancel_entered_rx => {}
    }
    assert_eq!(
        manager.quota_permits.available_permits(),
        MAX_CONCURRENT_QUOTA_QUERIES - 1
    );
    drop(pending_query);
    assert_eq!(
        manager.quota_permits.available_permits(),
        MAX_CONCURRENT_QUOTA_QUERIES
    );
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

#[tokio::test]
async fn quota_with_backfill_runs_under_permit_only_when_fetch_succeeds() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let (_dir, _login, manager) = manager();
    add(&manager, "alice").await;
    manager
        .store
        .lock()
        .await
        .accounts
        .get_mut("alice")
        .unwrap()
        .credential["token"]["expiry"] = json!(in_an_hour());

    // 额度查询失败时绝不补头像，信号量许可正常归还
    let backfill_calls = Arc::new(AtomicUsize::new(0));
    let calls_on_err = Arc::clone(&backfill_calls);
    let err = manager
        .quota_with_backfill(
            "alice",
            |_token| async { Err(quota::QuotaError::Network("boom".into())) },
            move |_id, _token| async move {
                calls_on_err.fetch_add(1, Ordering::SeqCst);
            },
        )
        .await;
    assert!(matches!(err, Err(AccountQuotaError::Failed(_))));
    assert_eq!(backfill_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        manager.quota_permits.available_permits(),
        MAX_CONCURRENT_QUOTA_QUERIES
    );

    // 未刚续期且额度查询成功时，在信号量许可内触发补头像，返回后释放许可
    let observed_permits = Arc::new(AtomicUsize::new(usize::MAX));
    let observed_permits_ref = Arc::clone(&observed_permits);
    let calls_on_ok = Arc::clone(&backfill_calls);
    let manager_ref = &manager;
    let ok = manager
        .quota_with_backfill(
            "alice",
            |token| async move {
                assert_eq!(token, "fresh");
                Ok(quota::AccountQuota {
                    tiers: Vec::new(),
                    plan: Some("Pro".into()),
                })
            },
            move |id, token| async move {
                assert_eq!(id, "alice");
                assert_eq!(token, "fresh");
                observed_permits_ref.store(
                    manager_ref.quota_permits.available_permits(),
                    Ordering::SeqCst,
                );
                calls_on_ok.fetch_add(1, Ordering::SeqCst);
            },
        )
        .await;
    assert!(ok.is_ok());
    assert_eq!(backfill_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        observed_permits.load(Ordering::SeqCst),
        MAX_CONCURRENT_QUOTA_QUERIES - 1
    );
    assert_eq!(
        manager.quota_permits.available_permits(),
        MAX_CONCURRENT_QUOTA_QUERIES
    );
}

#[test]
fn private_avatar_detection_matches_google_private_paths_only() {
    assert!(is_private_avatar_url(
        "https://lh3.googleusercontent.com/a-/ALV-UjX123=s96-c"
    ));
    assert!(is_private_avatar_url(
        "https://LH4.GoogleUserContent.com/a-/ALV-UjX123=s96-c"
    ));
    assert!(!is_private_avatar_url(
        "https://lh3.googleusercontent.com/a/ACg8oc123=s96-c"
    ));
    assert!(!is_private_avatar_url(
        "https://lh3.googleusercontent.com.evil.example/a-/ALV-UjX123=s96-c"
    ));
    assert!(!is_private_avatar_url(
        "ftp://lh3.googleusercontent.com/a-/ALV-UjX123=s96-c"
    ));
    assert!(!is_private_avatar_url("not-a-url"));

    assert!(needs_avatar_refresh(None));
    assert!(needs_avatar_refresh(Some("")));
    assert!(needs_avatar_refresh(Some("   ")));
    assert!(needs_avatar_refresh(Some(
        "https://lh3.googleusercontent.com/a-/ALV-UjX123=s96-c"
    )));
    assert!(!needs_avatar_refresh(Some(
        "https://lh3.googleusercontent.com/a/ACg8oc123=s96-c"
    )));
}

#[test]
fn merge_identity_fills_missing_or_private_avatar_and_guards_sub() {
    let base = oauth::IdentityClaims {
        sub: Some("sub-1".into()),
        email: Some("alice@example.com".into()),
        name: None,
        picture: None,
    };
    let fetched = oauth::IdentityClaims {
        sub: Some("sub-1".into()),
        email: Some("other@example.com".into()),
        name: Some("Alice".into()),
        picture: Some("https://lh3.googleusercontent.com/a-/private-1=s96-c".into()),
    };
    let merged = merge_identity(base, fetched);
    assert_eq!(merged.email.as_deref(), Some("alice@example.com"));
    assert_eq!(merged.name.as_deref(), Some("Alice"));
    assert_eq!(
        merged.picture.as_deref(),
        Some("https://lh3.googleusercontent.com/a-/private-1=s96-c")
    );

    // 私有 /a-/ 头像可以被 userinfo 返回的新地址更新
    let rotated = merge_identity(
        merged.clone(),
        oauth::IdentityClaims {
            sub: Some("sub-1".into()),
            email: None,
            name: None,
            picture: Some("https://lh3.googleusercontent.com/a-/private-2=s96-c".into()),
        },
    );
    assert_eq!(
        rotated.picture.as_deref(),
        Some("https://lh3.googleusercontent.com/a-/private-2=s96-c")
    );

    // 已有公开 /a/ 头像时不被 userinfo 覆盖，但空白 name 仍补齐
    let public_kept = merge_identity(
        oauth::IdentityClaims {
            sub: Some("sub-1".into()),
            email: Some("alice@example.com".into()),
            name: None,
            picture: Some("https://lh3.googleusercontent.com/a/public-1=s96-c".into()),
        },
        oauth::IdentityClaims {
            sub: Some("sub-1".into()),
            email: None,
            name: Some("Alice".into()),
            picture: Some("https://lh3.googleusercontent.com/a/public-2=s96-c".into()),
        },
    );
    assert_eq!(public_kept.name.as_deref(), Some("Alice"));
    assert_eq!(
        public_kept.picture.as_deref(),
        Some("https://lh3.googleusercontent.com/a/public-1=s96-c")
    );

    // sub 不一致或 fetched.picture 仅空白时不污染原身份
    let mismatched = merge_identity(
        oauth::IdentityClaims {
            sub: Some("sub-1".into()),
            email: Some("alice@example.com".into()),
            name: None,
            picture: None,
        },
        oauth::IdentityClaims {
            sub: Some("sub-2".into()),
            email: Some("bob@example.com".into()),
            name: Some("Bob".into()),
            picture: Some("https://lh3.googleusercontent.com/a-/bob=s96-c".into()),
        },
    );
    assert_eq!(mismatched.name, None);
    assert_eq!(mismatched.picture, None);

    let blank_picture = merge_identity(
        rotated.clone(),
        oauth::IdentityClaims {
            sub: Some("sub-1".into()),
            email: None,
            name: None,
            picture: Some("   ".into()),
        },
    );
    assert_eq!(blank_picture.picture, rotated.picture);
}

#[test]
fn apply_refreshed_identity_updates_matching_account_only() {
    let mut account = AccountData {
        sub: "sub-1".into(),
        email: Some("alice@example.com".into()),
        name: None,
        picture: None,
        credential: credential("sub-1", "a1"),
        authenticated_at: 100,
        requires_reauth: false,
    };

    assert!(!apply_refreshed_identity(&mut account, None));
    assert!(!apply_refreshed_identity(
        &mut account,
        Some(oauth::IdentityClaims {
            sub: Some("other".into()),
            email: Some("other@example.com".into()),
            name: Some("Other".into()),
            picture: Some("https://lh3.googleusercontent.com/a-/other=s96-c".into()),
        })
    ));
    assert_eq!(account.picture, None);

    assert!(apply_refreshed_identity(
        &mut account,
        Some(oauth::IdentityClaims {
            sub: Some("sub-1".into()),
            email: None,
            name: Some("Alice".into()),
            picture: Some("https://lh3.googleusercontent.com/a-/v1=s96-c".into()),
        })
    ));
    assert_eq!(account.name.as_deref(), Some("Alice"));
    assert_eq!(
        account.picture.as_deref(),
        Some("https://lh3.googleusercontent.com/a-/v1=s96-c")
    );

    // 相同值或空白值不重复标记变更
    assert!(!apply_refreshed_identity(
        &mut account,
        Some(oauth::IdentityClaims {
            sub: Some("sub-1".into()),
            email: Some("   ".into()),
            name: Some("Alice".into()),
            picture: Some("https://lh3.googleusercontent.com/a-/v1=s96-c".into()),
        })
    ));

    // 私有头像轮换到新 URL 时更新
    assert!(apply_refreshed_identity(
        &mut account,
        Some(oauth::IdentityClaims {
            sub: Some("sub-1".into()),
            email: None,
            name: None,
            picture: Some("https://lh3.googleusercontent.com/a-/v2=s96-c".into()),
        })
    ));
    assert_eq!(
        account.picture.as_deref(),
        Some("https://lh3.googleusercontent.com/a-/v2=s96-c")
    );
}

#[tokio::test]
async fn enrich_identity_fetches_only_when_needed_and_handles_missing_base_or_failure() {
    let base = oauth::IdentityClaims {
        sub: Some("sub-1".into()),
        email: Some("alice@example.com".into()),
        name: Some("Alice".into()),
        picture: None,
    };

    // should_fetch = false 时绝不触发 userinfo 请求
    let skipped = enrich_identity(Some(base.clone()), false, || async {
        panic!("userinfo should not be fetched");
    })
    .await
    .unwrap();
    assert_eq!(skipped.picture, None);

    // 缺少 picture 时合并 userinfo 返回的私有头像
    let enriched = enrich_identity(Some(base.clone()), true, || async {
        Some(oauth::IdentityClaims {
            sub: Some("sub-1".into()),
            email: None,
            name: None,
            picture: Some("https://lh3.googleusercontent.com/a-/custom=s96-c".into()),
        })
    })
    .await
    .unwrap();
    assert_eq!(
        enriched.picture.as_deref(),
        Some("https://lh3.googleusercontent.com/a-/custom=s96-c")
    );

    // 续期响应不含 id_token（base = None）时直接采用 userinfo 结果
    let fetched_only = enrich_identity(None, true, || async {
        Some(oauth::IdentityClaims {
            sub: Some("sub-1".into()),
            email: Some("alice@example.com".into()),
            name: None,
            picture: Some("https://lh3.googleusercontent.com/a-/custom=s96-c".into()),
        })
    })
    .await
    .unwrap();
    assert_eq!(fetched_only.sub.as_deref(), Some("sub-1"));
    assert_eq!(
        fetched_only.picture.as_deref(),
        Some("https://lh3.googleusercontent.com/a-/custom=s96-c")
    );

    // userinfo 请求失败时保留原 id_token 身份，不中断流程
    let fallback = enrich_identity(Some(base), true, || async { None })
        .await
        .unwrap();
    assert_eq!(fallback.sub.as_deref(), Some("sub-1"));
    assert_eq!(fallback.picture, None);
}

#[test]
fn absorb_bumps_authenticated_at_only_when_recovering_from_requires_reauth() {
    let mut store = StoreFile::default();
    let old_cred = credential("alice", "old");
    let claims = oauth::identity(old_cred["id_token"].as_str().unwrap()).unwrap();
    upsert(&mut store, old_cred, &claims);
    store.accounts.get_mut("alice").unwrap().authenticated_at = 100;

    // 正常续期：凭据变化但不处于 requires_reauth，不改 authenticated_at
    let refreshed_cred = credential("alice", "refreshed");
    let refreshed_claims = oauth::identity(refreshed_cred["id_token"].as_str().unwrap()).unwrap();
    let (id, changed) = absorb(
        &mut store,
        LiveLogin::Known {
            credential: refreshed_cred,
            claims: refreshed_claims,
        },
        false,
    );
    assert_eq!(id.as_deref(), Some("alice"));
    assert!(changed);
    assert_eq!(store.accounts["alice"].authenticated_at, 100);

    // 失效后经 agy 重新登录：清除 requires_reauth 并刷新 authenticated_at（驱动前端刷新缓存头像）
    store.accounts.get_mut("alice").unwrap().requires_reauth = true;
    let reauthed_cred = credential("alice", "reauthed");
    let reauthed_claims = oauth::identity(reauthed_cred["id_token"].as_str().unwrap()).unwrap();
    let (id, changed) = absorb(
        &mut store,
        LiveLogin::Known {
            credential: reauthed_cred,
            claims: reauthed_claims,
        },
        false,
    );
    assert_eq!(id.as_deref(), Some("alice"));
    assert!(changed);
    assert!(!store.accounts["alice"].requires_reauth);
    assert!(store.accounts["alice"].authenticated_at > 100);
}

#[tokio::test]
async fn backfill_missing_picture_populates_blank_skips_existing_and_preserves_concurrent_login() {
    let (dir, login, manager) = manager();
    add(&manager, "alice").await;
    assert_eq!(manager.account_picture("alice").await, None);

    // 首次缺失头像时补齐并落盘
    manager
        .backfill_missing_picture_with("alice", || async {
            Some(oauth::IdentityClaims {
                sub: Some("alice".into()),
                email: None,
                name: Some("Alice".into()),
                picture: Some("https://lh3.googleusercontent.com/a-/backfilled=s96-c".into()),
            })
        })
        .await;
    assert_eq!(
        manager.account_picture("alice").await.as_deref(),
        Some("https://lh3.googleusercontent.com/a-/backfilled=s96-c")
    );
    let reopened = AgyAccountManager::with_login_store(dir.path().to_path_buf(), login.clone());
    assert_eq!(
        reopened.account_picture("alice").await.as_deref(),
        Some("https://lh3.googleusercontent.com/a-/backfilled=s96-c")
    );

    // 已有头像时绝不再触发 userinfo 请求
    manager
        .backfill_missing_picture_with("alice", || async {
            panic!("userinfo should not be fetched when picture already exists");
        })
        .await;

    // 并发保护：锁外请求期间如果其他路径（如重新登录）已经填上新头像，不用旧响应覆盖
    add(&manager, "bob").await;
    manager
        .backfill_missing_picture_with("bob", || async {
            manager
                .store
                .lock()
                .await
                .accounts
                .get_mut("bob")
                .unwrap()
                .picture = Some("https://lh3.googleusercontent.com/a-/fresh-login=s96-c".into());
            Some(oauth::IdentityClaims {
                sub: Some("bob".into()),
                email: None,
                name: Some("Stale".into()),
                picture: Some("https://lh3.googleusercontent.com/a-/stale-backfill=s96-c".into()),
            })
        })
        .await;
    assert_eq!(
        manager.account_picture("bob").await.as_deref(),
        Some("https://lh3.googleusercontent.com/a-/fresh-login=s96-c")
    );
}

#[test]
fn will_import_account_skips_existing_and_dismissed_unless_unlocking() {
    let mut store = StoreFile::default();
    let cred = credential("alice", "a1");
    let claims = oauth::identity(cred["id_token"].as_str().unwrap()).unwrap();
    upsert(&mut store, cred, &claims);
    store.dismissed.push("dismissed".into());

    assert!(!will_import_account(&store, None, false));
    assert!(!will_import_account(&store, Some("alice"), false));
    assert!(!will_import_account(&store, Some("alice"), true));
    assert!(!will_import_account(&store, Some("dismissed"), false));
    assert!(will_import_account(&store, Some("dismissed"), true));
    assert!(will_import_account(&store, Some("fresh"), false));
}
