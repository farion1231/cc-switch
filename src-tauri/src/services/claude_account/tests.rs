use super::*;

impl ClaudeAccountManager {
    /// 测试用：不跑登录，直接收录一个账号并建好目录。
    async fn insert_for_test(&self, key: &str, email: &str) -> PathBuf {
        let local_id = layout::new_local_id();
        let config_dir = layout::config_dir(&self.data_dir, &local_id);
        create_private_dir(&config_dir).unwrap();
        let mut store = self.store.lock().await;
        store.accounts.insert(
            key.to_string(),
            AccountData {
                local_id,
                email: Some(email.to_string()),
                org_name: None,
                plan: Some("max".to_string()),
                rate_limit_tier: None,
                added_at: 1,
            },
        );
        self.persist(&store).unwrap();
        config_dir
    }
}

#[tokio::test]
async fn persists_and_reloads_accounts() {
    let tmp = tempfile::tempdir().unwrap();
    let manager = ClaudeAccountManager::new(tmp.path().to_path_buf());
    let dir = manager.insert_for_test("acc:org", "me@example.com").await;

    let reloaded = ClaudeAccountManager::new(tmp.path().to_path_buf());
    let accounts = reloaded.list().await;
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].id, "acc:org");
    assert_eq!(accounts[0].label(), "me@example.com");
    assert_eq!(reloaded.config_dir_of("acc:org").await, Some(dir.clone()));
    assert_eq!(reloaded.all_config_dirs().await, vec![dir]);
    assert_eq!(reloaded.config_dir_of("other").await, None);
}

#[tokio::test]
async fn account_without_credentials_needs_reauth() {
    let tmp = tempfile::tempdir().unwrap();
    let manager = ClaudeAccountManager::new(tmp.path().to_path_buf());
    let dir = manager.insert_for_test("acc:", "me@example.com").await;
    // 只看在不在：测试环境的钥匙串里没有这个目录的条目
    assert!(manager.list().await[0].requires_reauth);

    std::fs::write(dir.join(".credentials.json"), "{}").unwrap();
    assert!(!manager.list().await[0].requires_reauth);
}

#[tokio::test]
async fn backup_login_counts_and_expiry_is_listed_and_removed() {
    let tmp = tempfile::tempdir().unwrap();
    let manager = ClaudeAccountManager::new(tmp.path().to_path_buf());
    let dir = manager.insert_for_test("acc:", "me@example.com").await;
    let backup = snapshot::backup_dir(&dir);
    std::fs::create_dir_all(&backup).unwrap();
    std::fs::write(backup.join(".credentials.json"), "{}").unwrap();
    snapshot::update_state(tmp.path(), |state| {
        state.login_expiry.insert("acc:".to_string(), 42);
    })
    .unwrap();

    let listed = &manager.list().await[0];
    assert!(!listed.requires_reauth);
    assert_eq!(listed.login_expires_at, Some(42));

    manager.remove("acc:").await.unwrap();
    assert!(!backup.exists());
    assert!(snapshot::load_state(tmp.path()).login_expiry.is_empty());
}

#[tokio::test]
async fn missing_dir_resolves_to_none() {
    let tmp = tempfile::tempdir().unwrap();
    let manager = ClaudeAccountManager::new(tmp.path().to_path_buf());
    let dir = manager.insert_for_test("acc:", "me@example.com").await;
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(manager.config_dir_of("acc:").await, None);
    assert!(manager.list().await[0].requires_reauth);
}

#[tokio::test]
async fn remove_deletes_account_dir_but_not_link_targets() {
    let tmp = tempfile::tempdir().unwrap();
    let manager = ClaudeAccountManager::new(tmp.path().to_path_buf());
    let dir = manager.insert_for_test("acc:", "me@example.com").await;

    // 共享层把默认目录链进账号目录：删号只能删链接本身
    let shared = tmp.path().join("shared-skills");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::write(shared.join("keep.md"), "keep").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&shared, dir.join("skills")).unwrap();

    manager.remove("acc:").await.unwrap();
    assert!(!dir.exists());
    assert!(!dir.parent().unwrap().exists());
    assert!(shared.join("keep.md").is_file());
    assert!(manager.list().await.is_empty());
    assert!(manager.remove("acc:").await.is_err());
}

#[tokio::test]
async fn corrupt_local_id_never_resolves() {
    let tmp = tempfile::tempdir().unwrap();
    let manager = ClaudeAccountManager::new(tmp.path().to_path_buf());
    {
        let mut store = manager.store.lock().await;
        store.accounts.insert(
            "acc:".to_string(),
            AccountData {
                local_id: "../../escape".to_string(),
                email: None,
                org_name: None,
                plan: None,
                rate_limit_tier: None,
                added_at: 0,
            },
        );
        // 账号表以磁盘为准（切换时可能收录账号）
        manager.persist(&store).unwrap();
    }
    assert_eq!(manager.config_dir_of("acc:").await, None);
    assert!(manager.all_config_dirs().await.is_empty());
    assert!(manager.list().await[0].requires_reauth);
}

#[tokio::test]
async fn polls_after_login_ends_never_report_expired() {
    // 前端不等上一次轮询返回：收录期间回「还没好」，收录完（命令层还在刷新 live）回同一个账号
    let tmp = tempfile::tempdir().unwrap();
    let manager = ClaudeAccountManager::new(tmp.path().to_path_buf());
    manager
        .ended_flows()
        .insert("flow".to_string(), FlowEnd::Finishing);
    assert!(manager.poll_login("flow").await.unwrap().is_none());

    manager.insert_for_test("acc:", "me@example.com").await;
    let account = manager.list().await.remove(0);
    manager
        .ended_flows()
        .insert("flow".to_string(), FlowEnd::Added(account));
    let again = manager.poll_login("flow").await.unwrap().unwrap();
    assert_eq!(again.id, "acc:");

    manager.ended_flows().clear();
    assert!(manager.poll_login("flow").await.is_err());
}

#[tokio::test]
async fn plan_from_the_credentials_updates_the_account() {
    let tmp = tempfile::tempdir().unwrap();
    let manager = ClaudeAccountManager::new(tmp.path().to_path_buf());
    manager.insert_for_test("acc:org", "me@example.com").await;

    let max_20x = AccountPlan {
        subscription_type: Some("max".to_string()),
        rate_limit_tier: Some("default_claude_max_20x".to_string()),
    };
    note_plan(tmp.path(), "acc:org", &max_20x);
    let listed = &manager.list().await[0];
    assert_eq!(listed.plan.as_deref(), Some("max"));
    assert_eq!(
        listed.rate_limit_tier.as_deref(),
        Some("default_claude_max_20x")
    );

    // 凭据里没写套餐（旧版 Claude Code）：不抹掉已有的记录
    note_plan(tmp.path(), "acc:org", &AccountPlan::default());
    let listed = &manager.list().await[0];
    assert_eq!(
        listed.rate_limit_tier.as_deref(),
        Some("default_claude_max_20x")
    );

    // 降级到 Pro 后跟着变；不认识的账号不收录
    note_plan(
        tmp.path(),
        "acc:org",
        &AccountPlan {
            subscription_type: Some("pro".to_string()),
            rate_limit_tier: None,
        },
    );
    note_plan(tmp.path(), "other:org", &max_20x);
    let accounts = manager.list().await;
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].plan.as_deref(), Some("pro"));
    assert_eq!(accounts[0].rate_limit_tier, None);
}
