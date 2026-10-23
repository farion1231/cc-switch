use serde_json::json;
use serial_test::serial;

use super::*;

fn credential(uid: &str, domain: &str, access: &str, session: &str) -> String {
    serde_json::to_string_pretty(&json!({
        "auth": {
            "accessToken": access,
            "refreshToken": format!("refresh-{uid}-{session}"),
            "tokenType": "Bearer",
            "expiresIn": 3600,
            "expiresAt": 4_102_444_800_000_i64,
            "refreshExpiresIn": 2_592_000,
            "refreshExpiresAt": 4_102_444_800_000_i64,
            "lastRefreshTime": 1_760_000_000_000_i64,
            "scope": "openid",
            "domain": domain,
            "sessionState": session,
            "notBeforePolicy": 0,
        },
        "account": {"uid": uid, "nickname": format!("nick-{uid}"), "type": "personal"},
        "accounts": [],
        "allAccounts": [],
    }))
    .unwrap()
}

struct Fixture {
    _dir: tempfile::TempDir,
    auth_dir: PathBuf,
    data_dir: PathBuf,
    manager: CodebuddyAccountManager,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let auth_dir = dir.path().join("auth");
        let data_dir = dir.path().join("data");
        std::fs::create_dir_all(&auth_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        let manager = CodebuddyAccountManager::with_auth_dir(data_dir.clone(), auth_dir.clone());
        Self {
            _dir: dir,
            auth_dir,
            data_dir,
            manager,
        }
    }

    fn set_live(&self, content: &str) {
        std::fs::write(self.auth_dir.join(CREDENTIAL_FILENAME), content).unwrap();
    }

    fn live(&self) -> String {
        std::fs::read_to_string(self.auth_dir.join(CREDENTIAL_FILENAME)).unwrap()
    }

    fn marker(&self) -> PathBuf {
        self.auth_dir.join(LOGGED_OUT_FILENAME)
    }
}

const CN: &str = "www.codebuddy.cn";
const INTL: &str = "www.codebuddy.ai";

#[tokio::test]
async fn status_does_not_import_an_uncollected_login() {
    let fx = Fixture::new();
    fx.set_live(&credential("100", CN, "a1", "s1"));
    let status = fx.manager.status().await.unwrap();
    assert!(status.accounts.is_empty());
    assert_eq!(status.current_account_id, None);
    let live = fx.manager.live_summary().await.unwrap();
    assert_eq!(live.state, LiveState::Uncollected);
    assert_eq!(live.account_id.as_deref(), Some("100@www.codebuddy.cn"));
    assert_eq!(live.label.as_deref(), Some("nick-100"));
}

#[tokio::test]
async fn import_current_collects_and_status_tracks_refreshes() {
    let fx = Fixture::new();
    fx.set_live(&credential("100", CN, "a1", "s1"));
    let account = fx.manager.import_current().await.unwrap();
    assert_eq!(account.id, "100@www.codebuddy.cn");
    assert_eq!(account.label(), "nick-100");
    assert!(!account.expired);

    // CodeBuddy 续期后写回文件：查看状态时更新副本
    let refreshed = credential("100", CN, "a2", "s1");
    fx.set_live(&refreshed);
    let status = fx.manager.status().await.unwrap();
    assert_eq!(
        status.current_account_id.as_deref(),
        Some(account.id.as_str())
    );
    let store = fx.manager.store.lock().await;
    assert_eq!(store.accounts[&account.id].raw, refreshed);
    drop(store);

    // 账号文件 0600，重新打开还在
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(fx.data_dir.join(STORE_FILENAME))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let reopened = CodebuddyAccountManager::with_auth_dir(fx.data_dir.clone(), fx.auth_dir.clone());
    assert_eq!(reopened.status().await.unwrap().accounts.len(), 1);
}

#[tokio::test]
async fn same_uid_on_both_sites_are_two_accounts() {
    let fx = Fixture::new();
    fx.set_live(&credential("100", CN, "a1", "s1"));
    fx.manager.import_current().await.unwrap();
    fx.set_live(&credential("100", INTL, "b1", "s2"));
    fx.manager.import_current().await.unwrap();
    let status = fx.manager.status().await.unwrap();
    assert_eq!(status.accounts.len(), 2);
    assert_eq!(
        status.current_account_id.as_deref(),
        Some("100@www.codebuddy.ai")
    );
}

#[tokio::test]
async fn logged_out_marker_wins_over_the_file() {
    let fx = Fixture::new();
    fx.set_live(&credential("100", CN, "a1", "s1"));
    fx.manager.import_current().await.unwrap();
    std::fs::write(fx.marker(), "").unwrap();
    let status = fx.manager.status().await.unwrap();
    assert_eq!(status.current_account_id, None);
    assert_eq!(
        fx.manager.live_summary().await.unwrap().state,
        LiveState::LoggedOut
    );
    assert!(fx.manager.import_current().await.is_err());
}

#[tokio::test]
async fn encrypted_envelope_is_stashed_and_never_overwritten() {
    let fx = Fixture::new();
    fx.set_live(&credential("100", CN, "a1", "s1"));
    fx.manager.import_current().await.unwrap();

    let envelope = r#"{"$wbEncrypted":1,"envelope":"opaque"}"#;
    fx.set_live(envelope);
    assert_eq!(
        fx.manager.live_summary().await.unwrap().state,
        LiveState::Unrecognized
    );
    assert!(fx.manager.import_current().await.is_err());
    assert!(fx.manager.switch_to("100@www.codebuddy.cn").await.is_err());
    assert_eq!(fx.live(), envelope);

    // 字段级信封也认不出
    let nested = r#"{"auth":{"$wbEncrypted":1,"envelope":"x"},"account":{"uid":"1"}}"#;
    fx.set_live(nested);
    assert!(fx.manager.import_current().await.is_err());

    let stash: Value =
        serde_json::from_slice(&std::fs::read(fx.data_dir.join(STASH_FILENAME)).unwrap()).unwrap();
    let raws: Vec<&str> = stash["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["raw"].as_str())
        .collect();
    assert_eq!(raws, vec![envelope, nested]);
    assert!(!fx.auth_dir.join(LOCK_DIRNAME).exists());
}

#[tokio::test]
async fn switch_absorbs_the_current_copy_then_writes_the_target() {
    let fx = Fixture::new();
    let alice = credential("100", CN, "a1", "s1");
    fx.set_live(&alice);
    fx.manager.import_current().await.unwrap();
    fx.set_live(&credential("200", INTL, "b1", "s2"));
    fx.manager.import_current().await.unwrap();

    // CLI 续期了 bob 的令牌、而且用户点过登出：切走前要收回最新那份
    let bob_refreshed = credential("200", INTL, "b2", "s2");
    fx.set_live(&bob_refreshed);
    fx.manager.switch_to("100@www.codebuddy.cn").await.unwrap();
    assert_eq!(fx.live(), alice);
    assert!(!fx.auth_dir.join(LOCK_DIRNAME).exists());
    {
        let store = fx.manager.store.lock().await;
        assert_eq!(store.accounts["200@www.codebuddy.ai"].raw, bob_refreshed);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(fx.auth_dir.join(CREDENTIAL_FILENAME))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    std::fs::write(fx.marker(), "").unwrap();
    fx.manager.switch_to("200@www.codebuddy.ai").await.unwrap();
    assert_eq!(fx.live(), bob_refreshed);
    assert!(!fx.marker().exists());
    let status = fx.manager.status().await.unwrap();
    assert_eq!(
        status.current_account_id.as_deref(),
        Some("200@www.codebuddy.ai")
    );
}

#[tokio::test]
async fn switching_away_from_an_uncollected_login_collects_it() {
    let fx = Fixture::new();
    fx.set_live(&credential("100", CN, "a1", "s1"));
    fx.manager.import_current().await.unwrap();
    let stranger = credential("300", CN, "c1", "s3");
    fx.set_live(&stranger);
    fx.manager.switch_to("100@www.codebuddy.cn").await.unwrap();
    let store = fx.manager.store.lock().await;
    assert_eq!(store.accounts["300@www.codebuddy.cn"].raw, stranger);
}

#[tokio::test]
async fn expired_accounts_are_flagged_and_not_switched_to() {
    let fx = Fixture::new();
    let mut value: Value = serde_json::from_str(&credential("100", CN, "a1", "s1")).unwrap();
    // 秒级时间戳也认
    value["auth"]["refreshExpiresAt"] = json!(1_000_000_000);
    fx.set_live(&value.to_string());
    let account = fx.manager.import_current().await.unwrap();
    assert!(account.expired);
    assert_eq!(account.refresh_expires_at, Some(1_000_000_000_000));
    fx.set_live(&credential("200", CN, "b1", "s2"));
    assert!(fx.manager.switch_to(&account.id).await.is_err());
}

#[tokio::test]
async fn a_held_lock_blocks_the_switch_and_a_stale_one_is_cleared() {
    let fx = Fixture::new();
    fx.set_live(&credential("100", CN, "a1", "s1"));
    fx.manager.import_current().await.unwrap();
    let bob = credential("200", CN, "b1", "s2");
    fx.set_live(&bob);
    let lock = fx.auth_dir.join(LOCK_DIRNAME);
    std::fs::create_dir(&lock).unwrap();
    assert!(fx.manager.switch_to("100@www.codebuddy.cn").await.is_err());
    assert_eq!(fx.live(), bob);
    assert!(lock.exists(), "别人的锁不能被删");

    #[cfg(unix)]
    {
        let old = std::time::SystemTime::now() - Duration::from_secs(60);
        std::fs::File::open(&lock)
            .unwrap()
            .set_modified(old)
            .unwrap();
        fx.manager.switch_to("100@www.codebuddy.cn").await.unwrap();
        assert!(!lock.exists());
        assert_ne!(fx.live(), bob);
    }
}

#[tokio::test]
async fn terminal_login_completes_on_a_new_account_or_session() {
    let fx = Fixture::new();
    fx.set_live(&credential("100", CN, "a1", "s1"));
    let start = fx.manager.start_login().await.unwrap();
    // 开始登录时把现在登着的号收进来
    assert_eq!(fx.manager.status().await.unwrap().accounts.len(), 1);
    assert!(fx
        .manager
        .poll_login(&start.flow_id)
        .await
        .unwrap()
        .is_none());

    // 同一会话的续期不算登录完成
    fx.set_live(&credential("100", CN, "a2", "s1"));
    assert!(fx
        .manager
        .poll_login(&start.flow_id)
        .await
        .unwrap()
        .is_none());

    // 登出后登录了另一个号
    std::fs::write(fx.marker(), "").unwrap();
    assert!(fx
        .manager
        .poll_login(&start.flow_id)
        .await
        .unwrap()
        .is_none());
    std::fs::remove_file(fx.marker()).unwrap();
    fx.set_live(&credential("200", INTL, "b1", "s2"));
    let account = fx
        .manager
        .poll_login(&start.flow_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account.id, "200@www.codebuddy.ai");
    assert!(fx.manager.poll_login(&start.flow_id).await.is_err());

    // 同一个号重新登录（新会话）
    let again = fx.manager.start_login().await.unwrap();
    fx.set_live(&credential("200", INTL, "b2", "s9"));
    let account = fx
        .manager
        .poll_login(&again.flow_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account.id, "200@www.codebuddy.ai");
    assert!(!fx.manager.cancel_login(&again.flow_id).await);
}

#[tokio::test]
async fn removing_copies_never_touches_codebuddy() {
    let fx = Fixture::new();
    let alice = credential("100", CN, "a1", "s1");
    fx.set_live(&alice);
    let account = fx.manager.import_current().await.unwrap();
    fx.manager.remove(&account.id).await.unwrap();
    assert_eq!(fx.live(), alice);
    assert!(!fx.marker().exists());
    fx.manager.import_current().await.unwrap();
    assert_eq!(fx.manager.remove_all().await.unwrap().len(), 1);
    assert_eq!(fx.live(), alice);
}

#[test]
fn parse_identity_requires_tokens_and_uid() {
    assert!(parse_identity("not json").is_none());
    assert!(parse_identity(r#"{"auth":{"accessToken":"a"},"account":{"uid":"1"}}"#).is_none());
    let numeric = r#"{"auth":{"accessToken":"a","refreshToken":"r"},"account":{"uid":42}}"#;
    let identity = parse_identity(numeric).unwrap();
    assert_eq!(identity.key(), "42@");
    assert_eq!(identity.refresh_expires_at, None);
    let derived = r#"{"auth":{"accessToken":"a","refreshToken":"r","lastRefreshTime":1000,"refreshExpiresIn":60},"account":{"uid":"1"}}"#;
    assert_eq!(
        parse_identity(derived).unwrap().refresh_expires_at,
        Some(1_060_000)
    );
}

#[test]
#[serial]
fn auth_dir_can_be_overridden() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var(AUTH_DIR_ENV, dir.path());
    assert_eq!(default_auth_dir(), dir.path());
    std::env::remove_var(AUTH_DIR_ENV);
    assert!(default_auth_dir().ends_with(
        Path::new("CodeBuddyExtension")
            .join("Data")
            .join("Public")
            .join("auth")
    ));
}

// ---------- 额度 ----------

#[test]
fn quota_base_url_only_for_known_sites() {
    assert_eq!(
        quota::base_url(Some("www.codebuddy.cn")).as_deref(),
        Some("https://www.codebuddy.cn")
    );
    assert_eq!(
        quota::base_url(Some("WWW.CodeBuddy.ai")).as_deref(),
        Some("https://www.codebuddy.ai")
    );
    assert_eq!(
        quota::base_url(Some("acme.sso.copilot.qq.com")).as_deref(),
        Some("https://acme.sso.copilot.qq.com")
    );
    assert_eq!(quota::base_url(None), None);
    assert_eq!(quota::base_url(Some("evil.example.com")), None);
    assert_eq!(quota::base_url(Some("codebuddy.cn.evil.com")), None);
    assert_eq!(quota::base_url(Some("evilcodebuddy.cn")), None);
    assert_eq!(quota::base_url(Some("www.codebuddy.cn/x")), None);
    assert_eq!(quota::base_url(Some("www.codebuddy.cn:8443")), None);
}

#[test]
fn quota_parses_personal_summary() {
    let body = br#"{"code":0,"msg":"OK","data":{
        "Packages":[
            {"PackageCode":"a","CycleTotalCapacity":"100","CycleRemainCapacity":"60.5","CycleUsedCapacity":"39.5"},
            {"PackageCode":"b","CycleTotalCapacity":50,"CycleRemainCapacity":"50","CycleUsedCapacity":"0"}
        ],
        "SubscriptionPackageName":" Plan ","IsPaidUser":true}}"#;
    let quota::ParseOutcome::Ok(credits) = quota::parse_personal(body) else {
        panic!("should parse");
    };
    assert_eq!(credits.plan_name.as_deref(), Some("Plan"));
    assert!(credits.is_paid);
    assert!(!credits.enterprise);
    assert_eq!(credits.total, 150.0);
    assert_eq!(credits.remaining, 110.5);
    assert_eq!(credits.used, 39.5);

    let empty = br#"{"code":0,"data":{"SubscriptionPackageName":""}}"#;
    let quota::ParseOutcome::Ok(credits) = quota::parse_personal(empty) else {
        panic!("should parse");
    };
    assert_eq!(credits.plan_name, None);
    assert_eq!(credits.total, 0.0);
}

#[test]
fn quota_reports_business_errors() {
    assert_eq!(
        quota::parse_personal(br#"{"code":10085,"msg":"bad request"}"#),
        quota::ParseOutcome::Rejected("bad request".into())
    );
    assert_eq!(
        quota::parse_personal(br#"{"code":0}"#),
        quota::ParseOutcome::Rejected("CodeBuddy 没有返回额度".into())
    );
    assert!(matches!(
        quota::parse_personal(b"<html>"),
        quota::ParseOutcome::Rejected(_)
    ));
}

#[test]
fn quota_parses_enterprise_usage() {
    let quota::ParseOutcome::Ok(credits) =
        quota::parse_enterprise(br#"{"code":0,"data":{"limit_num":1000,"used_num":250}}"#)
    else {
        panic!("should parse");
    };
    assert!(credits.enterprise && !credits.unlimited);
    assert_eq!(
        (credits.total, credits.remaining, credits.used),
        (1000.0, 750.0, 250.0)
    );

    let quota::ParseOutcome::Ok(credits) =
        quota::parse_enterprise(br#"{"code":0,"data":{"limitNum":-1,"credit":12}}"#)
    else {
        panic!("should parse");
    };
    assert!(credits.unlimited);
    assert_eq!((credits.total, credits.used), (0.0, 12.0));
}

#[test]
fn quota_credentials_and_expiry() {
    let raw = r#"{"auth":{"accessToken":"t","domain":"www.codebuddy.cn","expiresAt":2000,"refreshExpiresAt":5000},
        "account":{"uid":"u1","enterpriseId":" "}}"#;
    let creds = quota::credentials_of(raw).unwrap();
    assert_eq!(creds.uid, "u1");
    assert_eq!(creds.enterprise_id, None);
    assert!(!format!("{creds:?}").contains("\"t\""));
    // 小于 1e12 的时间戳按秒算：access 2_000_000 ms，refresh 5_000_000 ms
    assert!(quota::expired_status(&creds, 1_000_000).is_none());
    assert!(matches!(
        quota::expired_status(&creds, 3_000_000),
        Some(crate::services::subscription::CredentialStatus::RefreshPending)
    ));
    let creds = quota::QuotaCredentials {
        access_expires_at: Some(1_000_000),
        refresh_expires_at: Some(5_000_000),
        ..creds
    };
    assert!(quota::expired_status(&creds, 500_000).is_none());
    assert!(matches!(
        quota::expired_status(&creds, 2_000_000),
        Some(crate::services::subscription::CredentialStatus::RefreshPending)
    ));
    assert!(matches!(
        quota::expired_status(&creds, 6_000_000),
        Some(crate::services::subscription::CredentialStatus::Expired)
    ));
    assert!(quota::credentials_of(r#"{"auth":{},"account":{"uid":"1"}}"#).is_none());
}

#[tokio::test]
async fn quota_does_not_query_with_expired_tokens() {
    let fx = Fixture::new();
    let mut value: Value = serde_json::from_str(&credential("100", CN, "a1", "s1")).unwrap();
    value["auth"]["expiresAt"] = json!(1_000);
    let raw = serde_json::to_string(&value).unwrap();
    fx.set_live(&raw);
    let account = fx.manager.import_current().await.unwrap();
    let quota = fx.manager.quota(&account.id, "zh-CN", false).await.unwrap();
    assert!(!quota.success);
    assert!(matches!(
        quota.credential_status,
        crate::services::subscription::CredentialStatus::RefreshPending
    ));
    // 没查到的不进缓存，文件没被动过
    assert!(fx.manager.quota_cache.lock().await.is_empty());
    assert_eq!(fx.live(), raw);
    assert!(fx.manager.quota("nope@x", "en", false).await.is_err());
}
