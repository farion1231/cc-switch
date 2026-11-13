//! WSL 家目录里的切卡：语义和本机 [`super::switch`] 一样，但文件操作在发行版里做。
//!
//! 9P（`\\wsl.localhost`）上的锁和权限不可靠，所以不在 Windows 侧改写
//! `.credentials.json` / `.claude.json`。计划仍由 Rust 按本机的规则算（只动账号键和
//! `oauthAccount`，MCP 等共享键留在各自槽位），然后一条
//! `wsl.exe --exec /bin/sh -c` 在 Linux 里 `flock`、拿 Claude Code 的目录锁、
//! 比对内容、`0600` + 原子改名。令牌走 stdin，不进 argv。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

#[cfg(any(windows, test))]
use super::creds::changed;
use super::creds::{
    account_fingerprint, credential_org, has_login, login_expiry, merge, note_expiry, owner_of,
};
use super::store::Secret;
use super::{update_state, Ctx, Journal, Outcome, Step, SWITCH};
use crate::error::AppError;
use crate::services::claude_account::run_target::{self, WslTarget};

const LOCK_WAIT: u64 = 10;
const REFRESH_STALE: u64 = 60;
const STORAGE_STALE: u64 = 15;
const STORAGE_WAIT: u64 = 10;
const CONFIG_STALE: u64 = 10;
const CONFIG_WAIT: u64 = 8;
#[cfg(windows)]
const OWNER_CACHE_TTL: Duration = Duration::from_secs(1);

/// 在发行版里执行的切换脚本。内容固定，路径和文件字节都在 stdin 里。
const APPLY_SCRIPT: &str = r#"set -efu
umask 077
cc_tmp=$(mktemp -d)
cc_locks=$cc_tmp/locks
cc_puts=$cc_tmp/puts
cc_dels=$cc_tmp/dels
cc_stages=$cc_tmp/stages
: > "$cc_locks"
: > "$cc_puts"
: > "$cc_dels"
: > "$cc_stages"
cc_cr=$(printf '\r')
cc_end=0
cc_release() {
  if [ -f "$cc_stages" ]; then
    while IFS= read -r stage; do
      [ -n "$stage" ] || continue
      rm -f -- "$stage"
    done < "$cc_stages"
  fi
  if [ -f "$cc_locks" ]; then
    while IFS=' ' read -r dir stamp; do
      [ -n "$dir" ] || continue
      if [ -d "$dir" ]; then
        now=$(stat -c %Y -- "$dir" 2>/dev/null || true)
        if [ -n "$now" ] && [ "$now" = "$stamp" ]; then
          rm -rf -- "$dir"
        fi
      fi
    done < "$cc_locks"
  fi
  rm -rf -- "$cc_tmp"
}
trap cc_release EXIT
cc_sha() {
  if [ -f "$1" ]; then
    sha256sum -- "$1" | awk 'NR==1 { print $1 }'
  else
    printf '%s' '-'
  fi
}
cc_acquire() {
  p=$1
  stale=$2
  wait_s=$3
  start=$(date +%s)
  cc_parent_try=0
  while :; do
    if mkdir -- "$p" 2>/dev/null; then
      stamp=$(stat -c %Y -- "$p")
      printf '%s %s\n' "$p" "$stamp" >> "$cc_locks"
      return 0
    fi
    if [ -d "$p" ]; then
      now=$(date +%s)
      mt=$(stat -c %Y -- "$p" 2>/dev/null || printf '%s' "$now")
      if [ $((now - mt)) -ge "$stale" ]; then
        rm -rf -- "$p" 2>/dev/null || true
        continue
      fi
    elif [ "$cc_parent_try" = 0 ]; then
      # 和本机 CcLock 一样：父目录还不存在（备份目录 config.prev 等）就先建好再抢，
      # 不能把「没有这个目录」当成 Claude Code 正拿着锁。
      mkdir -p -- "$(dirname -- "$p")" 2>/dev/null || true
      cc_parent_try=1
      continue
    fi
    now=$(date +%s)
    if [ $((now - start)) -ge "$wait_s" ]; then
      echo "lock busy: $p" >&2
      exit 75
    fi
    sleep 0.1
  done
}
while IFS= read -r line || [ -n "$line" ]; do
  line=${line%"$cc_cr"}
  case $line in
    '') continue ;;
    END) cc_end=1; break ;;
  esac
  op=${line%%|*}
  rest=${line#*|}
  case $op in
    LOCK)
      mkdir -p -- "$(dirname -- "$rest")"
      exec 9>"$rest"
      flock -w 10 9 || { echo "lock busy: $rest" >&2; exit 75; }
      ;;
    ACQUIRE)
      path=${rest%%|*}
      rest=${rest#*|}
      stale=${rest%%|*}
      wait_s=${rest#*|}
      cc_acquire "$path" "$stale" "$wait_s"
      ;;
    ACQUIRE_REAL)
      path=${rest%%|*}
      rest=${rest#*|}
      stale=${rest%%|*}
      wait_s=${rest#*|}
      real=$(readlink -f -- "$path" 2>/dev/null || printf '%s' "$path")
      cc_acquire "${real}.lock" "$stale" "$wait_s"
      ;;
    EXPECT)
      path=${rest%%|*}
      sha=${rest#*|}
      got=$(cc_sha "$path")
      if [ "$got" != "$sha" ]; then
        echo "changed" >&2
        exit 3
      fi
      ;;
    PUT)
      path=${rest%%|*}
      rest=${rest#*|}
      mode=${rest%%|*}
      b64=${rest#*|}
      cc_n=$((${cc_n:-0} + 1))
      printf '%s' "$b64" | base64 -d > "$cc_tmp/b64.$cc_n"
      printf '%s %s %s\n' "$path" "$mode" "$cc_tmp/b64.$cc_n" >> "$cc_puts"
      ;;
    DEL)
      printf '%s\n' "$rest" >> "$cc_dels"
      ;;
    *)
      echo "bad plan" >&2
      exit 2
      ;;
  esac
done
if [ "$cc_end" != 1 ]; then
  echo "bad plan" >&2
  exit 2
fi
while IFS=' ' read -r path mode src; do
  [ -n "$path" ] || continue
  dir=$(dirname -- "$path")
  mkdir -p -- "$dir"
  stage="$dir/.cc-switch-new.$$"
  printf '%s\n' "$stage" >> "$cc_stages"
  rm -f -- "$stage"
  cp -- "$src" "$stage"
  chmod "$mode" "$stage"
  mv -f -- "$stage" "$path"
done < "$cc_puts"
while IFS='|' read -r path sha; do
  [ -n "$path" ] || continue
  got=$(cc_sha "$path")
  if [ "$got" != "$sha" ]; then
    echo "changed" >&2
    exit 3
  fi
  rm -f -- "$path"
done < "$cc_dels"
"#;

#[derive(Clone)]
struct WslLayout {
    distro: String,
    #[cfg_attr(not(windows), allow(dead_code))]
    user: String,
    linux_home: String,
}

impl WslLayout {
    fn from_target(wsl: &WslTarget) -> Result<Self, AppError> {
        let user = run_target::wsl_user(&wsl.linux_home).map_err(|error| {
            AppError::localized(
                "claude.account.wsl_user",
                format!("无法从 WSL 家目录得到用户名: {error}"),
                format!("Couldn't get the WSL user from the home directory: {error}"),
            )
        })?;
        let layout = Self {
            distro: wsl.distro.clone(),
            user,
            linux_home: wsl.linux_home.clone(),
        };
        check_path(&layout.linux_home)?;
        Ok(layout)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Switch,
    Replace,
    Vacate,
    Recover,
}

struct Job<'a> {
    ctx: &'a Ctx<'a>,
    kind: Kind,
    layout: &'a WslLayout,
    target: &'a str,
    linux_dir: &'a str,
    journal_dir: &'a Path,
    now_ms: i64,
}

trait Runner {
    fn read(&mut self, paths: &[String]) -> Result<Vec<Option<Vec<u8>>>, AppError>;
    fn apply(&mut self, script: &str, payload: &[u8]) -> Result<(), AppError>;
}

#[derive(Default, Clone)]
struct Snap {
    files: BTreeMap<String, Vec<u8>>,
}

impl Snap {
    fn absorb(&mut self, paths: &[String], got: Vec<Option<Vec<u8>>>) {
        for (path, bytes) in paths.iter().zip(got) {
            match bytes {
                Some(bytes) => {
                    self.files.insert(path.clone(), bytes);
                }
                None => {
                    self.files.remove(path);
                }
            }
        }
    }

    fn raw(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    fn secret(&self, path: &str) -> Result<Option<Secret>, AppError> {
        match self.raw(path) {
            None => Ok(None),
            Some(bytes) if bytes.iter().all(u8::is_ascii_whitespace) => Ok(None),
            Some(bytes) => Secret::parse(bytes).map(Some),
        }
    }

    fn profile(&self, path: &str) -> Option<Value> {
        serde_json::from_slice(self.raw(path)?).ok()
    }

    fn sha(&self, path: &str) -> String {
        match self.files.get(path) {
            Some(bytes) => sha256_hex(bytes),
            None => "-".to_string(),
        }
    }
}

struct SlotPaths {
    secure: String,
    cred: String,
    profile: String,
}

struct Edits {
    /// 先写入、后删除。插入顺序就是写入顺序：先收回旧登录，再写默认槽位。
    writes: Vec<(String, Option<Vec<u8>>)>,
    /// 要拿刷新锁的目录（`.claude` 或账号 `config`）。
    refresh_dirs: BTreeSet<String>,
    /// 要拿 `.storage-write.lock` 的目录。
    cred_dirs: BTreeSet<String>,
    /// 要拿配置锁的资料文件。
    profiles: BTreeSet<String>,
}

impl Edits {
    fn new() -> Self {
        Self {
            writes: Vec::new(),
            refresh_dirs: BTreeSet::new(),
            cred_dirs: BTreeSet::new(),
            profiles: BTreeSet::new(),
        }
    }

    fn watch(&mut self, secure: &str) {
        self.refresh_dirs.insert(secure.to_string());
    }

    fn put_cred(&mut self, path: &str, bytes: Vec<u8>) {
        self.touch_cred(path);
        self.set(path, Some(bytes));
    }

    fn del_cred(&mut self, path: &str) {
        self.touch_cred(path);
        self.set(path, None);
    }

    fn put_profile(&mut self, path: &str, bytes: Vec<u8>) {
        self.profiles.insert(path.to_string());
        self.set(path, Some(bytes));
    }

    fn set(&mut self, path: &str, bytes: Option<Vec<u8>>) {
        if let Some(found) = self
            .writes
            .iter_mut()
            .find(|(existing, _)| existing == path)
        {
            found.1 = bytes;
        } else {
            self.writes.push((path.to_string(), bytes));
        }
    }

    fn touch_cred(&mut self, path: &str) {
        if let Some(dir) = cred_dir(path) {
            self.refresh_dirs.insert(dir.clone());
            self.cred_dirs.insert(dir);
        }
    }

    fn is_empty(&self) -> bool {
        self.writes.is_empty()
    }
}

struct Planned {
    outcome: Outcome,
    edits: Edits,
    journal: Option<Journal>,
    expiry: Vec<(String, Secret)>,
    drop_expiry: Vec<String>,
}

pub(super) fn dispatch_check_out(
    ctx: &Ctx<'_>,
    target: &str,
    dir: &Path,
) -> Option<Result<Outcome, AppError>> {
    dispatch(ctx, Kind::Switch, target, dir)
}

pub(super) fn dispatch_replace(
    ctx: &Ctx<'_>,
    target: &str,
    dir: &Path,
) -> Option<Result<bool, AppError>> {
    Some(
        dispatch(ctx, Kind::Replace, target, dir)?
            .map(|outcome| matches!(outcome, Outcome::Switched { .. })),
    )
}

pub(super) fn dispatch_vacate(
    ctx: &Ctx<'_>,
    target: &str,
    dir: &Path,
) -> Option<Result<(), AppError>> {
    Some(dispatch(ctx, Kind::Vacate, target, dir)?.map(|_| ()))
}

pub(super) fn dispatch_recover(ctx: &Ctx<'_>) -> Option<Result<(), AppError>> {
    Some(dispatch(ctx, Kind::Recover, "", Path::new("."))?.map(|_| ()))
}

fn dispatch(
    ctx: &Ctx<'_>,
    kind: Kind,
    target: &str,
    dir: &Path,
) -> Option<Result<Outcome, AppError>> {
    let wsl = run_target::current_wsl()?;
    Some(run_on_host(ctx, &wsl, kind, target, dir))
}

fn run_on_host(
    ctx: &Ctx<'_>,
    wsl: &WslTarget,
    kind: Kind,
    target: &str,
    dir: &Path,
) -> Result<Outcome, AppError> {
    let layout = WslLayout::from_target(wsl)?;
    let linux_dir = if kind == Kind::Recover {
        String::new()
    } else {
        linux_of(&layout, dir)?
    };
    let now_ms = chrono::Utc::now().timestamp_millis();
    let job = Job {
        ctx,
        kind,
        layout: &layout,
        target,
        linux_dir: &linux_dir,
        journal_dir: dir,
        now_ms,
    };
    let mut runner = WslExeRunner { layout: &layout };
    execute(&job, &mut runner)
}

fn execute(job: &Job<'_>, runner: &mut dyn Runner) -> Result<Outcome, AppError> {
    let _serial = SWITCH
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    invalidate_owner_cache();

    let mut snap = Snap::default();
    absorb_read(
        runner,
        &mut snap,
        &default_read_paths(&job.layout.linux_home),
    )?;
    recover_if_needed(job, runner, &mut snap)?;
    if job.kind == Kind::Recover {
        invalidate_owner_cache();
        return Ok(Outcome::AlreadyLive);
    }

    let planned = plan_main(job, runner, &mut snap)?;
    if planned.edits.is_empty() {
        return Ok(planned.outcome);
    }
    if let Some(journal) = &planned.journal {
        update_state(job.ctx.data_dir, |state| {
            state.journal = Some(journal.clone());
        })?;
    }
    let payload = render_payload(job.layout, &snap, &planned.edits);
    // 失败也留下日志。比对失败时什么都没写，下次补完看见默认槽位仍是切换前的登录，
    // 会把日志清掉。中途被杀掉时可能已经写了一部分，丢掉日志就补不完。
    runner.apply(APPLY_SCRIPT, &payload)?;
    update_state(job.ctx.data_dir, |state| {
        for (account, secret) in &planned.expiry {
            note_expiry(state, account, secret);
        }
        for account in &planned.drop_expiry {
            state.login_expiry.remove(account);
        }
        if let Some(journal) = &planned.journal {
            state.journal = Some(Journal {
                step: Step::Done,
                ..journal.clone()
            });
        }
    })?;
    invalidate_owner_cache();
    log::info!("[ClaudeAccount] 已在 WSL 里切换 Claude Code 的登录");
    Ok(planned.outcome)
}

fn recover_if_needed(
    job: &Job<'_>,
    runner: &mut dyn Runner,
    snap: &mut Snap,
) -> Result<(), AppError> {
    let Some(journal) = super::load_state(job.ctx.data_dir)
        .journal
        .filter(|journal| journal.step != Step::Done)
    else {
        return Ok(());
    };
    let linux_target = match linux_of(job.layout, &journal.target_dir) {
        Ok(dir) => dir,
        Err(error) => {
            log::warn!("[ClaudeAccount] 上次的 WSL 切换日志对不上发行版路径，暂不补完: {error}");
            return Ok(());
        }
    };
    absorb_read(runner, snap, &slot_read_paths(&linux_target))?;
    absorb_read(runner, snap, &[cred_path(&backup_linux(&linux_target))])?;
    let default = default_paths(job.layout, snap);
    let target = resolve_slot(snap, &linux_target);
    match plan_recover(snap, &journal, &default, &target, job.now_ms)? {
        RecoverPlan::Keep => Ok(()),
        RecoverPlan::Clear => {
            update_state(job.ctx.data_dir, |state| state.journal = None)?;
            Ok(())
        }
        RecoverPlan::Finish(edits) => {
            let payload = render_payload(job.layout, snap, &edits);
            runner.apply(APPLY_SCRIPT, &payload)?;
            update_state(job.ctx.data_dir, |state| {
                state.login_expiry.remove(&journal.target);
                state.journal = Some(Journal {
                    step: Step::Done,
                    ..journal.clone()
                });
            })?;
            *snap = Snap::default();
            absorb_read(runner, snap, &default_read_paths(&job.layout.linux_home))?;
            Ok(())
        }
    }
}

enum RecoverPlan {
    Keep,
    Clear,
    Finish(Edits),
}

fn plan_recover(
    snap: &Snap,
    journal: &Journal,
    default: &SlotPaths,
    target: &SlotPaths,
    now_ms: i64,
) -> Result<RecoverPlan, AppError> {
    let now = snap.secret(&default.cred)?;
    let now_account = now.as_ref().map(account_fingerprint);
    let owner = snap
        .profile(&default.profile)
        .as_ref()
        .and_then(super::identity::identity_from_profile)
        .map(|found| found.key);
    let refreshed = journal.step == Step::Committed
        && journal.target_org.is_some()
        && now.as_ref().and_then(credential_org) == journal.target_org
        && (owner.as_deref() == Some(journal.target.as_str()) || owner == journal.before_owner);
    if now_account.as_deref() == Some(journal.target_account.as_str()) || refreshed {
        let Some(profile) = snap.profile(&target.profile) else {
            log::warn!("[ClaudeAccount] 上次 WSL 切换没做完，读不到目标账号的资料，暂不补完");
            return Ok(RecoverPlan::Keep);
        };
        let found = super::identity::identity_from_profile(&profile);
        if found.as_ref().map(|item| item.key.as_str()) != Some(journal.target.as_str()) {
            log::warn!("[ClaudeAccount] 上次 WSL 切换没做完，目标目录的资料对不上，暂不补完");
            return Ok(RecoverPlan::Keep);
        }
        let mut edits = Edits::new();
        edits.watch(&default.secure);
        edits.watch(&target.secure);
        finish_into(
            snap,
            &mut edits,
            default,
            target,
            &journal.target_account,
            &profile,
            now_ms,
        )?;
        log::info!("[ClaudeAccount] 补完上次没做完的 WSL 切换");
        return Ok(RecoverPlan::Finish(edits));
    }
    if now_account != journal.before_account {
        log::warn!("[ClaudeAccount] 上次 WSL 切换没做完，默认登录之后又变了，按现状处理");
    }
    Ok(RecoverPlan::Clear)
}

fn plan_main(job: &Job<'_>, runner: &mut dyn Runner, snap: &mut Snap) -> Result<Planned, AppError> {
    let default = default_paths(job.layout, snap);
    let live = snap.secret(&default.cred)?;
    let live_profile = snap.profile(&default.profile);
    let live_owner = owner_of(live.as_ref(), live_profile.as_ref())?;
    let live_is_target = live_owner.as_ref().map(|owner| owner.key.as_str()) == Some(job.target);
    if job.kind != Kind::Vacate && live_is_target != (job.kind == Kind::Replace) {
        return Ok(noop(Outcome::AlreadyLive));
    }

    let mut extra = slot_read_paths(job.linux_dir);
    extra.push(cred_path(&backup_linux(job.linux_dir)));
    let mut park_linux: Option<String> = None;
    let mut park_journal: Option<PathBuf> = None;
    if job.kind == Kind::Switch {
        if let Some(owner) = live_owner.as_ref() {
            if owner.key != job.target {
                if let Some(dir) = job.ctx.accounts.dir_of(&owner.key) {
                    let linux = linux_of(job.layout, &dir)?;
                    extra.extend(slot_read_paths(&linux));
                    extra.push(cred_path(&backup_linux(&linux)));
                    park_journal = Some(dir);
                    park_linux = Some(linux);
                }
            }
        }
    }
    absorb_read(runner, snap, &extra)?;

    let mut adopted = None;
    if job.kind == Kind::Switch {
        if let Some(owner) = live_owner.as_ref() {
            if owner.key != job.target && park_linux.is_none() {
                let profile = live_profile
                    .as_ref()
                    .and_then(|doc| doc.get("oauthAccount"))
                    .cloned()
                    .unwrap_or(Value::Null);
                let dir = job.ctx.adopter.adopt(owner, &profile)?;
                let linux = linux_of(job.layout, &dir)?;
                absorb_read(runner, snap, &slot_read_paths(&linux))?;
                adopted = Some(owner.key.clone());
                park_journal = Some(dir);
                park_linux = Some(linux);
            }
        }
    }

    let default = default_paths(job.layout, snap);
    let target = resolve_slot(snap, job.linux_dir);
    let mut edits = Edits::new();
    edits.watch(&default.secure);
    edits.watch(&target.secure);

    if job.kind == Kind::Vacate {
        let Some(live) = live.as_ref().filter(|secret| has_login(secret)) else {
            return Ok(noop(Outcome::AlreadyLive));
        };
        if live_owner.as_ref().map(|owner| owner.key.as_str()) != Some(job.target) {
            return Ok(noop(Outcome::AlreadyLive));
        }
        park_into(
            snap,
            &mut edits,
            live,
            live_profile.as_ref(),
            job.target,
            &target,
        )?;
        strip_cred(snap, &mut edits, &default.cred, live)?;
        edits.put_profile(
            &default.profile,
            profile_bytes(snap.profile(&default.profile).as_ref(), None)?,
        );
        return Ok(Planned {
            outcome: Outcome::Switched {
                adopted: None,
                previous: None,
            },
            edits,
            journal: None,
            expiry: vec![(job.target.to_string(), live.clone())],
            drop_expiry: Vec::new(),
        });
    }

    let (target_secret, target_from_backup) = read_target_secret(snap, &target, job.now_ms)?;
    let target_profile = snap.profile(&target.profile).ok_or_else(|| {
        AppError::localized(
            "claude.snapshot.target_profile_missing",
            "读不到这个 Claude 账号的资料，请在授权中心重新登录",
            "This Claude account's profile could not be read. Sign in again in the Auth Center.",
        )
    })?;
    match owner_of(Some(&target_secret), Some(&target_profile)) {
        Ok(Some(found)) if found.key == job.target => {}
        _ => {
            return Err(AppError::localized(
                "claude.snapshot.target_mismatch",
                "这个账号的目录里现在登录的不是它（可能在它的终端里登录了别的账号），没有切换。请在授权中心重新登录这个账号",
                "This account's folder is now signed in to a different account (maybe someone signed in to another account in its terminal), so nothing was switched. Sign in to this account again in the Auth Center.",
            ));
        }
    }

    let mut previous = None;
    let mut expiry = Vec::new();
    if job.kind == Kind::Switch {
        if let (Some(live), Some(owner)) = (live.as_ref(), live_owner.as_ref()) {
            if owner.key != job.target {
                let linux = park_linux.ok_or_else(|| {
                    AppError::localized(
                        "claude.snapshot.park_failed",
                        "保存原来的 Claude 登录失败，没有切换",
                        "Saving the previous Claude sign-in failed, so nothing was switched.",
                    )
                })?;
                let slot = resolve_slot(snap, &linux);
                park_into(
                    snap,
                    &mut edits,
                    live,
                    live_profile.as_ref(),
                    &owner.key,
                    &slot,
                )?;
                expiry.push((owner.key.clone(), live.clone()));
                if let Some(dir) = park_journal {
                    previous = Some((owner.key.clone(), dir));
                }
            }
        }
    }

    let journal = Journal {
        target: job.target.to_string(),
        target_dir: job.journal_dir.to_path_buf(),
        target_org: credential_org(&target_secret),
        target_account: account_fingerprint(&target_secret),
        before_account: live.as_ref().map(account_fingerprint),
        before_owner: live_owner.as_ref().map(|owner| owner.key.clone()),
        step: Step::Parked,
    };
    edits.put_cred(
        &default.cred,
        merge(live.as_ref(), &target_secret).to_bytes(),
    );
    // 用的是备份时，提交后删掉那份，避免两份令牌各自刷新。
    if target_from_backup {
        let backup_cred = cred_path(&backup_linux(job.linux_dir));
        if snap
            .secret(&backup_cred)?
            .as_ref()
            .is_some_and(|secret| account_fingerprint(secret) == journal.target_account)
        {
            edits.del_cred(&backup_cred);
        }
    }
    finish_into(
        snap,
        &mut edits,
        &default,
        &target,
        &journal.target_account,
        &target_profile,
        job.now_ms,
    )?;
    Ok(Planned {
        outcome: Outcome::Switched { adopted, previous },
        edits,
        journal: Some(journal),
        expiry,
        drop_expiry: vec![job.target.to_string()],
    })
}

fn noop(outcome: Outcome) -> Planned {
    Planned {
        outcome,
        edits: Edits::new(),
        journal: None,
        expiry: Vec::new(),
        drop_expiry: Vec::new(),
    }
}

fn read_target_secret(
    snap: &Snap,
    target: &SlotPaths,
    now_ms: i64,
) -> Result<(Secret, bool), AppError> {
    if let Some(secret) = usable(snap.secret(&target.cred)?, now_ms) {
        return Ok((secret, false));
    }
    let backup = cred_path(&backup_linux(&target.secure));
    if let Some(secret) = usable(snap.secret(&backup)?, now_ms) {
        return Ok((secret, true));
    }
    Err(AppError::localized(
        "claude.snapshot.target_signed_out",
        "这个 Claude 账号的登录已失效或已过期，请在授权中心重新登录后再切换",
        "This Claude account is signed out or its sign-in has expired. Sign in again in the Auth Center, then switch.",
    ))
}

fn usable(secret: Option<Secret>, now_ms: i64) -> Option<Secret> {
    secret.filter(|secret| has_login(secret) && login_expiry(secret).is_none_or(|at| at > now_ms))
}

/// 把默认槽位里的账号键写回 `slot`。槽位里如果是另一份登录，先挪到备份目录。
fn park_into(
    snap: &Snap,
    edits: &mut Edits,
    live: &Secret,
    live_profile: Option<&Value>,
    owner: &str,
    slot: &SlotPaths,
) -> Result<(), AppError> {
    edits.watch(&slot.secure);
    let dir_owner = snap
        .profile(&slot.profile)
        .as_ref()
        .and_then(super::identity::identity_from_profile);
    if dir_owner.as_ref().map(|found| found.key.as_str()) != Some(owner) {
        if let Some(account) = live_profile.and_then(|doc| doc.get("oauthAccount")) {
            // 和本机 park 一样：收回时原样写回 oauthAccount，不盖 profileFetchedAt。
            // 这个时间戳只打在默认槽位换进去的那份上。
            edits.put_profile(
                &slot.profile,
                profile_bytes(snap.profile(&slot.profile).as_ref(), Some(account))?,
            );
        }
    }
    let existing = snap.secret(&slot.cred)?;
    if let Some(existing) = existing.as_ref() {
        if has_login(existing) && account_fingerprint(existing) != account_fingerprint(live) {
            let backup_dir = backup_linux(&slot.secure);
            let backup_cred = cred_path(&backup_dir);
            edits.put_cred(&backup_cred, merge(None, existing).to_bytes());
            log::warn!("[ClaudeAccount] 账号槽位里原有另一份登录，已留作备份");
        }
    }
    edits.put_cred(&slot.cred, merge(existing.as_ref(), live).to_bytes());
    Ok(())
}

fn finish_into(
    snap: &Snap,
    edits: &mut Edits,
    default: &SlotPaths,
    target: &SlotPaths,
    target_account: &str,
    target_profile: &Value,
    now_ms: i64,
) -> Result<(), AppError> {
    let mut account = target_profile
        .get("oauthAccount")
        .cloned()
        .unwrap_or(Value::Null);
    if account.is_object() {
        account = stamp_account(&account, now_ms);
        edits.put_profile(
            &default.profile,
            profile_bytes(snap.profile(&default.profile).as_ref(), Some(&account))?,
        );
    }
    let backup_cred = cred_path(&backup_linux(&target.secure));
    if snap
        .secret(&backup_cred)?
        .as_ref()
        .is_some_and(|secret| account_fingerprint(secret) == target_account)
    {
        edits.del_cred(&backup_cred);
    }
    if let Some(secret) = snap.secret(&target.cred)? {
        if account_fingerprint(&secret) == target_account {
            strip_cred(snap, edits, &target.cred, &secret)?;
        }
    }
    Ok(())
}

fn strip_cred(snap: &Snap, edits: &mut Edits, path: &str, secret: &Secret) -> Result<(), AppError> {
    let _ = snap;
    match without_account_keys(secret) {
        Some(rest) => edits.put_cred(path, rest.to_bytes()),
        None => edits.del_cred(path),
    }
    Ok(())
}

fn without_account_keys(secret: &Secret) -> Option<Secret> {
    let rest: Map<String, Value> = secret
        .0
        .iter()
        .filter(|(key, _)| !super::ACCOUNT_KEYS.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    (!rest.is_empty()).then_some(Secret(rest))
}

fn stamp_account(account: &Value, now_ms: i64) -> Value {
    let mut account = account.clone();
    if let Some(object) = account.as_object_mut() {
        object.insert("profileFetchedAt".to_string(), Value::from(now_ms));
    }
    account
}

fn profile_bytes(existing: Option<&Value>, account: Option<&Value>) -> Result<Vec<u8>, AppError> {
    let mut doc = match existing {
        Some(Value::Object(map)) => map.clone(),
        Some(_) => {
            return Err(AppError::localized(
                "claude.snapshot.unreadable",
                "Claude Code 的登录资料不是 JSON 对象，没有改动",
                "Claude Code's profile is not a JSON object. Nothing was changed.",
            ));
        }
        None => Map::new(),
    };
    match account {
        Some(account) => {
            doc.insert("oauthAccount".to_string(), account.clone());
        }
        None => {
            doc.remove("oauthAccount");
        }
    }
    for key in super::ACCOUNT_CACHE_KEYS {
        doc.remove(*key);
    }
    doc.entry("hasCompletedOnboarding".to_string())
        .or_insert(Value::Bool(true));
    serde_json::to_vec_pretty(&Value::Object(doc))
        .map_err(|source| AppError::JsonSerialize { source })
}

fn render_payload(layout: &WslLayout, snap: &Snap, edits: &Edits) -> Vec<u8> {
    let mut out = String::new();
    let lock = format!(
        "{}/.cc-switch/claude-accounts/.switch.lock",
        layout.linux_home
    );
    out.push_str(&format!("LOCK|{lock}\n"));
    for dir in &edits.refresh_dirs {
        out.push_str(&format!(
            "ACQUIRE|{dir}/.oauth_refresh.lock|{REFRESH_STALE}|{LOCK_WAIT}\n"
        ));
        out.push_str(&format!("ACQUIRE_REAL|{dir}|{REFRESH_STALE}|{LOCK_WAIT}\n"));
    }
    for dir in &edits.cred_dirs {
        out.push_str(&format!(
            "ACQUIRE|{dir}/.storage-write.lock|{STORAGE_STALE}|{STORAGE_WAIT}\n"
        ));
    }
    for profile in &edits.profiles {
        out.push_str(&format!(
            "ACQUIRE|{profile}.lock|{CONFIG_STALE}|{CONFIG_WAIT}\n"
        ));
    }
    for (path, _) in &edits.writes {
        out.push_str(&format!("EXPECT|{path}|{}\n", snap.sha(path)));
    }
    for (path, bytes) in &edits.writes {
        if let Some(bytes) = bytes {
            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
            out.push_str(&format!("PUT|{path}|600|{encoded}\n"));
        }
    }
    for (path, bytes) in &edits.writes {
        if bytes.is_none() {
            out.push_str(&format!("DEL|{path}|{}\n", snap.sha(path)));
        }
    }
    out.push_str("END\n");
    out.into_bytes()
}

fn absorb_read(runner: &mut dyn Runner, snap: &mut Snap, paths: &[String]) -> Result<(), AppError> {
    if paths.is_empty() {
        return Ok(());
    }
    let got = runner.read(paths)?;
    if got.len() != paths.len() {
        return Err(wsl_io("读取 WSL 登录文件的结果对不上"));
    }
    snap.absorb(paths, got);
    Ok(())
}

fn default_paths(layout: &WslLayout, snap: &Snap) -> SlotPaths {
    let secure = format!("{}/.claude", layout.linux_home);
    resolve_slot_at(
        snap,
        &secure,
        &format!("{}/.claude.json", layout.linux_home),
    )
}

fn resolve_slot(snap: &Snap, dir: &str) -> SlotPaths {
    resolve_slot_at(snap, dir, &format!("{dir}/.claude.json"))
}

fn resolve_slot_at(snap: &Snap, secure: &str, fallback: &str) -> SlotPaths {
    let legacy = format!("{secure}/.config.json");
    let profile = if snap.raw(&legacy).is_some_and(|bytes| !bytes.is_empty()) {
        legacy
    } else {
        fallback.to_string()
    };
    SlotPaths {
        cred: cred_path(secure),
        secure: secure.to_string(),
        profile,
    }
}

fn default_read_paths(home: &str) -> Vec<String> {
    let secure = format!("{home}/.claude");
    vec![
        cred_path(&secure),
        format!("{secure}/.config.json"),
        format!("{home}/.claude.json"),
    ]
}

fn slot_read_paths(dir: &str) -> Vec<String> {
    vec![
        cred_path(dir),
        format!("{dir}/.config.json"),
        format!("{dir}/.claude.json"),
    ]
}

fn cred_path(dir: &str) -> String {
    format!("{dir}/.credentials.json")
}

fn cred_dir(path: &str) -> Option<String> {
    path.strip_suffix("/.credentials.json").map(str::to_string)
}

/// `<dir>/config` 的备份目录是旁边的 `<dir>/config.prev`（和本机 [`super::files::backup_dir`] 一样）。
fn backup_linux(dir: &str) -> String {
    format!("{dir}.prev")
}

fn linux_of(layout: &WslLayout, path: &Path) -> Result<String, AppError> {
    let text = path.to_string_lossy();
    let linux = if text.starts_with('/') && !text.starts_with("//") {
        text.into_owned()
    } else {
        run_target::windows_path_to_linux(&text, &layout.distro).map_err(|error| {
            AppError::localized(
                "claude.account.wsl_path",
                format!("无法把目录转成 WSL 里的路径: {error}"),
                format!("Couldn't turn the directory into a path inside WSL: {error}"),
            )
        })?
    };
    check_path(&linux)?;
    Ok(linux)
}

fn check_path(path: &str) -> Result<(), AppError> {
    let ok = path.starts_with('/')
        && path.len() <= 4096
        && !path.contains("//")
        && path.split('/').skip(1).all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
        });
    if ok {
        Ok(())
    } else {
        Err(AppError::localized(
            "claude.account.wsl_path",
            format!("WSL 路径不能安全地放进切换脚本: {path}"),
            format!("The WSL path cannot be passed safely to the switch script: {path}"),
        ))
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn wsl_io(zh: &str) -> AppError {
    AppError::localized("claude.account.wsl_switch", zh, zh)
}

#[cfg(any(windows, test))]
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(any(windows, test))]
fn read_script(paths: &[String]) -> Result<String, AppError> {
    let mut script = String::from("set -eu\n");
    for path in paths {
        check_path(path)?;
        let quoted = shell_single_quote(path);
        script.push_str(&format!(
            "if [ -e {quoted} ]; then printf '1'; base64 -w 0 -- {quoted}; printf '\\n'; else printf '0\\n'; fi\n"
        ));
    }
    Ok(script)
}

#[cfg(any(windows, test))]
fn parse_read_output(text: &str, count: usize) -> Result<Vec<Option<Vec<u8>>>, AppError> {
    let lines: Vec<&str> = text.split('\n').filter(|line| !line.is_empty()).collect();
    if lines.len() != count {
        return Err(wsl_io("读取 WSL 登录文件的行数不对"));
    }
    let mut out = Vec::with_capacity(count);
    for line in lines {
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix('1') {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(rest)
                .map_err(|_| wsl_io("WSL 登录文件的内容无法解码"))?;
            out.push(Some(bytes));
        } else if line == "0" {
            out.push(None);
        } else {
            return Err(wsl_io("WSL 登录文件的读取结果无法识别"));
        }
    }
    Ok(out)
}

struct WslExeRunner<'a> {
    #[cfg_attr(not(windows), allow(dead_code))]
    layout: &'a WslLayout,
}

impl Runner for WslExeRunner<'_> {
    fn read(&mut self, paths: &[String]) -> Result<Vec<Option<Vec<u8>>>, AppError> {
        #[cfg(windows)]
        {
            let script = read_script(paths)?;
            let output = run_wsl(self.layout, &script, &[])?;
            let text = String::from_utf8_lossy(&output);
            parse_read_output(&text, paths.len())
        }
        #[cfg(not(windows))]
        {
            let _ = paths;
            Err(wsl_io("WSL 切换只在 Windows 上执行"))
        }
    }

    fn apply(&mut self, script: &str, payload: &[u8]) -> Result<(), AppError> {
        #[cfg(windows)]
        {
            run_wsl(self.layout, script, payload).map(|_| ())
        }
        #[cfg(not(windows))]
        {
            let _ = (script, payload);
            Err(wsl_io("WSL 切换只在 Windows 上执行"))
        }
    }
}

#[cfg(windows)]
fn run_wsl(layout: &WslLayout, script: &str, payload: &[u8]) -> Result<Vec<u8>, AppError> {
    use std::io::Write;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let argv = run_target::wsl_exec_argv(&layout.distro, &layout.user, &layout.linux_home, script)
        .map_err(|error| {
            AppError::localized(
                "claude.account.wsl_command",
                format!("无法构造在 WSL 里切换登录的命令: {error}"),
                format!("Couldn't build the command that switches the WSL sign-in: {error}"),
            )
        })?;
    let mut child = Command::new("wsl.exe");
    child
        .args(&argv)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW);
    let mut child = child.spawn().map_err(|error| {
        AppError::localized(
            "claude.account.wsl_switch",
            format!("无法启动 wsl.exe 切换登录: {error}"),
            format!("Couldn't start wsl.exe to switch the sign-in: {error}"),
        )
    })?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload).map_err(|error| {
            AppError::localized(
                "claude.account.wsl_switch",
                format!("把切换内容写进 WSL 失败: {error}"),
                format!("Couldn't write the switch payload into WSL: {error}"),
            )
        })?;
    }
    let output = child.wait_with_output().map_err(|error| {
        AppError::localized(
            "claude.account.wsl_switch",
            format!("等待 WSL 切换结束失败: {error}"),
            format!("Waiting for the WSL switch failed: {error}"),
        )
    })?;
    match output.status.code() {
        Some(0) => Ok(output.stdout),
        Some(3) => Err(changed()),
        Some(75) => {
            let detail = String::from_utf8_lossy(&output.stderr);
            let path = detail.lines().find_map(|line| {
                line.trim()
                    .strip_prefix("lock busy:")
                    .map(str::trim)
                    .filter(|path| !path.is_empty())
            });
            match path {
                Some(path) => Err(AppError::localized(
                    "claude.snapshot.lock_busy",
                    format!(
                        "Claude Code 正在使用登录信息（刷新或登录中），请稍后再切换（{path}）"
                    ),
                    format!(
                        "Claude Code is using its sign-in right now (refreshing or signing in). Try switching again in a moment ({path})."
                    ),
                )),
                None => Err(AppError::localized(
                    "claude.snapshot.lock_busy",
                    "Claude Code 正在使用登录信息（刷新或登录中），请稍后再切换",
                    "Claude Code is using its sign-in right now (refreshing or signing in). Try switching again in a moment.",
                )),
            }
        }
        _ => {
            let detail = String::from_utf8_lossy(&output.stderr);
            let detail = detail.trim();
            Err(AppError::localized(
                "claude.account.wsl_switch",
                format!("在 WSL 里切换登录失败: {detail}"),
                format!("Switching the sign-in inside WSL failed: {detail}"),
            ))
        }
    }
}

#[cfg(windows)]
struct OwnerCache {
    key: String,
    at: Instant,
    owner: Option<String>,
}

#[cfg(windows)]
fn cache_slot() -> &'static std::sync::Mutex<Option<OwnerCache>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Option<OwnerCache>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(None))
}

fn invalidate_owner_cache() {
    #[cfg(windows)]
    if let Ok(mut guard) = cache_slot().lock() {
        *guard = None;
    }
}

#[cfg(windows)]
pub(super) fn probe_live_owner(wsl: &WslTarget) -> Result<Option<String>, AppError> {
    let layout = WslLayout::from_target(wsl)?;
    let key = format!("{}:{}", layout.distro, layout.linux_home);
    if let Ok(guard) = cache_slot().lock() {
        if let Some(cache) = guard.as_ref() {
            if cache.key == key && cache.at.elapsed() < OWNER_CACHE_TTL {
                return Ok(cache.owner.clone());
            }
        }
    }
    let legacy = shell_single_quote(&format!("{}/.claude/.config.json", layout.linux_home));
    let profile = shell_single_quote(&format!("{}/.claude.json", layout.linux_home));
    let script = format!(
        "if [ -s {legacy} ]; then cat {legacy}; else cat {profile} 2>/dev/null || true; fi\n"
    );
    let output = run_wsl(&layout, &script, &[])?;
    let text = String::from_utf8_lossy(&output);
    let owner = serde_json::from_str::<Value>(text.trim())
        .ok()
        .as_ref()
        .and_then(super::identity::identity_from_profile)
        .map(|found| found.key);
    if let Ok(mut guard) = cache_slot().lock() {
        *guard = Some(OwnerCache {
            key,
            at: Instant::now(),
            owner: owner.clone(),
        });
    }
    Ok(owner)
}

#[cfg(windows)]
pub(super) fn probe_dir_has_login(wsl: &WslTarget, linux_dir: &str) -> Result<bool, AppError> {
    let layout = WslLayout::from_target(wsl)?;
    check_path(linux_dir)?;
    let cred = shell_single_quote(&cred_path(linux_dir));
    let script = format!("if [ -f {cred} ]; then printf '1'; base64 -w 0 -- {cred}; printf '\\n'; else printf '0\\n'; fi\n");
    let output = run_wsl(&layout, &script, &[])?;
    let text = String::from_utf8_lossy(&output);
    let parsed = parse_read_output(text.trim(), 1)?;
    match parsed.into_iter().next().flatten() {
        Some(bytes) => Ok(Secret::parse(&bytes).ok().as_ref().is_some_and(has_login)),
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::Mutex;

    use serde_json::json;
    use sha2::{Digest, Sha256};

    use super::super::store::Slot;
    use super::*;
    use crate::services::claude_account::identity::{self, Identity};

    struct FakeRunner {
        files: Mutex<HashMap<String, Vec<u8>>>,
        fail_apply: bool,
        last_script: RefCell<String>,
        last_payload: RefCell<Vec<u8>>,
        applies: RefCell<usize>,
    }

    impl FakeRunner {
        fn new(files: HashMap<String, Vec<u8>>) -> Self {
            Self {
                files: Mutex::new(files),
                fail_apply: false,
                last_script: RefCell::new(String::new()),
                last_payload: RefCell::new(Vec::new()),
                applies: RefCell::new(0),
            }
        }

        fn file(&self, path: &str) -> Option<Vec<u8>> {
            self.files.lock().unwrap().get(path).cloned()
        }
    }

    impl Runner for FakeRunner {
        fn read(&mut self, paths: &[String]) -> Result<Vec<Option<Vec<u8>>>, AppError> {
            let files = self.files.lock().unwrap();
            Ok(paths.iter().map(|path| files.get(path).cloned()).collect())
        }

        fn apply(&mut self, script: &str, payload: &[u8]) -> Result<(), AppError> {
            *self.last_script.borrow_mut() = script.to_string();
            *self.last_payload.borrow_mut() = payload.to_vec();
            *self.applies.borrow_mut() += 1;
            if self.fail_apply {
                return Err(changed());
            }
            apply_payload_to_map(&mut self.files.lock().unwrap(), script, payload)
        }
    }

    fn apply_payload_to_map(
        files: &mut HashMap<String, Vec<u8>>,
        script: &str,
        payload: &[u8],
    ) -> Result<(), AppError> {
        if !script.contains("flock") || !script.contains("chmod") || !script.contains("mv -f") {
            return Err(wsl_io("切换脚本缺少 flock、chmod 或原子改名"));
        }
        let text = String::from_utf8_lossy(payload);
        let mut checks = Vec::new();
        let mut puts = Vec::new();
        let mut dels = Vec::new();
        let mut ended = false;
        for line in text.split('\n') {
            if line.is_empty() {
                continue;
            }
            if line == "END" {
                ended = true;
                break;
            }
            let (op, rest) = line.split_once('|').ok_or_else(|| wsl_io("bad plan"))?;
            match op {
                "LOCK" | "ACQUIRE" | "ACQUIRE_REAL" => {}
                "EXPECT" => {
                    let (path, sha) = rest.split_once('|').ok_or_else(|| wsl_io("bad plan"))?;
                    checks.push((path.to_string(), sha.to_string()));
                }
                "PUT" => {
                    let mut parts = rest.splitn(3, '|');
                    let path = parts.next().unwrap();
                    let _mode = parts.next().unwrap();
                    let b64 = parts.next().unwrap_or("");
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(b64)
                        .map_err(|_| wsl_io("bad plan"))?;
                    puts.push((path.to_string(), bytes));
                }
                "DEL" => {
                    let (path, sha) = rest.split_once('|').ok_or_else(|| wsl_io("bad plan"))?;
                    dels.push((path.to_string(), sha.to_string()));
                }
                _ => return Err(wsl_io("bad plan")),
            }
        }
        if !ended {
            return Err(wsl_io("bad plan"));
        }
        let sha_of = |bytes: Option<&Vec<u8>>| match bytes {
            Some(bytes) => sha256_hex(bytes),
            None => "-".to_string(),
        };
        for (path, sha) in &checks {
            if sha_of(files.get(path)) != *sha {
                return Err(changed());
            }
        }
        for (path, bytes) in puts {
            files.insert(path, bytes);
        }
        for (path, sha) in dels {
            if sha_of(files.get(&path)) != sha {
                return Err(changed());
            }
            files.remove(&path);
        }
        Ok(())
    }

    struct MemAccounts {
        dirs: RefCell<HashMap<String, PathBuf>>,
    }

    impl super::super::Accounts for MemAccounts {
        fn dir_of(&self, account: &str) -> Option<PathBuf> {
            self.dirs.borrow().get(account).cloned()
        }
    }

    struct MemAdopter {
        seq: RefCell<usize>,
        adopted: RefCell<Vec<String>>,
    }

    impl super::super::Adopter for MemAdopter {
        fn adopt(&self, found: &Identity, _profile: &Value) -> Result<PathBuf, AppError> {
            let n = *self.seq.borrow();
            *self.seq.borrow_mut() = n + 1;
            self.adopted.borrow_mut().push(found.key.clone());
            Ok(PathBuf::from(format!(
                "/home/sean/.cc-switch/claude-accounts/adopt{n}/config"
            )))
        }
    }

    struct World {
        _tmp: tempfile::TempDir,
        data: PathBuf,
        accounts: MemAccounts,
        adopter: MemAdopter,
    }

    impl World {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let data = tmp.path().join("data");
            std::fs::create_dir_all(&data).unwrap();
            Self {
                data,
                accounts: MemAccounts {
                    dirs: RefCell::new(HashMap::new()),
                },
                adopter: MemAdopter {
                    seq: RefCell::new(0),
                    adopted: RefCell::new(Vec::new()),
                },
                _tmp: tmp,
            }
        }

        fn ctx(&self) -> Ctx<'_> {
            Ctx {
                data_dir: &self.data,
                default: Slot::Default {
                    home: PathBuf::from("/home/sean"),
                },
                store: &UnusedStore,
                accounts: &self.accounts,
                adopter: &self.adopter,
            }
        }
    }

    struct UnusedStore;

    impl super::super::store::CredentialStore for UnusedStore {
        fn read(&self, _slot: &Slot) -> Result<Option<Secret>, AppError> {
            panic!("wsl switch must not read credentials through the windows store");
        }
        fn write(&self, _slot: &Slot, _secret: &Secret) -> Result<(), AppError> {
            panic!("wsl switch must not write credentials through the windows store");
        }
        fn delete(&self, _slot: &Slot) -> Result<(), AppError> {
            panic!("wsl switch must not delete credentials through the windows store");
        }
    }

    fn layout() -> WslLayout {
        WslLayout {
            distro: "Ubuntu-26.04".to_string(),
            user: "sean".to_string(),
            linux_home: "/home/sean".to_string(),
        }
    }

    fn home() -> &'static str {
        "/home/sean"
    }

    fn acct(name: &str) -> String {
        format!("/home/sean/.cc-switch/claude-accounts/{name}/config")
    }

    fn secret_bytes(org: &str, refresh: &str, mcp: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "claudeAiOauth": {"accessToken": format!("at-{refresh}"), "refreshToken": refresh, "expiresAt": 1},
            "organizationUuid": org,
            "mcpOAuth": {"server": mcp},
        }))
        .unwrap()
    }

    fn profile_bytes(account: &str, org: &str, extra: Value) -> Vec<u8> {
        let mut doc = extra;
        doc["oauthAccount"] = json!({
            "accountUuid": account,
            "organizationUuid": org,
            "emailAddress": format!("{account}@x.com"),
        });
        serde_json::to_vec(&doc).unwrap()
    }

    fn key(account: &str, org: &str) -> String {
        identity::account_key(account, Some(org))
    }

    fn run(
        world: &World,
        kind: Kind,
        target: &str,
        linux_dir: &str,
        runner: &mut dyn Runner,
    ) -> Result<Outcome, AppError> {
        let layout = layout();
        let journal_dir = PathBuf::from(linux_dir);
        let ctx = world.ctx();
        let job = Job {
            ctx: &ctx,
            kind,
            layout: &layout,
            target,
            linux_dir,
            journal_dir: &journal_dir,
            now_ms: 1_700_000_000_000,
        };
        execute(&job, runner)
    }

    fn oauth_email(bytes: &[u8]) -> String {
        let doc: Value = serde_json::from_slice(bytes).unwrap();
        doc["oauthAccount"]["emailAddress"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn refresh_of(bytes: &[u8]) -> Option<String> {
        let doc: Value = serde_json::from_slice(bytes).unwrap();
        doc.get("claudeAiOauth")?
            .get("refreshToken")?
            .as_str()
            .map(str::to_string)
    }

    fn mcp_of(bytes: &[u8]) -> String {
        let doc: Value = serde_json::from_slice(bytes).unwrap();
        doc["mcpOAuth"]["server"].as_str().unwrap().to_string()
    }

    #[test]
    fn switch_moves_account_keys_and_keeps_shared_keys() {
        let world = World::new();
        let a_key = key("acct-a", "org-a");
        let b_key = key("acct-b", "org-b");
        let a_dir = acct("a");
        let b_dir = acct("b");
        world
            .accounts
            .dirs
            .borrow_mut()
            .insert(a_key.clone(), PathBuf::from(&a_dir));
        world
            .accounts
            .dirs
            .borrow_mut()
            .insert(b_key.clone(), PathBuf::from(&b_dir));
        let mut files = HashMap::new();
        files.insert(
            format!("{}/.claude/.credentials.json", home()),
            secret_bytes("org-a", "refresh-a", "mcp-default"),
        );
        files.insert(
            format!("{}/.claude.json", home()),
            profile_bytes(
                "acct-a",
                "org-a",
                json!({"mcpServers": {"keep": true}, "cachedUsageUtilization": {"x": 1}}),
            ),
        );
        files.insert(
            cred_path(&a_dir),
            secret_bytes("org-a", "refresh-a-old", "mcp-a"),
        );
        files.insert(
            format!("{a_dir}/.claude.json"),
            profile_bytes("acct-a", "org-a", json!({})),
        );
        files.insert(
            cred_path(&b_dir),
            secret_bytes("org-b", "refresh-b", "mcp-b"),
        );
        files.insert(
            format!("{b_dir}/.claude.json"),
            profile_bytes("acct-b", "org-b", json!({"mcpServers": {"b": true}})),
        );
        files.insert(
            format!("{}/.claude/settings.json", home()),
            b"do-not-touch".to_vec(),
        );
        let mut runner = FakeRunner::new(files);
        let outcome = run(&world, Kind::Switch, &b_key, &b_dir, &mut runner).unwrap();
        assert!(
            matches!(outcome, Outcome::Switched { previous: Some((key, _)), .. } if key == a_key)
        );

        let default_cred = runner
            .file(&format!("{}/.claude/.credentials.json", home()))
            .unwrap();
        assert_eq!(refresh_of(&default_cred).as_deref(), Some("refresh-b"));
        assert_eq!(mcp_of(&default_cred), "mcp-default");

        let parked = runner.file(&cred_path(&a_dir)).unwrap();
        assert_eq!(refresh_of(&parked).as_deref(), Some("refresh-a"));
        assert_eq!(mcp_of(&parked), "mcp-a");
        let backup = runner
            .file(&cred_path(&backup_linux(&a_dir)))
            .expect("previous login in A's slot is backed up");
        assert_eq!(refresh_of(&backup).as_deref(), Some("refresh-a-old"));
        let backup_doc: Value = serde_json::from_slice(&backup).unwrap();
        assert!(backup_doc.get("mcpOAuth").is_none());

        let target = runner.file(&cred_path(&b_dir)).unwrap();
        assert!(refresh_of(&target).is_none());
        assert_eq!(mcp_of(&target), "mcp-b");

        let profile = runner.file(&format!("{}/.claude.json", home())).unwrap();
        assert_eq!(oauth_email(&profile), "acct-b@x.com");
        let doc: Value = serde_json::from_slice(&profile).unwrap();
        assert!(doc.get("cachedUsageUtilization").is_none());
        assert_eq!(doc["mcpServers"]["keep"], true);
        assert!(doc["oauthAccount"]["profileFetchedAt"].is_number());
        assert_eq!(
            runner
                .file(&format!("{}/.claude/settings.json", home()))
                .unwrap(),
            b"do-not-touch"
        );

        let script = runner.last_script.borrow().clone();
        let payload = String::from_utf8(runner.last_payload.borrow().clone()).unwrap();
        assert!(script.contains("flock"));
        assert!(script.contains("chmod"));
        assert!(script.contains("mv -f"));
        assert!(!script.contains("refresh-b"));
        assert!(
            payload.contains("refresh-b")
                || payload
                    .contains(&base64::engine::general_purpose::STANDARD.encode(b"refresh-b"))
        );
        assert!(payload.contains(".oauth_refresh.lock"));
        assert!(payload.contains(".storage-write.lock"));
        assert!(payload.contains("LOCK|/home/sean/.cc-switch/claude-accounts/.switch.lock"));
        let argv = run_target::wsl_exec_argv("Ubuntu-26.04", "sean", home(), &script).unwrap();
        assert_eq!(
            &argv[..9],
            [
                "-d",
                "Ubuntu-26.04",
                "-u",
                "sean",
                "--cd",
                "/home/sean",
                "--exec",
                "/bin/sh",
                "-c"
            ]
        );
        assert!(!argv.iter().any(|arg| arg == "--"));
        assert!(!argv.iter().any(|arg| arg.contains("refresh-b")));

        let state = super::super::load_state(&world.data);
        assert_eq!(state.journal.unwrap().step, Step::Done);
        super::super::confirm(&world.data);
        assert!(super::super::load_state(&world.data).journal.is_none());
    }

    #[test]
    fn already_live_does_not_apply() {
        let world = World::new();
        let b_key = key("acct-b", "org-b");
        let b_dir = acct("b");
        let mut files = HashMap::new();
        files.insert(
            format!("{}/.claude/.credentials.json", home()),
            secret_bytes("org-b", "refresh-b", "mcp-default"),
        );
        files.insert(
            format!("{}/.claude.json", home()),
            profile_bytes("acct-b", "org-b", json!({})),
        );
        let mut runner = FakeRunner::new(files);
        let outcome = run(&world, Kind::Switch, &b_key, &b_dir, &mut runner).unwrap();
        assert_eq!(outcome, Outcome::AlreadyLive);
        assert_eq!(*runner.applies.borrow(), 0);
    }

    #[test]
    fn unknown_live_login_is_adopted_before_the_move() {
        let world = World::new();
        let b_key = key("acct-b", "org-b");
        let b_dir = acct("b");
        world
            .accounts
            .dirs
            .borrow_mut()
            .insert(b_key.clone(), PathBuf::from(&b_dir));
        let mut files = HashMap::new();
        files.insert(
            format!("{}/.claude/.credentials.json", home()),
            secret_bytes("org-a", "refresh-a", "mcp-default"),
        );
        files.insert(
            format!("{}/.claude.json", home()),
            profile_bytes("acct-a", "org-a", json!({})),
        );
        files.insert(
            cred_path(&b_dir),
            secret_bytes("org-b", "refresh-b", "mcp-b"),
        );
        files.insert(
            format!("{b_dir}/.claude.json"),
            profile_bytes("acct-b", "org-b", json!({})),
        );
        let mut runner = FakeRunner::new(files);
        let outcome = run(&world, Kind::Switch, &b_key, &b_dir, &mut runner).unwrap();
        match outcome {
            Outcome::Switched {
                adopted: Some(adopted),
                previous: Some((previous, dir)),
                ..
            } => {
                assert_eq!(adopted, key("acct-a", "org-a"));
                assert_eq!(previous, adopted);
                let parked = runner.file(&cred_path(&dir.to_string_lossy())).unwrap();
                assert_eq!(refresh_of(&parked).as_deref(), Some("refresh-a"));
                let parked_profile = runner
                    .file(&format!("{}/.claude.json", dir.to_string_lossy()))
                    .unwrap();
                let parked_doc: Value = serde_json::from_slice(&parked_profile).unwrap();
                assert!(parked_doc["oauthAccount"].get("profileFetchedAt").is_none());
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(world.adopter.adopted.borrow().len(), 1);
    }

    #[test]
    fn vacate_puts_the_login_back_and_clears_the_default_profile() {
        let world = World::new();
        let b_key = key("acct-b", "org-b");
        let b_dir = acct("b");
        world
            .accounts
            .dirs
            .borrow_mut()
            .insert(b_key.clone(), PathBuf::from(&b_dir));
        let mut files = HashMap::new();
        files.insert(
            format!("{}/.claude/.credentials.json", home()),
            secret_bytes("org-b", "refresh-b", "mcp-default"),
        );
        files.insert(
            format!("{}/.claude.json", home()),
            profile_bytes(
                "acct-b",
                "org-b",
                json!({"mcpServers": {"keep": true}, "clientDataCache": {"x": 1}}),
            ),
        );
        files.insert(
            cred_path(&b_dir),
            secret_bytes("org-b", "refresh-b", "mcp-b"),
        );
        files.insert(
            format!("{b_dir}/.claude.json"),
            profile_bytes("acct-b", "org-b", json!({})),
        );
        let mut runner = FakeRunner::new(files);
        run(&world, Kind::Vacate, &b_key, &b_dir, &mut runner).unwrap();
        let default_cred = runner
            .file(&format!("{}/.claude/.credentials.json", home()))
            .unwrap();
        assert!(refresh_of(&default_cred).is_none());
        assert_eq!(mcp_of(&default_cred), "mcp-default");
        let profile = runner.file(&format!("{}/.claude.json", home())).unwrap();
        let doc: Value = serde_json::from_slice(&profile).unwrap();
        assert!(doc.get("oauthAccount").is_none());
        assert!(doc.get("clientDataCache").is_none());
        assert_eq!(doc["mcpServers"]["keep"], true);
        let parked = runner.file(&cred_path(&b_dir)).unwrap();
        assert_eq!(refresh_of(&parked).as_deref(), Some("refresh-b"));
        assert_eq!(mcp_of(&parked), "mcp-b");
        assert!(super::super::load_state(&world.data).journal.is_none());
    }

    #[test]
    fn replace_swaps_the_live_token_without_parking_it() {
        let world = World::new();
        let b_key = key("acct-b", "org-b");
        let b_dir = acct("b");
        let mut files = HashMap::new();
        files.insert(
            format!("{}/.claude/.credentials.json", home()),
            secret_bytes("org-b", "refresh-old", "mcp-default"),
        );
        files.insert(
            format!("{}/.claude.json", home()),
            profile_bytes("acct-b", "org-b", json!({})),
        );
        files.insert(
            cred_path(&b_dir),
            secret_bytes("org-b", "refresh-new", "mcp-b"),
        );
        files.insert(
            format!("{b_dir}/.claude.json"),
            profile_bytes("acct-b", "org-b", json!({})),
        );
        let mut runner = FakeRunner::new(files);
        let outcome = run(&world, Kind::Replace, &b_key, &b_dir, &mut runner).unwrap();
        assert!(matches!(outcome, Outcome::Switched { .. }));
        let default_cred = runner
            .file(&format!("{}/.claude/.credentials.json", home()))
            .unwrap();
        assert_eq!(refresh_of(&default_cred).as_deref(), Some("refresh-new"));
        assert_eq!(mcp_of(&default_cred), "mcp-default");
        assert!(refresh_of(&runner.file(&cred_path(&b_dir)).unwrap()).is_none());
    }

    #[test]
    fn changed_file_aborts_without_writing_and_drops_the_journal() {
        let world = World::new();
        let a_key = key("acct-a", "org-a");
        let b_key = key("acct-b", "org-b");
        let a_dir = acct("a");
        let b_dir = acct("b");
        world
            .accounts
            .dirs
            .borrow_mut()
            .insert(a_key.clone(), PathBuf::from(&a_dir));
        world
            .accounts
            .dirs
            .borrow_mut()
            .insert(b_key.clone(), PathBuf::from(&b_dir));
        let mut files = HashMap::new();
        files.insert(
            format!("{}/.claude/.credentials.json", home()),
            secret_bytes("org-a", "refresh-a", "mcp-default"),
        );
        files.insert(
            format!("{}/.claude.json", home()),
            profile_bytes("acct-a", "org-a", json!({})),
        );
        files.insert(
            cred_path(&b_dir),
            secret_bytes("org-b", "refresh-b", "mcp-b"),
        );
        files.insert(
            format!("{b_dir}/.claude.json"),
            profile_bytes("acct-b", "org-b", json!({})),
        );
        let before = files.clone();
        let mut runner = FakeRunner::new(files);
        runner.fail_apply = true;
        let error = run(&world, Kind::Switch, &b_key, &b_dir, &mut runner).unwrap_err();
        let _ = error;
        assert_eq!(runner.files.lock().unwrap().clone(), before);
        assert_eq!(
            super::super::load_state(&world.data).journal.unwrap().step,
            Step::Parked
        );
        run(&world, Kind::Recover, "", "", &mut runner).unwrap();
        assert_eq!(runner.files.lock().unwrap().clone(), before);
        assert!(super::super::load_state(&world.data).journal.is_none());
    }

    #[test]
    fn recover_finishes_a_switch_that_committed_in_the_distro() {
        let world = World::new();
        let b_key = key("acct-b", "org-b");
        let b_dir = acct("b");
        let cred = secret_bytes("org-b", "refresh-b", "mcp-b");
        let fingerprint = super::account_fingerprint(&Secret::parse(&cred).unwrap());
        let mut files = HashMap::new();
        files.insert(
            format!("{}/.claude/.credentials.json", home()),
            secret_bytes("org-b", "refresh-b", "mcp-default"),
        );
        files.insert(
            format!("{}/.claude.json", home()),
            profile_bytes(
                "acct-b",
                "org-b",
                json!({"cachedUsageUtilization": {"x": 1}, "mcpServers": {"keep": true}}),
            ),
        );
        files.insert(cred_path(&b_dir), cred);
        files.insert(
            format!("{b_dir}/.claude.json"),
            profile_bytes("acct-b", "org-b", json!({})),
        );
        super::super::update_state(&world.data, |state| {
            state.journal = Some(Journal {
                target: b_key.clone(),
                target_dir: PathBuf::from(&b_dir),
                target_org: Some("org-b".to_string()),
                target_account: fingerprint,
                before_account: None,
                before_owner: None,
                step: Step::Committed,
            });
        })
        .unwrap();
        let mut runner = FakeRunner::new(files);
        run(&world, Kind::Recover, "", "", &mut runner).unwrap();
        let target = runner.file(&cred_path(&b_dir)).unwrap();
        assert!(refresh_of(&target).is_none());
        assert_eq!(mcp_of(&target), "mcp-b");
        let doc: Value =
            serde_json::from_slice(&runner.file(&format!("{}/.claude.json", home())).unwrap())
                .unwrap();
        assert!(doc.get("cachedUsageUtilization").is_none());
        assert_eq!(doc["mcpServers"]["keep"], true);
        assert_eq!(
            super::super::load_state(&world.data).journal.unwrap().step,
            Step::Done
        );
    }

    #[test]
    fn legacy_config_json_is_the_profile_that_moves() {
        let world = World::new();
        let b_key = key("acct-b", "org-b");
        let b_dir = acct("b");
        let mut files = HashMap::new();
        files.insert(
            format!("{}/.claude/.credentials.json", home()),
            secret_bytes("org-a", "refresh-a", "mcp-default"),
        );
        files.insert(
            format!("{}/.claude/.config.json", home()),
            profile_bytes("acct-a", "org-a", json!({"theme": "dark"})),
        );
        files.insert(
            format!("{}/.claude.json", home()),
            b"not-the-profile".to_vec(),
        );
        files.insert(
            cred_path(&b_dir),
            secret_bytes("org-b", "refresh-b", "mcp-b"),
        );
        files.insert(
            format!("{b_dir}/.claude.json"),
            profile_bytes("acct-b", "org-b", json!({})),
        );
        let mut runner = FakeRunner::new(files);
        // A is not registered: adopted. That's fine; we only check which profile file changed.
        run(&world, Kind::Switch, &b_key, &b_dir, &mut runner).unwrap();
        let legacy = runner
            .file(&format!("{}/.claude/.config.json", home()))
            .unwrap();
        assert_eq!(oauth_email(&legacy), "acct-b@x.com");
        let doc: Value = serde_json::from_slice(&legacy).unwrap();
        assert_eq!(doc["theme"], "dark");
        assert_eq!(
            runner.file(&format!("{}/.claude.json", home())).unwrap(),
            b"not-the-profile"
        );
    }

    #[test]
    fn read_script_quotes_paths_and_roundtrips_bytes() {
        let path = "/home/sean/.claude/.credentials.json";
        let script = read_script(&[path.to_string()]).unwrap();
        assert!(script.contains("'/home/sean/.claude/.credentials.json'"));
        assert!(script.contains("base64 -w 0"));
        let body = br#"{"refreshToken":"a'b $HOME"}"#;
        let encoded = base64::engine::general_purpose::STANDARD.encode(body);
        let parsed = parse_read_output(&format!("1{encoded}\n0\n"), 2).unwrap();
        assert_eq!(parsed[0].as_deref(), Some(body.as_slice()));
        assert!(parsed[1].is_none());
    }

    /// macOS CI 没有 `flock(1)`；脚本只在发行版的 Linux 里跑，所以只在 Linux 上实测。
    #[cfg(target_os = "linux")]
    #[test]
    fn shell_script_applies_atomically_and_rejects_a_changed_file() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("home");
        let cred = root.join(".claude").join(".credentials.json");
        std::fs::create_dir_all(cred.parent().unwrap()).unwrap();
        std::fs::write(&cred, b"old-token").unwrap();
        let cred_s = cred.to_string_lossy().replace('\\', "/");
        let sha = sha256_hex(b"old-token");
        let new_b64 = base64::engine::general_purpose::STANDARD.encode(b"new-token");
        let lock = root.join(".switch.lock");
        let lock_s = lock.to_string_lossy().replace('\\', "/");
        let payload =
            format!("LOCK|{lock_s}\nEXPECT|{cred_s}|{sha}\nPUT|{cred_s}|600|{new_b64}\nEND\n");
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(APPLY_SCRIPT)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                child
                    .stdin
                    .as_mut()
                    .unwrap()
                    .write_all(payload.as_bytes())?;
                drop(child.stdin.take());
                child.wait_with_output()
            })
            .unwrap();
        assert!(
            status.status.success(),
            "{}",
            String::from_utf8_lossy(&status.stderr)
        );
        assert_eq!(std::fs::read(&cred).unwrap(), b"new-token");
        let mode = std::fs::metadata(&cred).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(mode.mode() & 0o777, 0o600);

        let sha_now = sha256_hex(b"new-token");
        let stale =
            format!("LOCK|{lock_s}\nEXPECT|{cred_s}|{sha}\nPUT|{cred_s}|600|{new_b64}\nEND\n");
        let failed = std::process::Command::new("sh")
            .arg("-c")
            .arg(APPLY_SCRIPT)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                child.stdin.as_mut().unwrap().write_all(stale.as_bytes())?;
                drop(child.stdin.take());
                child.wait_with_output()
            })
            .unwrap();
        assert_eq!(failed.status.code(), Some(3));
        assert_eq!(std::fs::read(&cred).unwrap(), b"new-token");
        let _ = sha_now;
    }

    /// 备份目录还不存在时要先建父目录再拿锁。不能把「没有这个目录」报成 Claude 正在刷新。
    #[cfg(target_os = "linux")]
    #[test]
    fn shell_script_creates_a_missing_lock_parent_and_still_reports_a_held_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_string_lossy().replace('\\', "/");
        let missing = format!("{root}/config.prev/.oauth_refresh.lock");
        let held = format!("{root}/held/.oauth_refresh.lock");
        std::fs::create_dir_all(&held).unwrap();
        let switch_lock = format!("{root}/.switch.lock");
        // Ubuntu 的 /bin/sh 是 dash，Fedora / RHEL 的 /bin/sh 是 bash。两边都跑同一支脚本。
        for shell in ["sh", "bash"] {
            let created = std::process::Command::new(shell)
                .arg("-c")
                .arg(APPLY_SCRIPT)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    use std::io::Write;
                    child.stdin.as_mut().unwrap().write_all(
                        format!("LOCK|{switch_lock}\nACQUIRE|{missing}|60|1\nEND\n").as_bytes(),
                    )?;
                    drop(child.stdin.take());
                    child.wait_with_output()
                })
                .unwrap();
            assert!(
                created.status.success(),
                "{shell}: {}",
                String::from_utf8_lossy(&created.stderr)
            );
            assert!(!std::path::Path::new(&missing).exists());

            let busy = std::process::Command::new(shell)
                .arg("-c")
                .arg(APPLY_SCRIPT)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    use std::io::Write;
                    child.stdin.as_mut().unwrap().write_all(
                        format!("LOCK|{switch_lock}\nACQUIRE|{held}|60|1\nEND\n").as_bytes(),
                    )?;
                    drop(child.stdin.take());
                    child.wait_with_output()
                })
                .unwrap();
            assert_eq!(busy.status.code(), Some(75), "{shell}");
            let stderr = String::from_utf8_lossy(&busy.stderr);
            assert!(
                stderr.contains(&format!("lock busy: {held}")),
                "{shell}: {stderr}"
            );
        }
    }

    #[test]
    fn wsl_exec_argv_uses_exec_and_keeps_the_script_as_one_arg() {
        let argv =
            run_target::wsl_exec_argv("Ubuntu-26.04", "sean", "/home/sean", APPLY_SCRIPT).unwrap();
        assert!(!argv.iter().any(|arg| arg == "--"));
        assert_eq!(argv[6], "--exec");
        assert_eq!(argv[7], "/bin/sh");
        assert_eq!(argv[8], "-c");
        assert!(argv[9].contains("flock"));
        assert!(argv[9].contains("chmod"));
        assert!(!argv[9].contains("refresh-b"));
    }

    #[test]
    fn backup_path_sits_beside_the_account_dir() {
        assert_eq!(
            backup_linux("/home/sean/.cc-switch/claude-accounts/abc/config"),
            "/home/sean/.cc-switch/claude-accounts/abc/config.prev"
        );
    }

    #[test]
    fn unsafe_paths_are_rejected() {
        assert!(check_path("/home/sean/.claude/.credentials.json").is_ok());
        assert!(check_path("/home/sean/my dir/.claude").is_err());
        assert!(check_path("/home/sean/../root/.claude").is_err());
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}
