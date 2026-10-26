use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::json;

use super::store::memory::MemoryStore;
use super::*;

/// 做完、等调用方确认的切换日志。
fn done(data: &Path) -> Option<Journal> {
    load_state(data)
        .journal
        .filter(|journal| journal.step == Step::Done)
}

/// 有没有没做完的切换。
fn unfinished(data: &Path) -> bool {
    load_state(data)
        .journal
        .is_some_and(|journal| journal.step != Step::Done)
}

struct Fixture {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    data: PathBuf,
    store: MemoryStore,
    accounts: FakeAccounts,
    adopter: FakeAdopter,
}

#[derive(Default)]
struct FakeAccounts(RefCell<HashMap<String, PathBuf>>);

impl Accounts for FakeAccounts {
    fn dir_of(&self, account: &str) -> Option<PathBuf> {
        self.0.borrow().get(account).cloned()
    }
}

struct FakeAdopter {
    root: PathBuf,
    adopted: RefCell<Vec<String>>,
}

impl Adopter for FakeAdopter {
    fn adopt(&self, found: &identity::Identity, profile: &Value) -> Result<PathBuf, AppError> {
        let dir = self
            .root
            .join(format!("adopted-{}", self.adopted.borrow().len()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(".claude.json"),
            serde_json::to_vec(&json!({ "oauthAccount": profile })).unwrap(),
        )
        .unwrap();
        self.adopted.borrow_mut().push(found.key.clone());
        Ok(dir)
    }
}

fn secret(org: &str, refresh: &str) -> Secret {
    let Value::Object(map) = json!({
        "claudeAiOauth": {"accessToken": format!("at-{refresh}"), "refreshToken": refresh, "expiresAt": 1},
        "organizationUuid": org,
        "mcpOAuth": {"server": "kept-per-slot"},
    }) else {
        unreachable!()
    };
    Secret(map)
}

fn profile(account: &str, org: &str) -> Value {
    json!({"accountUuid": account, "organizationUuid": org, "emailAddress": format!("{account}@x.com")})
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let data = tmp.path().join("data");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let adopter = FakeAdopter {
            root: tmp.path().join("accounts"),
            adopted: RefCell::default(),
        };
        Fixture {
            home,
            data,
            store: MemoryStore::default(),
            accounts: FakeAccounts::default(),
            adopter,
            _tmp: tmp,
        }
    }

    fn default(&self) -> Slot {
        Slot::Default {
            home: self.home.clone(),
        }
    }

    fn ctx(&self) -> Ctx<'_> {
        Ctx {
            data_dir: &self.data,
            default: self.default(),
            store: &self.store,
            accounts: &self.accounts,
            adopter: &self.adopter,
        }
    }

    /// 一个已收录的账号目录，里面登着 `account`。
    fn account(&self, account: &str, org: &str, refresh: &str) -> (String, PathBuf) {
        let dir = self._tmp.path().join(format!("acct-{account}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(".claude.json"),
            serde_json::to_vec(&json!({"oauthAccount": profile(account, org)})).unwrap(),
        )
        .unwrap();
        self.store
            .put(&Slot::Account(dir.clone()), secret(org, refresh));
        let key = identity::account_key(account, Some(org));
        self.accounts
            .0
            .borrow_mut()
            .insert(key.clone(), dir.clone());
        (key, dir)
    }

    /// 默认槽位登着 `account`。
    fn sign_in_default(&self, account: &str, org: &str, refresh: &str) {
        self.store.put(&self.default(), secret(org, refresh));
        std::fs::write(
            self.default().global_config(),
            serde_json::to_vec(&json!({
                "oauthAccount": profile(account, org),
                "mcpServers": {"a": {}},
                "cachedUsageUtilization": {"x": 1},
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn default_profile(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.default().global_config()).unwrap()).unwrap()
    }

    fn refresh_of(&self, slot: &Slot) -> Option<String> {
        self.store
            .get(slot)?
            .0
            .get("claudeAiOauth")?
            .get("refreshToken")?
            .as_str()
            .map(str::to_string)
    }
}

#[test]
fn first_switch_adopts_the_default_login_and_moves_the_target_in() {
    let fx = Fixture::new();
    fx.sign_in_default("me", "org-me", "rt-me");
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");

    let outcome = check_out(&fx.ctx(), &work, &work_dir).unwrap();
    let adopted_dir = fx.adopter.root.join("adopted-0");
    assert_eq!(
        outcome,
        Outcome::Switched {
            adopted: Some("me:org-me".into()),
            previous: Some(("me:org-me".into(), adopted_dir.clone())),
        }
    );

    // 默认槽位换成了 work 的登录，共享键留下
    let default = fx.store.get(&fx.default()).unwrap();
    assert_eq!(fx.refresh_of(&fx.default()).as_deref(), Some("rt-work"));
    assert_eq!(default.0["organizationUuid"], "org-work");
    assert_eq!(default.0["mcpOAuth"], json!({"server": "kept-per-slot"}));
    // work 的账号槽位去掉账号键（同一个号只留一份），它自己的 MCP 授权留着
    let left = fx.store.get(&Slot::Account(work_dir)).unwrap();
    assert!(left.0.get("claudeAiOauth").is_none());
    assert_eq!(left.0["mcpOAuth"], json!({"server": "kept-per-slot"}));
    // 原来的号收进了新账号目录
    assert_eq!(
        fx.refresh_of(&Slot::Account(adopted_dir)).as_deref(),
        Some("rt-me")
    );
    // 资料换成 work 的，账号缓存删掉，其他键不碰
    let doc = fx.default_profile();
    assert_eq!(doc["oauthAccount"]["accountUuid"], "work");
    assert!(doc.get("cachedUsageUtilization").is_none());
    assert_eq!(doc["mcpServers"], json!({"a": {}}));
    assert_eq!(
        done(&fx.data).map(|journal| journal.target),
        Some(work.clone())
    );
    assert_eq!(default_owner(&fx.default()).as_deref(), Some(work.as_str()));
}

#[test]
fn switching_back_parks_the_live_account_in_its_own_slot() {
    let fx = Fixture::new();
    fx.sign_in_default("me", "org-me", "rt-me");
    let (me, me_dir) = fx.account("me", "org-me", "rt-me-old");
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");

    check_out(&fx.ctx(), &work, &work_dir).unwrap();
    // 默认槽位里 me 的那份（更新的）覆盖了它账号槽位里的旧登录
    assert_eq!(
        fx.refresh_of(&Slot::Account(me_dir.clone())).as_deref(),
        Some("rt-me")
    );
    // work 在用期间被 Claude Code 刷新过
    let mut rotated = fx.store.get(&fx.default()).unwrap();
    rotated.0["claudeAiOauth"]["refreshToken"] = json!("rt-work-2");
    fx.store.put(&fx.default(), rotated);

    let outcome = check_out(&fx.ctx(), &me, &me_dir).unwrap();
    assert_eq!(
        outcome,
        Outcome::Switched {
            adopted: None,
            previous: Some((work.clone(), work_dir.clone())),
        }
    );
    assert_eq!(fx.refresh_of(&fx.default()).as_deref(), Some("rt-me"));
    // 收回的是刷新后的那份
    assert_eq!(
        fx.refresh_of(&Slot::Account(work_dir)).as_deref(),
        Some("rt-work-2")
    );
    assert!(fx.refresh_of(&Slot::Account(me_dir.clone())).is_none());
    assert!(fx.adopter.adopted.borrow().is_empty());
    // 第一次收回时 me 槽位里那份另外的登录没被覆盖掉，留在备份里（只有账号键）
    let backup = fx.store.get(&Slot::Account(backup_dir(&me_dir))).unwrap();
    assert_eq!(
        backup.0["claudeAiOauth"]["refreshToken"],
        json!("rt-me-old")
    );
    assert!(backup.0.get("mcpOAuth").is_none());
}

fn with_expiry(mut secret: Secret, at: i64) -> Secret {
    secret.0.get_mut("claudeAiOauth").unwrap()["refreshTokenExpiresAt"] = json!(at);
    secret
}

#[test]
fn parking_the_same_login_keeps_no_backup() {
    let fx = Fixture::new();
    fx.sign_in_default("me", "org-me", "rt-me");
    let (_, me_dir) = fx.account("me", "org-me", "rt-me");
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");
    check_out(&fx.ctx(), &work, &work_dir).unwrap();
    assert!(fx.store.get(&Slot::Account(backup_dir(&me_dir))).is_none());
}

#[test]
fn signed_out_target_falls_back_to_its_backup() {
    let fx = Fixture::new();
    fx.sign_in_default("me", "org-me", "rt-me");
    let (work, work_dir) = fx.account("work", "org-work", "rt-work-old");
    fx.store.delete(&Slot::Account(work_dir.clone())).unwrap();
    let backup = Slot::Account(backup_dir(&work_dir));
    fx.store
        .put(&backup, merge(None, &secret("org-work", "rt-work-old")));

    check_out(&fx.ctx(), &work, &work_dir).unwrap();
    assert_eq!(fx.refresh_of(&fx.default()).as_deref(), Some("rt-work-old"));
    // 移进默认槽位后备份就不留了，同一份令牌只有一处
    assert!(fx.store.get(&backup).is_none());
    // 默认槽位的共享键还是原来的
    assert_eq!(
        fx.store.get(&fx.default()).unwrap().0["mcpOAuth"],
        json!({"server": "kept-per-slot"})
    );
}

#[test]
fn expired_target_is_refused_without_changes() {
    let fx = Fixture::new();
    fx.sign_in_default("me", "org-me", "rt-me");
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");
    let past = chrono::Utc::now().timestamp_millis() - 1000;
    fx.store.put(
        &Slot::Account(work_dir.clone()),
        with_expiry(secret("org-work", "rt-work"), past),
    );
    assert!(check_out(&fx.ctx(), &work, &work_dir).is_err());
    assert_eq!(fx.refresh_of(&fx.default()).as_deref(), Some("rt-me"));
    assert!(fx.adopter.adopted.borrow().is_empty());
}

#[test]
fn login_expiry_is_noted_when_parked_and_dropped_when_live() {
    let fx = Fixture::new();
    let later = chrono::Utc::now().timestamp_millis() + 86_400_000;
    fx.sign_in_default("me", "org-me", "rt-me");
    fx.store
        .put(&fx.default(), with_expiry(secret("org-me", "rt-me"), later));
    let (me, me_dir) = fx.account("me", "org-me", "rt-me-old");
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");

    check_out(&fx.ctx(), &work, &work_dir).unwrap();
    assert_eq!(load_state(&fx.data).login_expiry.get(&me), Some(&later));

    check_out(&fx.ctx(), &me, &me_dir).unwrap();
    let state = load_state(&fx.data);
    assert!(!state.login_expiry.contains_key(&me));
    // work 的凭据里没有这个字段：不记
    assert!(!state.login_expiry.contains_key(&work));
}

#[test]
fn login_expiry_accepts_seconds_and_milliseconds() {
    let ms = with_expiry(secret("o", "r"), 1_900_000_000_000);
    assert_eq!(login_expiry(&ms), Some(1_900_000_000_000));
    let secs = with_expiry(secret("o", "r"), 1_900_000_000);
    assert_eq!(login_expiry(&secs), Some(1_900_000_000_000));
    assert_eq!(login_expiry(&secret("o", "r")), None);
}

#[test]
fn switching_to_the_live_account_changes_nothing() {
    let fx = Fixture::new();
    fx.sign_in_default("me", "org-me", "rt-me");
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");
    check_out(&fx.ctx(), &work, &work_dir).unwrap();
    let before = fx.store.get(&fx.default()).unwrap();
    assert_eq!(
        check_out(&fx.ctx(), &work, &work_dir).unwrap(),
        Outcome::AlreadyLive
    );
    assert_eq!(fx.store.get(&fx.default()).unwrap(), before);
}

#[test]
fn signed_out_target_is_refused_without_changes() {
    let fx = Fixture::new();
    fx.sign_in_default("me", "org-me", "rt-me");
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");
    fx.store.delete(&Slot::Account(work_dir.clone())).unwrap();
    assert!(check_out(&fx.ctx(), &work, &work_dir).is_err());
    assert_eq!(fx.refresh_of(&fx.default()).as_deref(), Some("rt-me"));
    assert!(fx.adopter.adopted.borrow().is_empty());
}

#[test]
fn unknown_or_mixed_default_login_is_refused() {
    let fx = Fixture::new();
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");
    // 有凭据、资料里的组织和凭据对不上
    fx.sign_in_default("me", "org-me", "rt-me");
    fx.store.put(&fx.default(), secret("org-other", "rt-me"));
    assert!(check_out(&fx.ctx(), &work, &work_dir).is_err());
    assert_eq!(fx.refresh_of(&fx.default()).as_deref(), Some("rt-me"));
    assert_eq!(
        fx.refresh_of(&Slot::Account(work_dir)).as_deref(),
        Some("rt-work")
    );
}

#[test]
fn empty_default_slot_just_receives_the_target() {
    let fx = Fixture::new();
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");
    let outcome = check_out(&fx.ctx(), &work, &work_dir).unwrap();
    assert_eq!(
        outcome,
        Outcome::Switched {
            adopted: None,
            previous: None,
        }
    );
    assert_eq!(fx.refresh_of(&fx.default()).as_deref(), Some("rt-work"));
    assert_eq!(fx.default_profile()["oauthAccount"]["accountUuid"], "work");
}

/// 第二次读默认槽位时（拿到写锁之后）内容变了：模拟 Claude Code 在我们读完之后写了一次。
struct ChangingStore {
    inner: MemoryStore,
    reads: AtomicUsize,
    default: Slot,
}

impl CredentialStore for ChangingStore {
    fn read(&self, slot: &Slot) -> Result<Option<Secret>, AppError> {
        if *slot == self.default && self.reads.fetch_add(1, Ordering::SeqCst) == 1 {
            let mut changed = self.inner.get(slot).unwrap();
            changed
                .0
                .insert("mcpOAuth".into(), json!({"server": "new"}));
            self.inner.put(slot, changed);
        }
        self.inner.read(slot)
    }
    fn write(&self, slot: &Slot, secret: &Secret) -> Result<(), AppError> {
        self.inner.write(slot, secret)
    }
    fn delete(&self, slot: &Slot) -> Result<(), AppError> {
        self.inner.delete(slot)
    }
}

#[test]
fn change_after_reading_cancels_the_switch() {
    let fx = Fixture::new();
    fx.sign_in_default("me", "org-me", "rt-me");
    let (me, me_dir) = fx.account("me", "org-me", "rt-me");
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");
    let changing = ChangingStore {
        inner: MemoryStore::default(),
        reads: AtomicUsize::new(0),
        default: fx.default(),
    };
    for slot in [
        fx.default(),
        Slot::Account(me_dir.clone()),
        Slot::Account(work_dir.clone()),
    ] {
        changing.inner.put(&slot, fx.store.get(&slot).unwrap());
    }
    // 上一次切换做完了、还没被确认：这次取消后它要原样留着，给启动时核对卡片
    let prior = Journal {
        target: me.clone(),
        target_dir: me_dir.clone(),
        target_org: Some("org-me".into()),
        target_account: "fp".into(),
        before_account: None,
        before_owner: None,
        step: Step::Done,
    };
    update_state(&fx.data, |state| state.journal = Some(prior.clone())).unwrap();
    let ctx = Ctx {
        store: &changing,
        ..fx.ctx()
    };
    assert!(check_out(&ctx, &work, &work_dir).is_err());
    assert_eq!(done(&fx.data), Some(prior));
    // Claude Code 的改动保留，没有写入 work
    let default = changing.inner.get(&fx.default()).unwrap();
    assert_eq!(default.0["mcpOAuth"], json!({"server": "new"}));
    assert_eq!(default.0["organizationUuid"], "org-me");
    assert!(changing.inner.get(&Slot::Account(work_dir)).is_some());
    assert!(!unfinished(&fx.data));
}

#[test]
fn recovery_finishes_a_committed_switch() {
    let fx = Fixture::new();
    fx.sign_in_default("me", "org-me", "rt-me");
    let (_me, _me_dir) = fx.account("me", "org-me", "rt-me");
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");
    let target = fx.store.get(&Slot::Account(work_dir.clone())).unwrap();
    // 崩在提交点之后：默认槽位已经是 work，资料和账号槽位还没处理
    let planned = merge(fx.store.get(&fx.default()).as_ref(), &target);
    fx.store.put(&fx.default(), planned);
    save_state(
        &fx.data,
        &State {
            journal: Some(Journal {
                target: work.clone(),
                target_dir: work_dir.clone(),
                target_org: Some("org-work".into()),
                target_account: account_fingerprint(&target),
                before_account: Some(account_fingerprint(&secret("org-me", "rt-me"))),
                before_owner: Some(identity::account_key("me", Some("org-me"))),
                step: Step::Parked,
            }),
            ..Default::default()
        },
    )
    .unwrap();

    recover(&fx.ctx()).unwrap();
    assert_eq!(fx.default_profile()["oauthAccount"]["accountUuid"], "work");
    assert!(fx.refresh_of(&Slot::Account(work_dir)).is_none());
    assert_eq!(done(&fx.data).map(|journal| journal.target), Some(work));
}

#[test]
fn recovery_drops_a_switch_that_never_committed() {
    let fx = Fixture::new();
    fx.sign_in_default("me", "org-me", "rt-me");
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");
    let target = fx.store.get(&Slot::Account(work_dir.clone())).unwrap();
    save_state(
        &fx.data,
        &State {
            journal: Some(Journal {
                target: work,
                target_dir: work_dir.clone(),
                target_org: Some("org-work".into()),
                target_account: account_fingerprint(&target),
                before_account: Some(account_fingerprint(&secret("org-me", "rt-me"))),
                before_owner: Some(identity::account_key("me", Some("org-me"))),
                step: Step::Parked,
            }),
            ..Default::default()
        },
    )
    .unwrap();

    recover(&fx.ctx()).unwrap();
    assert_eq!(fx.refresh_of(&fx.default()).as_deref(), Some("rt-me"));
    assert_eq!(fx.default_profile()["oauthAccount"]["accountUuid"], "me");
    assert!(fx.store.get(&Slot::Account(work_dir)).is_some());
    assert!(!unfinished(&fx.data));
}

#[test]
fn recovery_leaves_a_later_login_alone() {
    // 提交后崩溃，之后用户在 Claude Code 里登了另一个号：不能把资料改成 work、也不能动 work 的槽位
    let fx = Fixture::new();
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");
    let target = fx.store.get(&Slot::Account(work_dir.clone())).unwrap();
    fx.sign_in_default("other", "org-other", "rt-other");
    save_state(
        &fx.data,
        &State {
            journal: Some(Journal {
                target: work,
                target_dir: work_dir.clone(),
                target_org: Some("org-work".into()),
                target_account: account_fingerprint(&target),
                before_account: None,
                before_owner: None,
                step: Step::Committed,
            }),
            ..Default::default()
        },
    )
    .unwrap();

    recover(&fx.ctx()).unwrap();
    assert_eq!(fx.default_profile()["oauthAccount"]["accountUuid"], "other");
    assert_eq!(
        fx.refresh_of(&Slot::Account(work_dir)).as_deref(),
        Some("rt-work")
    );
    assert!(!unfinished(&fx.data));
}

#[test]
fn recovery_finishes_a_committed_switch_after_a_refresh() {
    // 提交后崩溃，Claude Code 又刷新过（指纹变了、组织不变）：仍算 work，补完
    let fx = Fixture::new();
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");
    let target = fx.store.get(&Slot::Account(work_dir.clone())).unwrap();
    fx.sign_in_default("me", "org-me", "rt-me");
    fx.store.put(&fx.default(), secret("org-work", "rt-work-2"));
    save_state(
        &fx.data,
        &State {
            journal: Some(Journal {
                target: work.clone(),
                target_dir: work_dir.clone(),
                target_org: Some("org-work".into()),
                target_account: account_fingerprint(&target),
                before_account: None,
                before_owner: Some(identity::account_key("me", Some("org-me"))),
                step: Step::Committed,
            }),
            ..Default::default()
        },
    )
    .unwrap();

    recover(&fx.ctx()).unwrap();
    // 补完的切换留给启动流程核对卡片
    let recovered = done(&fx.data).unwrap();
    assert_eq!(recovered.target, work);
    assert_eq!(
        recovered.before_owner.as_deref(),
        Some(identity::account_key("me", Some("org-me")).as_str())
    );
    assert_eq!(fx.default_profile()["oauthAccount"]["accountUuid"], "work");
    // 槽位里那份和移走的是同一份：去掉账号键
    assert!(fx.refresh_of(&Slot::Account(work_dir)).is_none());
}

#[test]
fn recovery_leaves_a_later_login_in_the_same_org_alone() {
    // 提交后崩溃，用户之后在 Claude Code 里登了同一个 Team 组织的另一个号：组织一样，但资料
    // 是那个号的，不能当成 work 被刷新过
    let fx = Fixture::new();
    let (work, work_dir) = fx.account("work", "org-team", "rt-work");
    let target = fx.store.get(&Slot::Account(work_dir.clone())).unwrap();
    fx.sign_in_default("mate", "org-team", "rt-mate");
    save_state(
        &fx.data,
        &State {
            journal: Some(Journal {
                target: work,
                target_dir: work_dir.clone(),
                target_org: Some("org-team".into()),
                target_account: account_fingerprint(&target),
                before_account: None,
                before_owner: Some(identity::account_key("me", Some("org-me"))),
                step: Step::Committed,
            }),
            ..Default::default()
        },
    )
    .unwrap();

    recover(&fx.ctx()).unwrap();
    assert!(done(&fx.data).is_none());
    assert_eq!(fx.default_profile()["oauthAccount"]["accountUuid"], "mate");
    assert_eq!(
        fx.refresh_of(&Slot::Account(work_dir)).as_deref(),
        Some("rt-work")
    );
    assert!(!unfinished(&fx.data));
}

/// 读某一个槽位时报错，其余照常（模拟提交点之后钥匙串一时读不了）。
struct FailOn<'a> {
    inner: &'a MemoryStore,
    slot: Slot,
}

impl CredentialStore for FailOn<'_> {
    fn read(&self, slot: &Slot) -> Result<Option<Secret>, AppError> {
        if *slot == self.slot {
            return Err(AppError::Message("injected".into()));
        }
        self.inner.read(slot)
    }
    fn write(&self, slot: &Slot, secret: &Secret) -> Result<(), AppError> {
        self.inner.write(slot, secret)
    }
    fn delete(&self, slot: &Slot) -> Result<(), AppError> {
        self.inner.delete(slot)
    }
}

#[test]
fn failure_after_the_commit_still_counts_as_switched() {
    // 默认槽位已经写成 work 之后收尾失败：切换照样算成（调用方要去更新卡片），留着日志
    // 之后补完
    let fx = Fixture::new();
    fx.sign_in_default("me", "org-me", "rt-me");
    let (me, me_dir) = fx.account("me", "org-me", "rt-me");
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");
    let failing = FailOn {
        inner: &fx.store,
        slot: Slot::Account(backup_dir(&work_dir)),
    };
    let ctx = Ctx {
        store: &failing,
        ..fx.ctx()
    };

    let outcome = check_out(&ctx, &work, &work_dir).unwrap();
    assert_eq!(
        outcome,
        Outcome::Switched {
            adopted: None,
            previous: Some((me, me_dir)),
        }
    );
    assert_eq!(fx.refresh_of(&fx.default()).as_deref(), Some("rt-work"));
    assert_eq!(
        load_state(&fx.data).journal.map(|journal| journal.step),
        Some(Step::Committed)
    );

    recover(&fx.ctx()).unwrap();
    assert_eq!(fx.default_profile()["oauthAccount"]["accountUuid"], "work");
    assert!(fx.refresh_of(&Slot::Account(work_dir)).is_none());
    assert!(!unfinished(&fx.data));
}

/// 切到 work 之后收尾失败、日志停在 Committed（同 [`failure_after_the_commit_still_counts_as_switched`]）。
fn commit_without_finishing(fx: &Fixture, work: &str, work_dir: &Path) {
    let failing = FailOn {
        inner: &fx.store,
        slot: Slot::Account(backup_dir(work_dir)),
    };
    let ctx = Ctx {
        store: &failing,
        ..fx.ctx()
    };
    check_out(&ctx, work, work_dir).unwrap();
    assert!(unfinished(&fx.data));
}

#[test]
fn recovery_finds_the_target_in_its_new_folder_after_a_relogin() {
    // 崩在提交点之后（资料还没换），之后 work 在授权中心重新登录：旧目录删了，新登录在新目录
    let fx = Fixture::new();
    fx.sign_in_default("me", "org-me", "rt-me");
    fx.account("me", "org-me", "rt-me");
    let (work, old_dir) = fx.account("work", "org-work", "rt-work");
    let target = fx.store.get(&Slot::Account(old_dir.clone())).unwrap();
    let planned = merge(fx.store.get(&fx.default()).as_ref(), &target);
    fx.store.put(&fx.default(), planned);
    save_state(
        &fx.data,
        &State {
            journal: Some(Journal {
                target: work.clone(),
                target_dir: old_dir.clone(),
                target_org: Some("org-work".into()),
                target_account: account_fingerprint(&target),
                before_account: Some(account_fingerprint(&secret("org-me", "rt-me"))),
                before_owner: Some(identity::account_key("me", Some("org-me"))),
                step: Step::Committed,
            }),
            ..Default::default()
        },
    )
    .unwrap();
    std::fs::remove_dir_all(&old_dir).unwrap();
    fx.store.delete(&Slot::Account(old_dir)).unwrap();
    let new_dir = fx._tmp.path().join("acct-work-relogin");
    std::fs::create_dir_all(&new_dir).unwrap();
    std::fs::write(
        new_dir.join(".claude.json"),
        serde_json::to_vec(&json!({"oauthAccount": profile("work", "org-work")})).unwrap(),
    )
    .unwrap();
    fx.store.put(
        &Slot::Account(new_dir.clone()),
        secret("org-work", "rt-work-new"),
    );
    fx.accounts
        .0
        .borrow_mut()
        .insert(work.clone(), new_dir.clone());

    recover(&fx.ctx()).unwrap();
    assert_eq!(fx.default_profile()["oauthAccount"]["accountUuid"], "work");
    // 新目录里的是另一次登录，留着
    assert_eq!(
        fx.refresh_of(&Slot::Account(new_dir.clone())).as_deref(),
        Some("rt-work-new")
    );
    assert_eq!(
        done(&fx.data).map(|journal| journal.target_dir),
        Some(new_dir)
    );
}

#[test]
fn recovery_waits_when_the_target_profile_is_gone() {
    let fx = Fixture::new();
    fx.sign_in_default("me", "org-me", "rt-me");
    fx.account("me", "org-me", "rt-me");
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");
    commit_without_finishing(&fx, &work, &work_dir);
    std::fs::remove_file(work_dir.join(".claude.json")).unwrap();
    let before = fx.default_profile();

    recover(&fx.ctx()).unwrap();
    assert_eq!(fx.default_profile(), before);
    assert_eq!(
        fx.refresh_of(&Slot::Account(work_dir)).as_deref(),
        Some("rt-work")
    );
    assert!(unfinished(&fx.data));
}

#[test]
fn parking_brings_back_the_profile_after_a_login_in_its_terminal() {
    // me 在默认槽位里时，从它的终端里 /login 了另一个号：目录里的资料和凭据都是那个号的
    let fx = Fixture::new();
    fx.sign_in_default("me", "org-me", "rt-me");
    let (me, me_dir) = fx.account("me", "org-me", "rt-me");
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");
    std::fs::write(
        me_dir.join(".claude.json"),
        serde_json::to_vec(&json!({"oauthAccount": profile("other", "org-other")})).unwrap(),
    )
    .unwrap();
    fx.store.put(
        &Slot::Account(me_dir.clone()),
        secret("org-other", "rt-other"),
    );

    check_out(&fx.ctx(), &work, &work_dir).unwrap();
    confirm(&fx.data);
    let parked: Value =
        serde_json::from_slice(&std::fs::read(me_dir.join(".claude.json")).unwrap()).unwrap();
    assert_eq!(parked["oauthAccount"]["accountUuid"], "me");
    // 那个号的登录留作备份
    assert_eq!(
        fx.refresh_of(&Slot::Account(backup_dir(&me_dir)))
            .as_deref(),
        Some("rt-other")
    );

    check_out(&fx.ctx(), &me, &me_dir).unwrap();
    assert_eq!(fx.refresh_of(&fx.default()).as_deref(), Some("rt-me"));
    assert_eq!(fx.default_profile()["oauthAccount"]["accountUuid"], "me");
}

#[test]
fn switching_keeps_fields_written_meanwhile() {
    // 登录完成和切换各改各的字段
    let fx = Fixture::new();
    fx.sign_in_default("me", "org-me", "rt-me");
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");
    update_state(&fx.data, |state| {
        state.login_expiry.insert("other".into(), 1);
    })
    .unwrap();
    check_out(&fx.ctx(), &work, &work_dir).unwrap();
    assert_eq!(load_state(&fx.data).login_expiry.get("other"), Some(&1));
    assert_eq!(done(&fx.data).map(|journal| journal.target), Some(work));
}

#[test]
fn secrets_never_print() {
    let text = format!("{:?}", secret("org", "rt-very-secret"));
    assert!(!text.contains("rt-very-secret"));
    assert!(text.contains("claudeAiOauth"));
}

#[test]
fn a_switch_stays_unconfirmed_until_the_caller_confirms() {
    let fx = Fixture::new();
    fx.sign_in_default("me", "org-me", "rt-me");
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");
    check_out(&fx.ctx(), &work, &work_dir).unwrap();
    let unconfirmed = done(&fx.data).unwrap();
    assert_eq!(unconfirmed.target, work);
    assert_eq!(unconfirmed.target_dir, work_dir);
    assert_eq!(
        unconfirmed.before_owner.as_deref(),
        Some(identity::account_key("me", Some("org-me")).as_str())
    );
    confirm(&fx.data);
    assert!(done(&fx.data).is_none());
}

#[test]
fn vacate_moves_the_login_back_out_of_an_empty_default() {
    // 默认槽位原来没有登录，切进去之后写卡片失败：移回去，默认槽位回到没有登录
    let fx = Fixture::new();
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");
    fx.store.put(
        &fx.default(),
        Secret(
            json!({"mcpOAuth": {"server": "machine"}})
                .as_object()
                .unwrap()
                .clone(),
        ),
    );
    let outcome = check_out(&fx.ctx(), &work, &work_dir).unwrap();
    assert_eq!(
        outcome,
        Outcome::Switched {
            adopted: None,
            previous: None,
        }
    );
    assert!(done(&fx.data).unwrap().before_owner.is_none());

    vacate(&fx.ctx(), &work, &work_dir).unwrap();
    let default = fx.store.get(&fx.default()).unwrap();
    assert!(default.0.get("claudeAiOauth").is_none());
    assert_eq!(default.0["mcpOAuth"], json!({"server": "machine"}));
    assert_eq!(
        fx.refresh_of(&Slot::Account(work_dir)).as_deref(),
        Some("rt-work")
    );
    assert!(fx.default_profile().get("oauthAccount").is_none());
    assert!(default_owner(&fx.default()).is_none());
}

#[test]
fn vacate_leaves_a_different_login_alone() {
    let fx = Fixture::new();
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");
    fx.sign_in_default("me", "org-me", "rt-me");
    vacate(&fx.ctx(), &work, &work_dir).unwrap();
    assert_eq!(fx.refresh_of(&fx.default()).as_deref(), Some("rt-me"));
    assert_eq!(fx.default_profile()["oauthAccount"]["accountUuid"], "me");
}

#[test]
fn target_folder_signed_in_to_someone_else_is_refused() {
    // 从 work 的终端里 /login 了另一个号：目录里的资料和凭据都是那个号的
    let fx = Fixture::new();
    fx.sign_in_default("me", "org-me", "rt-me");
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");
    std::fs::write(
        work_dir.join(".claude.json"),
        serde_json::to_vec(&json!({"oauthAccount": profile("other", "org-other")})).unwrap(),
    )
    .unwrap();
    fx.store.put(
        &Slot::Account(work_dir.clone()),
        secret("org-other", "rt-other"),
    );

    assert!(check_out(&fx.ctx(), &work, &work_dir).is_err());
    assert_eq!(fx.refresh_of(&fx.default()).as_deref(), Some("rt-me"));
    assert_eq!(
        fx.refresh_of(&Slot::Account(work_dir)).as_deref(),
        Some("rt-other")
    );
    assert!(!unfinished(&fx.data));
}

#[test]
fn same_account_already_in_the_default_slot_is_left_as_is() {
    // 默认槽位和账号目录各登着同一个号（两次登录）：不换令牌，两份都留着
    let fx = Fixture::new();
    fx.sign_in_default("me", "org-me", "rt-me");
    let (me, me_dir) = fx.account("me", "org-me", "rt-me-2");
    assert_eq!(
        check_out(&fx.ctx(), &me, &me_dir).unwrap(),
        Outcome::AlreadyLive
    );
    assert_eq!(fx.refresh_of(&fx.default()).as_deref(), Some("rt-me"));
    assert_eq!(
        fx.refresh_of(&Slot::Account(me_dir)).as_deref(),
        Some("rt-me-2")
    );
    assert!(done(&fx.data).is_none());
}

#[test]
fn relogin_replaces_the_old_login_in_the_default_slot() {
    // 默认槽位里是 me 的旧登录（过期或被吊销），me 在授权中心重新登录：换上新的，旧的不收回
    let fx = Fixture::new();
    fx.store.put(
        &fx.default(),
        merge(
            Some(&Secret(
                json!({"mcpOAuth": {"server": "machine"}})
                    .as_object()
                    .unwrap()
                    .clone(),
            )),
            &with_expiry(secret("org-me", "rt-me-old"), 1_000),
        ),
    );
    std::fs::write(
        fx.default().global_config(),
        serde_json::to_vec(&json!({"oauthAccount": profile("me", "org-me")})).unwrap(),
    )
    .unwrap();
    let (me, me_dir) = fx.account("me", "org-me", "rt-me-new");

    assert!(replace_live(&fx.ctx(), &me, &me_dir).unwrap());
    let default = fx.store.get(&fx.default()).unwrap();
    assert_eq!(fx.refresh_of(&fx.default()).as_deref(), Some("rt-me-new"));
    assert_eq!(default.0["mcpOAuth"], json!({"server": "machine"}));
    assert!(fx.refresh_of(&Slot::Account(me_dir.clone())).is_none());
    assert!(fx.store.get(&Slot::Account(backup_dir(&me_dir))).is_none());
    assert_eq!(done(&fx.data).map(|journal| journal.target), Some(me));
}

#[test]
fn relogin_of_an_account_not_in_the_default_slot_changes_nothing() {
    let fx = Fixture::new();
    fx.sign_in_default("me", "org-me", "rt-me");
    let (work, work_dir) = fx.account("work", "org-work", "rt-work");
    assert!(!replace_live(&fx.ctx(), &work, &work_dir).unwrap());
    assert_eq!(fx.refresh_of(&fx.default()).as_deref(), Some("rt-me"));
    assert_eq!(
        fx.refresh_of(&Slot::Account(work_dir)).as_deref(),
        Some("rt-work")
    );
    assert!(load_state(&fx.data).journal.is_none());
}

#[test]
fn reads_the_plan_claude_code_keeps_with_the_login() {
    let Value::Object(map) = json!({
        "claudeAiOauth": {
            "accessToken": "at", "refreshToken": "rt", "expiresAt": 1,
            "subscriptionType": "max", "rateLimitTier": "default_claude_max_5x",
        },
    }) else {
        unreachable!()
    };
    let plan = credential_plan(&Secret(map));
    assert_eq!(plan.subscription_type.as_deref(), Some("max"));
    assert_eq!(
        plan.rate_limit_tier.as_deref(),
        Some("default_claude_max_5x")
    );

    // 旧版凭据没有这两个字段；空串也当没有
    let mut bare = secret("org", "rt");
    assert_eq!(credential_plan(&bare), Default::default());
    bare.0["claudeAiOauth"]["subscriptionType"] = json!("");
    assert_eq!(credential_plan(&bare).subscription_type, None);
}
