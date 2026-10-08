//! 一次写客户端文件的操作：按文件记录写前意图（pending），逐个发布，崩溃后前滚或丢弃。
//!
//! 把一次切换当成一个操作来提交：
//! - 发布前的失败（解析失败、路由校验不通过、并发冲突）什么都不改；
//! - 一旦开始发布，就以 pending 为准前滚补完。写到一半失败或进程崩溃，下次操作这个
//!   应用或下次启动时按 pending 补完剩下的文件和状态（指针等）。
//!
//! 只调换「先写文件、后改指针」的顺序不够：文件写成 B、指针更新失败，照样不一致。
//! 恢复规则（按文件比对当前内容和写前、写后的 hash。文件按顺序发布，有一个是写后内容，
//! 就说明已经开始发布了；换进第一个文件之前 pending 里还会先记下「已开始发布」，发布过的
//! 文件之后又被客户端改掉——比如 Codex 刷新了刚写的 `auth.json`——也认得出来）：
//! - 没有文件是写后内容：还没开始发布，丢弃。有文件被外部改过也一样，什么都不动；
//! - 已经开始发布：用备好的临时文件补完还是写前内容的文件，再落定状态。被外部改过
//!   （两者都不是）的文件以外部为准，不再动，状态照样落定：停在半路的话，已经发布的
//!   文件和指针、模式就对不上了。比如 Codex 已经删了 `auth.json`、登录还没写进暂存，
//!   指针还指着官方卡，下一次切换会当成用户在官方卡上登出了，把登录一起忘掉。

use std::fs;
use std::path::PathBuf;

use crate::config::{commit_staged, delete_file};
use crate::database::Database;
use crate::error::AppError;
use crate::live::engine::{
    digest, ensure_first_write_backup, plan, plan_from, read_current, stage, AppWriteGuard,
    DeviceStore, LiveFile, Planned,
};
use crate::live::patch::{LivePatch, LiveWriteError};

use super::state::{self, Pending, PendingFile, PendingTarget};

/// 发布时发现文件被改过，最多以新内容为底重算几次。
const MAX_REPLANS: usize = 3;

/// 操作里的一个文件：以当前内容为底，用 `patch` 算出新内容。
pub struct FileChange<'a> {
    pub file: LiveFile,
    pub patch: &'a dyn LivePatch,
}

/// 文件都写完之后落定状态（比如改指针）。必须可以重复执行：崩溃恢复可能再跑一次。
pub type CommitTarget<'a> = &'a dyn Fn(&PendingTarget) -> Result<(), AppError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryOutcome {
    /// 还没开始发布，丢弃了。
    Discarded,
    /// 补完了剩下的文件和状态。
    RolledForward,
    /// 补完了其余文件和状态；这些文件被外部改过（或临时文件不可用），保持原样。
    RolledForwardExcept { paths: Vec<PathBuf> },
    /// 还没开始发布，这些文件就被外部改过了：丢弃这次操作，什么都没改。
    Abandoned { paths: Vec<PathBuf> },
}

#[derive(Debug, Default)]
pub struct OperationReport {
    /// 实际改动的文件。
    pub changed: Vec<PathBuf>,
    /// 开始前补完或放弃的上一次未完成操作。
    pub recovered: Option<RecoveryOutcome>,
}

/// 执行一次操作。调用方持有这个应用的写锁。
pub fn run(
    store: &DeviceStore,
    guard: &AppWriteGuard,
    op: &str,
    changes: &[FileChange<'_>],
    target: PendingTarget,
    commit_target: CommitTarget<'_>,
) -> Result<OperationReport, AppError> {
    let mut report = OperationReport {
        recovered: recover(store, guard, commit_target)?,
        ..OperationReport::default()
    };

    // 1. 在内存里算好每个文件；任何一个解析失败都不写。
    let mut plans = Vec::with_capacity(changes.len());
    for change in changes {
        let planned = plan(&change.file, change.patch)?;
        if !planned.is_noop() {
            plans.push((planned, change.patch));
        }
    }
    if plans.is_empty() {
        commit_target(&target)?;
        return Ok(report);
    }

    // 2. 备好所有临时文件（要删的文件没有）；失败就清掉，什么都没改。
    let mut staged = Vec::with_capacity(plans.len());
    for (planned, _) in &plans {
        match stage(planned) {
            Ok(write) => staged.push(write.map(|write| write.tmp_path().to_path_buf())),
            Err(err) => {
                discard_all(&staged);
                return Err(err);
            }
        }
    }
    failpoint::hit("staged")?;

    // 3. 写下意图。从这里起，失败都留着 pending 等前滚。
    let mut pending = Pending {
        op: op.to_string(),
        files: plans
            .iter()
            .zip(&staged)
            .map(|((planned, _), staged)| pending_file(planned, staged.clone()))
            .collect(),
        target,
        published: false,
    };
    if let Err(err) = state::set_pending(store, guard.app(), Some(pending.clone())) {
        discard_all(&staged);
        return Err(err);
    }
    failpoint::hit("pending")?;

    // 4. 逐个发布：rename 前最后重读一次，被改过就以新内容为底重算。
    let mut published_any = false;
    for (index, (planned, patch)) in plans.iter().enumerate() {
        let mut current_planned = planned.clone();
        let mut replans = 0;
        loop {
            failpoint::before_publish(index, &current_planned.file.path);
            let current = read_current(&current_planned.file.path)?;
            if digest(current.as_deref()) == current_planned.pre {
                ensure_first_write_backup(store, &current_planned.file.path, current.as_deref())?;
                // 换进第一个文件之前先记下「已开始发布」，记不下来就不发布：换进去之后才记的
                // 话，中间崩溃、这个文件又被客户端改掉（Codex 刷新登录），恢复时就分不出发布
                // 开始过没有，还没发布的登录暂存会被当成没用的丢掉。
                if !pending.published {
                    pending.published = true;
                    let marked = failpoint::hit("mark").and_then(|()| {
                        state::set_pending(store, guard.app(), Some(pending.clone()))
                    });
                    if let Err(err) = marked {
                        drop_unpublished(store, guard, &pending);
                        return Err(err);
                    }
                    failpoint::hit("marked")?;
                }
                // 替换失败（文件被占用、只读）时临时文件还在：已发布过就留着 pending 等前滚，
                // 还没发布过就整体放弃（什么都没改）。
                if let Err(err) = publish(&pending.files[index]) {
                    if !published_any {
                        drop_unpublished(store, guard, &pending);
                    }
                    return Err(err);
                }
                published_any = true;
                report.changed.push(current_planned.file.path.clone());
                break;
            }

            replans += 1;
            if replans > MAX_REPLANS {
                return Err(give_up_on_conflict(
                    store,
                    guard,
                    &pending,
                    published_any,
                    &current_planned.file.path,
                ));
            }
            // 以新内容为底重算失败（新内容解析不了，或补丁拒绝在它上面改）：还没发布过
            // 任何文件就整体放弃，已发布过就留着 pending 等前滚。
            let replanned = match plan_from(&current_planned.file, *patch, current) {
                Ok(replanned) => replanned,
                Err(err) => {
                    if !published_any {
                        drop_unpublished(store, guard, &pending);
                    }
                    return Err(err.into());
                }
            };
            if replanned.is_noop() {
                // 外部写入的结果恰好就是目标内容。
                discard_staged(&pending.files[index]);
                pending.files[index] = pending_file(&replanned, None);
                state::set_pending(store, guard.app(), Some(pending.clone()))?;
                break;
            }
            let old = pending.files[index].clone();
            let new_staged = stage(&replanned)?.map(|write| write.tmp_path().to_path_buf());
            pending.files[index] = pending_file(&replanned, new_staged);
            state::set_pending(store, guard.app(), Some(pending.clone()))?;
            discard_staged(&old);
            current_planned = replanned;
        }
        failpoint::hit(&format!("published:{index}"))?;
    }

    // 5. 落定状态，再清掉意图。
    commit_target(&pending.target).map_err(|err| {
        AppError::Message(format!(
            "文件已写入，但状态更新失败，将在下次操作或启动时补完: {err}"
        ))
    })?;
    failpoint::hit("target")?;
    state::set_pending(store, guard.app(), None)?;
    Ok(report)
}

/// 补完或放弃这个应用上一次未完成的操作。调用方持有这个应用的写锁。
pub fn recover(
    store: &DeviceStore,
    guard: &AppWriteGuard,
    commit_target: CommitTarget<'_>,
) -> Result<Option<RecoveryOutcome>, AppError> {
    let Some(pending) = state::pending(store, guard.app())? else {
        return Ok(None);
    };

    enum At {
        Pre,
        Planned,
        Elsewhere,
    }
    let mut positions = Vec::with_capacity(pending.files.len());
    for file in &pending.files {
        let current = digest(read_current(&file.path)?.as_deref());
        positions.push(if current == file.planned {
            At::Planned
        } else if current == file.pre {
            At::Pre
        } else {
            At::Elsewhere
        });
    }
    let elsewhere = || -> Vec<PathBuf> {
        pending
            .files
            .iter()
            .zip(&positions)
            .filter(|(_, at)| matches!(at, At::Elsewhere))
            .map(|(file, _)| file.path.clone())
            .collect()
    };

    if !pending.published && !positions.iter().any(|at| matches!(at, At::Planned)) {
        discard_pending_files(&pending);
        state::set_pending(store, guard.app(), None)?;
        let paths = elsewhere();
        if paths.is_empty() {
            log::info!("[{}] 丢弃未开始发布的操作 {}", guard.app(), pending.op);
            return Ok(Some(RecoveryOutcome::Discarded));
        }
        log::warn!(
            "[{}] 上次未完成的操作 {} 还没开始发布，这些文件就被外部修改了，丢弃: {paths:?}",
            guard.app(),
            pending.op
        );
        return Ok(Some(RecoveryOutcome::Abandoned { paths }));
    }

    let mut skipped = elsewhere();
    for (file, at) in pending.files.iter().zip(&positions) {
        if !matches!(at, At::Pre) {
            continue;
        }
        let staged_ok = match &file.staged {
            Some(staged) => digest(read_current(staged)?.as_deref()) == file.planned,
            None => file.planned.is_none(),
        };
        if !staged_ok {
            skipped.push(file.path.clone());
            continue;
        }
        let current = read_current(&file.path)?;
        ensure_first_write_backup(store, &file.path, current.as_deref())?;
        publish(file)?;
    }
    failpoint::hit("recover:target")?;
    commit_target(&pending.target)?;
    // 发布过的临时文件已经换进去了；剩下的（被外部改过、内容不对）不再有用。
    discard_pending_files(&pending);
    state::set_pending(store, guard.app(), None)?;
    if skipped.is_empty() {
        log::info!("[{}] 已补完上次未完成的操作 {}", guard.app(), pending.op);
        return Ok(Some(RecoveryOutcome::RolledForward));
    }
    log::warn!(
        "[{}] 已补完上次未完成的操作 {}；这些文件被外部修改过或临时文件不可用，保持原样: {skipped:?}",
        guard.app(),
        pending.op
    );
    Ok(Some(RecoveryOutcome::RolledForwardExcept {
        paths: skipped,
    }))
}

/// 这个应用有没有已经发布、还没落定的操作。写入失败后用来判断要不要撤回刚存的供应商行：
/// 有的话下次操作或启动时会按它补完，行要留着。读不了状态文件按没有算。
pub(crate) fn has_pending(app: &str) -> bool {
    state::pending(&DeviceStore::for_device(), app)
        .ok()
        .flatten()
        .is_some()
}

/// 这个应用上一次操作留下的 pending 是否已经开始发布：`Some(true)` 表示下次操作或启动时
/// 会前滚补完，`Some(false)` 表示会被丢弃，`None` 表示没有 pending。操作返回错误后用来
/// 判断是「什么都没改」还是「已部分写入、待补完」。
pub(crate) fn pending_published(store: &DeviceStore, app: &str) -> Result<Option<bool>, AppError> {
    Ok(state::pending(store, app)?.map(|pending| pending.published))
}

/// 读指针、模式或「live 现在归谁」之前调用：先补完这个应用上一次没做完的操作，读到的
/// 才是落定过的状态。调用方不能持有这个应用的写锁（不可重入）；要拿代理切换锁时先拿它。
pub fn settle(db: &Database, app: &str) -> Result<Option<RecoveryOutcome>, AppError> {
    let store = DeviceStore::for_device();
    let guard = crate::live::engine::lock_app(app);
    recover(&store, &guard, &|target| {
        commit_target(db, &store, app, target)
    })
}

/// 应用的写入函数拿到写锁后、读任何文件之前调用。
///
/// 调用方在拿锁之前按指针算好了 live 现在归谁、要删哪些独有字段。这时才补完上一次的
/// 操作，指针、模式和文件可能已经变了，照旧写下去会留下上一家的独有字段、把刚补完的
/// 文件当成外部修改。所以补完过就停下，让调用方按新状态重来（入口处先调 [`settle`]
/// 的不会走到这一步）。
pub fn recover_before_write(
    store: &DeviceStore,
    guard: &AppWriteGuard,
    commit_target: CommitTarget<'_>,
) -> Result<(), AppError> {
    match recover(store, guard, commit_target)? {
        // 丢弃的操作什么都没改（指针、模式都没动）。
        None | Some(RecoveryOutcome::Discarded | RecoveryOutcome::Abandoned { .. }) => Ok(()),
        Some(outcome) => {
            log::info!("[{}] 写入前补完了上一次的操作: {outcome:?}", guard.app());
            Err(AppError::localized(
                "live.recovered_before_write",
                "上一次没做完的写入刚刚补完，当前状态已经变了。这次什么都没改，请重新操作一次",
                "An unfinished write from last time was just completed, so the current state has changed. Nothing was changed this time; please try again",
            ))
        }
    }
}

/// 一个应用的一次写入：拿着这个应用的写锁，上一次没做完的操作已经补完。各应用的写入
/// 函数都从 [`AppWrite::begin`] 开始，再按拿锁之后读到的状态算补丁。
pub struct AppWrite<'a> {
    db: &'a Database,
    pub store: DeviceStore,
    pub guard: AppWriteGuard,
}

impl<'a> AppWrite<'a> {
    /// 拿写锁，先补完上一次的操作；补完过就停下（见 [`recover_before_write`]）。
    pub fn begin(db: &'a Database, app: &str) -> Result<Self, AppError> {
        let write = Self {
            db,
            store: DeviceStore::for_device(),
            guard: crate::live::engine::lock_app(app),
        };
        recover_before_write(&write.store, &write.guard, &|target| write.commit(target))?;
        Ok(write)
    }

    fn commit(&self, target: &PendingTarget) -> Result<(), AppError> {
        commit_target(self.db, &self.store, self.guard.app(), target)
    }

    /// 执行一次操作（见 [`run`]）。
    pub fn run(
        &self,
        op: &str,
        changes: &[FileChange<'_>],
        target: PendingTarget,
    ) -> Result<OperationReport, AppError> {
        run(&self.store, &self.guard, op, changes, target, &|target| {
            self.commit(target)
        })
    }
}

/// 启动时补完所有应用未完成的操作。
pub fn recover_all(
    store: &DeviceStore,
    commit_target: &dyn Fn(&str, &PendingTarget) -> Result<(), AppError>,
) -> Vec<(String, Result<RecoveryOutcome, AppError>)> {
    let apps = match state::apps_with_pending(store) {
        Ok(apps) => apps,
        Err(err) => return vec![("*".to_string(), Err(err))],
    };
    apps.into_iter()
        .filter_map(|app| {
            let guard = crate::live::engine::lock_app(&app);
            let commit = |target: &PendingTarget| commit_target(&app, target);
            match recover(store, &guard, &commit) {
                Ok(Some(outcome)) => Some((app, Ok(outcome))),
                Ok(None) => None,
                Err(err) => Some((app, Err(err))),
            }
        })
        .collect()
}

/// 落定目标状态。必须可以重复执行（崩溃恢复可能再跑一次）。
///
/// - 直连指针：设备本地的 `current_provider_*` 和 DB 的 `is_current`，和现有切换用的是
///   同一套机制；
/// - 模式状态：写进 `live-state.json`，另把 `proxy_config.enabled` 镜像成
///   「mode == proxy」。旧版只认这一列来决定启动时是否接管，降级后才能照常工作。
pub fn commit_target(
    db: &crate::database::Database,
    store: &DeviceStore,
    app: &str,
    target: &PendingTarget,
) -> Result<(), AppError> {
    if let Some(id) = target.pointer.as_deref() {
        let app_type: crate::app_config::AppType = app.parse()?;
        crate::settings::set_current_provider(&app_type, Some(id))?;
        db.set_current_provider(app, id)?;
    }
    // 模式、写入记录和 Stack 模型在同一次状态文件写入里落定。
    if target.state.is_some() || target.written.is_some() || target.stack.is_some() {
        state::update(store, |live| {
            let entry = live.apps.entry(app.to_string()).or_default();
            if let Some(mode) = &target.state {
                entry.set_mode_state(mode.clone());
            }
            if let Some(written) = &target.written {
                entry.written = Some(written.clone());
            }
            if let Some(stack) = &target.stack {
                entry.stack = stack.clone();
            }
        })?;
    }
    if let Some(mode) = &target.state {
        mirror_proxy_flag(db, app, mode.is_proxy())?;
    }
    Ok(())
}

/// `proxy_config.enabled := (mode == proxy)`。
pub fn mirror_proxy_flag(
    db: &crate::database::Database,
    app: &str,
    proxy: bool,
) -> Result<(), AppError> {
    let (enabled, auto_failover) = db.get_proxy_flags_sync(app);
    if enabled == proxy {
        return Ok(());
    }
    db.set_proxy_flags_sync(app, proxy, auto_failover)
}

/// 启动时调用：补完上次崩溃留下的客户端文件写入。要在任何写客户端文件的启动步骤之前。
pub fn recover_on_startup(db: &crate::database::Database) {
    let store = DeviceStore::for_device();
    for (app, outcome) in recover_all(&store, &|app, target| {
        commit_target(db, &store, app, target)
    }) {
        match outcome {
            Ok(RecoveryOutcome::Abandoned { paths }) => {
                log::warn!("[{app}] 上次未完成的写入还没开始就有文件被外部修改，已丢弃: {paths:?}")
            }
            Ok(RecoveryOutcome::RolledForwardExcept { paths }) => {
                log::warn!(
                    "[{app}] 上次未完成的写入已补完，这些文件被外部修改过，保持原样: {paths:?}"
                )
            }
            Ok(outcome) => log::info!("[{app}] 上次未完成的写入: {outcome:?}"),
            Err(err) => log::error!("[{app}] 补完上次未完成的写入失败: {err}"),
        }
    }
}

fn pending_file(planned: &Planned, staged: Option<PathBuf>) -> PendingFile {
    PendingFile {
        path: planned.file.path.clone(),
        pre: planned.pre.clone(),
        planned: planned.planned.clone(),
        staged,
    }
}

/// 发布一个文件：用临时文件替换目标，或者删掉它。
fn publish(file: &PendingFile) -> Result<(), AppError> {
    match &file.staged {
        Some(staged) => commit_staged(staged, &file.path),
        None => delete_file(&file.path),
    }
}

fn discard_all(staged: &[Option<PathBuf>]) {
    for path in staged.iter().flatten() {
        let _ = fs::remove_file(path);
    }
}

fn discard_staged(file: &PendingFile) {
    if let Some(staged) = &file.staged {
        let _ = fs::remove_file(staged);
    }
}

fn discard_pending_files(pending: &Pending) {
    for file in &pending.files {
        discard_staged(file);
    }
}

/// 还没发布过任何文件时放弃：删掉临时文件和 pending，什么都没改。
fn drop_unpublished(store: &DeviceStore, guard: &AppWriteGuard, pending: &Pending) {
    discard_pending_files(pending);
    if let Err(err) = state::set_pending(store, guard.app(), None) {
        log::warn!("清除写前意图失败: {err}");
    }
}

/// 一直冲突：还没发布过任何文件就整体放弃（什么都没改）；已经发布过就留着 pending
/// 等前滚。
fn give_up_on_conflict(
    store: &DeviceStore,
    guard: &AppWriteGuard,
    pending: &Pending,
    published_any: bool,
    path: &std::path::Path,
) -> AppError {
    if !published_any {
        drop_unpublished(store, guard, pending);
    }
    LiveWriteError::Conflict {
        path: path.to_path_buf(),
    }
    .into()
}

/// 测试用的故障注入点：模拟进程在某一步崩溃（直接返回错误，不做任何清理）。
pub(crate) mod failpoint {
    #[cfg(test)]
    use std::cell::RefCell;
    use std::path::Path;

    use crate::error::AppError;

    #[cfg(test)]
    type PublishHook = Box<dyn FnMut(usize, &Path)>;

    #[cfg(test)]
    thread_local! {
        static CRASH_AT: RefCell<Option<String>> = const { RefCell::new(None) };
        static BEFORE_PUBLISH: RefCell<Option<PublishHook>> =
            const { RefCell::new(None) };
    }

    #[cfg(test)]
    pub fn crash_at(point: Option<&str>) {
        CRASH_AT.with(|slot| *slot.borrow_mut() = point.map(str::to_string));
    }

    #[cfg(test)]
    pub fn on_before_publish(hook: Option<PublishHook>) {
        BEFORE_PUBLISH.with(|slot| *slot.borrow_mut() = hook);
    }

    pub fn hit(point: &str) -> Result<(), AppError> {
        #[cfg(test)]
        if CRASH_AT.with(|slot| slot.borrow().as_deref() == Some(point)) {
            return Err(AppError::Message(format!("injected crash at {point}")));
        }
        let _ = point;
        Ok(())
    }

    pub fn before_publish(index: usize, path: &Path) {
        #[cfg(test)]
        BEFORE_PUBLISH.with(|slot| {
            if let Some(hook) = slot.borrow_mut().as_mut() {
                hook(index, path);
            }
        });
        let _ = (index, path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::engine::lock_app;
    use crate::live::patch::json::JsonPatch;
    use crate::live::patch::KeyPath;
    use serde_json::{json, Value};
    use std::cell::RefCell;
    use std::path::Path;

    struct Fixture {
        _dir: tempfile::TempDir,
        store: DeviceStore,
        a: PathBuf,
        b: PathBuf,
        app: String,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let a = dir.path().join("client/a.json");
            let b = dir.path().join("client/b.json");
            fs::create_dir_all(a.parent().unwrap()).unwrap();
            fs::write(&a, "{\n  \"user\": 1,\n  \"key\": \"old\"\n}").unwrap();
            fs::write(&b, "{\n  \"key\": \"old\"\n}").unwrap();
            // 每个测试用自己的应用名，写锁互不影响。
            let app = format!("op-test-{}", dir.path().display());
            Self {
                store: DeviceStore::at(dir.path().join("device")),
                a,
                b,
                app,
                _dir: dir,
            }
        }

        fn read(&self, path: &Path) -> Value {
            serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
        }

        fn temp_files(&self) -> Vec<PathBuf> {
            fs::read_dir(self.a.parent().unwrap())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.to_string_lossy().contains(".tmp."))
                .collect()
        }
    }

    fn set_key(value: &str) -> JsonPatch {
        JsonPatch {
            set: vec![(KeyPath::new(&["key"]), json!(value))],
            ..JsonPatch::default()
        }
    }

    fn switch(
        fx: &Fixture,
        pointer: &RefCell<Option<String>>,
    ) -> Result<OperationReport, AppError> {
        let patch = set_key("new");
        let guard = lock_app(&fx.app);
        run(
            &fx.store,
            &guard,
            state::op::SWITCH,
            &[
                FileChange {
                    file: LiveFile::shared(&fx.a),
                    patch: &patch,
                },
                FileChange {
                    file: LiveFile::shared(&fx.b),
                    patch: &patch,
                },
            ],
            PendingTarget::pointer(Some("B".into())),
            &|target| {
                *pointer.borrow_mut() = target.pointer.clone();
                Ok(())
            },
        )
    }

    fn recover_now(fx: &Fixture, pointer: &RefCell<Option<String>>) -> Option<RecoveryOutcome> {
        failpoint::crash_at(None);
        let guard = lock_app(&fx.app);
        recover(&fx.store, &guard, &|target| {
            *pointer.borrow_mut() = target.pointer.clone();
            Ok(())
        })
        .unwrap()
    }

    fn assert_old(fx: &Fixture) {
        assert_eq!(fx.read(&fx.a), json!({"user": 1, "key": "old"}));
        assert_eq!(fx.read(&fx.b), json!({"key": "old"}));
    }

    fn assert_new(fx: &Fixture) {
        assert_eq!(fx.read(&fx.a), json!({"user": 1, "key": "new"}));
        assert_eq!(fx.read(&fx.b), json!({"key": "new"}));
    }

    /// 改 a、删 b，在 `stage` 之后的某一步崩溃。
    fn write_a_delete_b(fx: &Fixture, crash: &str) -> RefCell<Option<String>> {
        let pointer = RefCell::new(None);
        let patch = set_key("new");
        let delete = crate::live::patch::WholeFile::Delete;
        failpoint::crash_at(Some(crash));
        let guard = lock_app(&fx.app);
        let result = run(
            &fx.store,
            &guard,
            state::op::SWITCH,
            &[
                FileChange {
                    file: LiveFile::shared(&fx.a),
                    patch: &patch,
                },
                FileChange {
                    file: LiveFile::shared(&fx.b),
                    patch: &delete,
                },
            ],
            PendingTarget::pointer(Some("B".into())),
            &|target| {
                *pointer.borrow_mut() = target.pointer.clone();
                Ok(())
            },
        );
        failpoint::crash_at(None);
        drop(guard);
        assert!(result.is_err(), "crash injected at {crash}");
        pointer
    }

    #[test]
    fn deleting_a_file_is_part_of_the_operation_and_rolls_forward() {
        let fx = Fixture::new();
        let pointer = write_a_delete_b(&fx, "pending");
        assert_eq!(recover_now(&fx, &pointer), Some(RecoveryOutcome::Discarded));
        assert_old(&fx);

        for crash in ["published:0", "published:1", "target"] {
            let fx = Fixture::new();
            let pointer = write_a_delete_b(&fx, crash);
            let pending = state::pending(&fx.store, &fx.app)
                .unwrap()
                .expect("pending");
            assert_eq!(pending.files[1].planned, None, "{crash}: deletion recorded");
            assert_eq!(pending.files[1].staged, None, "{crash}: nothing staged");

            assert_eq!(
                recover_now(&fx, &pointer),
                Some(RecoveryOutcome::RolledForward),
                "{crash}"
            );
            assert_eq!(fx.read(&fx.a), json!({"user": 1, "key": "new"}), "{crash}");
            assert!(!fx.b.exists(), "{crash}: b deleted");
            assert_eq!(*pointer.borrow(), Some("B".into()), "{crash}");
            assert!(fx.temp_files().is_empty(), "{crash}");
        }
    }

    #[test]
    fn deleting_a_missing_file_is_a_noop() {
        let fx = Fixture::new();
        fs::remove_file(&fx.b).unwrap();
        let guard = lock_app(&fx.app);
        let delete = crate::live::patch::WholeFile::Delete;
        let report = run(
            &fx.store,
            &guard,
            state::op::SWITCH,
            &[FileChange {
                file: LiveFile::shared(&fx.b),
                patch: &delete,
            }],
            PendingTarget::default(),
            &|_| Ok(()),
        )
        .unwrap();
        assert!(report.changed.is_empty());
        assert!(!fx.b.exists());
    }

    #[test]
    fn a_clean_run_writes_every_file_then_the_target() {
        let fx = Fixture::new();
        let pointer = RefCell::new(None);
        let report = switch(&fx, &pointer).unwrap();
        assert_new(&fx);
        assert_eq!(report.changed, vec![fx.a.clone(), fx.b.clone()]);
        assert_eq!(*pointer.borrow(), Some("B".into()));
        assert_eq!(state::pending(&fx.store, &fx.app).unwrap(), None);
        assert!(fx.temp_files().is_empty());
    }

    #[test]
    fn a_crash_before_the_intent_is_recorded_changes_nothing() {
        let fx = Fixture::new();
        let pointer = RefCell::new(None);
        failpoint::crash_at(Some("staged"));
        switch(&fx, &pointer).expect_err("crash");
        assert_eq!(recover_now(&fx, &pointer), None);
        assert_old(&fx);
        assert_eq!(*pointer.borrow(), None);
    }

    #[test]
    fn a_crash_before_publishing_is_discarded() {
        let fx = Fixture::new();
        let pointer = RefCell::new(None);
        failpoint::crash_at(Some("pending"));
        switch(&fx, &pointer).expect_err("crash");
        assert!(
            !fx.temp_files().is_empty(),
            "staged files survive the crash"
        );

        assert_eq!(recover_now(&fx, &pointer), Some(RecoveryOutcome::Discarded));
        assert_old(&fx);
        assert_eq!(*pointer.borrow(), None);
        assert!(fx.temp_files().is_empty());
        assert_eq!(state::pending(&fx.store, &fx.app).unwrap(), None);
    }

    #[test]
    fn a_crash_halfway_through_publishing_rolls_forward() {
        for point in ["published:0", "published:1", "target"] {
            let fx = Fixture::new();
            let pointer = RefCell::new(None);
            failpoint::crash_at(Some(point));
            switch(&fx, &pointer).expect_err("crash");
            *pointer.borrow_mut() = None;

            assert_eq!(
                recover_now(&fx, &pointer),
                Some(RecoveryOutcome::RolledForward),
                "{point}"
            );
            assert_new(&fx);
            assert_eq!(*pointer.borrow(), Some("B".into()), "{point}");
            assert!(fx.temp_files().is_empty(), "{point}");
            assert_eq!(state::pending(&fx.store, &fx.app).unwrap(), None);
        }
    }

    #[test]
    fn the_next_operation_finishes_a_crashed_one_first() {
        let fx = Fixture::new();
        let pointer = RefCell::new(None);
        failpoint::crash_at(Some("published:0"));
        switch(&fx, &pointer).expect_err("crash");
        failpoint::crash_at(None);

        let report = switch(&fx, &pointer).unwrap();
        assert_eq!(report.recovered, Some(RecoveryOutcome::RolledForward));
        assert_new(&fx);
    }

    #[test]
    fn a_file_changed_after_publishing_started_is_left_alone_and_the_rest_rolls_forward() {
        let fx = Fixture::new();
        let pointer = RefCell::new(None);
        failpoint::crash_at(Some("published:0"));
        switch(&fx, &pointer).expect_err("crash");
        fs::write(&fx.b, "{\"key\": \"user edit\"}").unwrap();

        assert_eq!(
            recover_now(&fx, &pointer),
            Some(RecoveryOutcome::RolledForwardExcept {
                paths: vec![fx.b.clone()]
            })
        );
        assert_eq!(fx.read(&fx.a), json!({"user": 1, "key": "new"}));
        assert_eq!(fx.read(&fx.b), json!({"key": "user edit"}));
        assert_eq!(
            *pointer.borrow(),
            Some("B".into()),
            "the target follows the files already published"
        );
        assert!(fx.temp_files().is_empty());
        assert_eq!(state::pending(&fx.store, &fx.app).unwrap(), None);
    }

    #[test]
    fn a_file_changed_before_anything_was_published_discards_the_operation() {
        let fx = Fixture::new();
        let pointer = RefCell::new(None);
        failpoint::crash_at(Some("pending"));
        switch(&fx, &pointer).expect_err("crash");
        fs::write(&fx.b, "{\"key\": \"user edit\"}").unwrap();

        assert_eq!(
            recover_now(&fx, &pointer),
            Some(RecoveryOutcome::Abandoned {
                paths: vec![fx.b.clone()]
            })
        );
        assert_eq!(fx.read(&fx.a), json!({"user": 1, "key": "old"}));
        assert_eq!(fx.read(&fx.b), json!({"key": "user edit"}));
        assert_eq!(*pointer.borrow(), None, "target is not committed");
        assert!(fx.temp_files().is_empty());
        assert_eq!(state::pending(&fx.store, &fx.app).unwrap(), None);
    }

    /// 「已开始发布」记不下来（这里模拟状态文件写失败）就不发布：什么都没改，意图也清掉。
    #[test]
    fn nothing_is_published_when_the_publish_marker_cannot_be_recorded() {
        let fx = Fixture::new();
        let pointer = RefCell::new(None);
        failpoint::crash_at(Some("mark"));
        switch(&fx, &pointer).expect_err("marker not recorded");
        failpoint::crash_at(None);

        assert_old(&fx);
        assert_eq!(*pointer.borrow(), None);
        assert!(fx.temp_files().is_empty());
        assert_eq!(state::pending(&fx.store, &fx.app).unwrap(), None);
    }

    /// 「已开始发布」在换进第一个文件之前就记下了：这之后崩溃、第一个文件又被外部改掉，
    /// 恢复时照样前滚，改掉的文件不动，其余文件的暂存内容照样发布，不会被当成「还没开始」
    /// 丢掉。
    #[test]
    fn a_file_changed_after_the_publish_marker_still_rolls_forward() {
        let fx = Fixture::new();
        let pointer = RefCell::new(None);
        failpoint::crash_at(Some("marked"));
        switch(&fx, &pointer).expect_err("crash");
        assert!(
            state::pending(&fx.store, &fx.app)
                .unwrap()
                .unwrap()
                .published
        );
        fs::write(&fx.a, "{\"user\": 1, \"key\": \"client refresh\"}").unwrap();

        assert_eq!(
            recover_now(&fx, &pointer),
            Some(RecoveryOutcome::RolledForwardExcept {
                paths: vec![fx.a.clone()]
            })
        );
        assert_eq!(fx.read(&fx.a), json!({"user": 1, "key": "client refresh"}));
        assert_eq!(fx.read(&fx.b), json!({"key": "new"}));
        assert_eq!(*pointer.borrow(), Some("B".into()));
        assert!(fx.temp_files().is_empty());
        assert_eq!(state::pending(&fx.store, &fx.app).unwrap(), None);
    }

    /// 放弃的时候不能连带丢掉还没发布的文件：Codex 删掉 `auth.json` 之后，登录只在暂存
    /// 的临时文件里。
    #[test]
    fn files_still_waiting_to_be_published_are_finished_even_if_another_file_changed() {
        let fx = Fixture::new();
        let stash = fx.store.file("stash.json");
        fs::create_dir_all(stash.parent().unwrap()).unwrap();
        fs::write(&stash, "old stash").unwrap();
        let pointer = RefCell::new(None);
        let delete = crate::live::patch::WholeFile::Delete;
        let patch = set_key("new");
        let write_stash = crate::live::patch::WholeFile::Write(b"login".to_vec());
        failpoint::crash_at(Some("published:0"));
        let guard = lock_app(&fx.app);
        let result = run(
            &fx.store,
            &guard,
            state::op::SWITCH,
            &[
                FileChange {
                    file: LiveFile::private(&fx.a),
                    patch: &delete,
                },
                FileChange {
                    file: LiveFile::shared(&fx.b),
                    patch: &patch,
                },
                FileChange {
                    file: LiveFile::private(&stash),
                    patch: &write_stash,
                },
            ],
            PendingTarget::pointer(Some("B".into())),
            &|_| Ok(()),
        );
        failpoint::crash_at(None);
        drop(guard);
        result.expect_err("crash");
        assert!(!fx.a.exists(), "the login already left a");
        fs::write(&fx.b, "{\"key\": \"client edit\"}").unwrap();

        assert_eq!(
            recover_now(&fx, &pointer),
            Some(RecoveryOutcome::RolledForwardExcept {
                paths: vec![fx.b.clone()]
            })
        );
        assert_eq!(fs::read(&stash).unwrap(), b"login");
        assert_eq!(fx.read(&fx.b), json!({"key": "client edit"}));
        assert_eq!(*pointer.borrow(), Some("B".into()));
    }

    #[test]
    fn a_missing_staged_file_is_skipped_and_the_rest_rolls_forward() {
        let fx = Fixture::new();
        let pointer = RefCell::new(None);
        failpoint::crash_at(Some("published:0"));
        switch(&fx, &pointer).expect_err("crash");
        for tmp in fx.temp_files() {
            fs::remove_file(tmp).unwrap();
        }

        assert_eq!(
            recover_now(&fx, &pointer),
            Some(RecoveryOutcome::RolledForwardExcept {
                paths: vec![fx.b.clone()]
            })
        );
        assert_eq!(fx.read(&fx.a), json!({"user": 1, "key": "new"}));
        assert_eq!(fx.read(&fx.b), json!({"key": "old"}));
        assert_eq!(*pointer.borrow(), Some("B".into()));
    }

    #[test]
    fn a_write_that_finds_an_unfinished_operation_finishes_it_and_asks_to_retry() {
        let fx = Fixture::new();
        let pointer = RefCell::new(None);
        let commit = |target: &PendingTarget| {
            *pointer.borrow_mut() = target.pointer.clone();
            Ok(())
        };
        let guard = lock_app(&fx.app);
        recover_before_write(&fx.store, &guard, &commit).expect("nothing pending");
        drop(guard);

        failpoint::crash_at(Some("published:0"));
        switch(&fx, &pointer).expect_err("crash");
        failpoint::crash_at(None);
        let guard = lock_app(&fx.app);
        let err = recover_before_write(&fx.store, &guard, &commit).expect_err("state moved");
        assert!(
            matches!(
                err,
                AppError::Localized {
                    key: "live.recovered_before_write",
                    ..
                }
            ),
            "{err}"
        );
        assert_new(&fx);
        assert_eq!(*pointer.borrow(), Some("B".into()));
        recover_before_write(&fx.store, &guard, &commit).expect("finished now");

        // 丢弃的操作什么都没改，照常往下写。
        drop(guard);
        fs::write(&fx.a, "{\"user\": 1, \"key\": \"old\"}").unwrap();
        fs::write(&fx.b, "{\"key\": \"old\"}").unwrap();
        failpoint::crash_at(Some("pending"));
        switch(&fx, &pointer).expect_err("crash");
        failpoint::crash_at(None);
        let guard = lock_app(&fx.app);
        recover_before_write(&fx.store, &guard, &commit).expect("discarded");
    }

    /// macOS 上用不可变标志让替换失败（目标被占用、只读时的样子）。
    #[cfg(target_os = "macos")]
    struct Immutable(PathBuf);

    #[cfg(target_os = "macos")]
    impl Immutable {
        fn set(path: &Path, on: bool) {
            let status = std::process::Command::new("/usr/bin/chflags")
                .arg(if on { "uchg" } else { "nouchg" })
                .arg(path)
                .status()
                .unwrap();
            assert!(status.success());
        }
    }

    #[cfg(target_os = "macos")]
    impl Drop for Immutable {
        fn drop(&mut self) {
            Self::set(&self.0, false);
        }
    }

    #[cfg(target_os = "macos")]
    fn switch_with_locked_file(fx: &Fixture, index: usize) -> RefCell<Option<String>> {
        let path = if index == 0 {
            fx.a.clone()
        } else {
            fx.b.clone()
        };
        let _unlock = Immutable(path.clone());
        let pointer = RefCell::new(None);
        failpoint::on_before_publish(Some(Box::new(move |at, _| {
            if at == index {
                Immutable::set(&path, true);
            }
        })));
        let result = switch(fx, &pointer);
        failpoint::on_before_publish(None);
        result.expect_err("the file cannot be replaced");
        pointer
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_publish_that_fails_midway_keeps_its_staged_file_and_rolls_forward_later() {
        let fx = Fixture::new();
        let pointer = switch_with_locked_file(&fx, 1);
        assert_eq!(fx.read(&fx.a), json!({"user": 1, "key": "new"}));
        assert_eq!(fx.temp_files().len(), 1, "b's staged file is kept");
        assert!(state::pending(&fx.store, &fx.app).unwrap().is_some());

        assert_eq!(
            recover_now(&fx, &pointer),
            Some(RecoveryOutcome::RolledForward)
        );
        assert_new(&fx);
        assert_eq!(*pointer.borrow(), Some("B".into()));
        assert!(fx.temp_files().is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_publish_that_fails_on_the_first_file_changes_nothing() {
        let fx = Fixture::new();
        let pointer = switch_with_locked_file(&fx, 0);
        assert_old(&fx);
        assert_eq!(*pointer.borrow(), None);
        assert_eq!(state::pending(&fx.store, &fx.app).unwrap(), None);
        assert!(fx.temp_files().is_empty());
    }

    #[test]
    fn a_concurrent_edit_is_merged_by_replanning() {
        let fx = Fixture::new();
        let pointer = RefCell::new(None);
        let b = fx.b.clone();
        let mut fired = false;
        failpoint::on_before_publish(Some(Box::new(move |index, _| {
            if index == 1 && !fired {
                fired = true;
                fs::write(&b, "{\n  \"key\": \"old\",\n  \"added\": true\n}").unwrap();
            }
        })));
        let result = switch(&fx, &pointer);
        failpoint::on_before_publish(None);

        result.unwrap();
        assert_eq!(fx.read(&fx.b), json!({"key": "new", "added": true}));
        assert_eq!(*pointer.borrow(), Some("B".into()));
        assert!(fx.temp_files().is_empty());
    }

    #[test]
    fn a_file_that_keeps_changing_before_anything_is_published_changes_nothing() {
        let fx = Fixture::new();
        let pointer = RefCell::new(None);
        let a = fx.a.clone();
        let mut round = 0;
        failpoint::on_before_publish(Some(Box::new(move |index, _| {
            if index == 0 {
                round += 1;
                fs::write(&a, format!("{{\"user\": {round}, \"key\": \"old\"}}")).unwrap();
            }
        })));
        let result = switch(&fx, &pointer);
        failpoint::on_before_publish(None);

        assert!(matches!(result, Err(AppError::Conflict(_))), "{result:?}");
        assert_eq!(fx.read(&fx.b), json!({"key": "old"}));
        assert_eq!(*pointer.borrow(), None);
        assert_eq!(state::pending(&fx.store, &fx.app).unwrap(), None);
        assert!(fx.temp_files().is_empty());
    }

    #[test]
    fn a_broken_file_stops_the_whole_operation_up_front() {
        let fx = Fixture::new();
        let pointer = RefCell::new(None);
        fs::write(&fx.b, "{ broken").unwrap();
        let err = switch(&fx, &pointer).expect_err("refused");
        assert!(err.to_string().contains("b.json"), "{err}");
        assert_eq!(fx.read(&fx.a), json!({"user": 1, "key": "old"}));
        assert_eq!(fs::read_to_string(&fx.b).unwrap(), "{ broken");
        assert_eq!(*pointer.borrow(), None);
        assert!(fx.temp_files().is_empty());
    }

    #[test]
    fn every_file_is_backed_up_once_before_its_first_write() {
        let fx = Fixture::new();
        let pointer = RefCell::new(None);
        switch(&fx, &pointer).unwrap();
        let backups: Vec<Vec<u8>> = fs::read_dir(fx.store.first_write_backup_dir())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| !path.to_string_lossy().ends_with(".source"))
            .map(|path| fs::read(path).unwrap())
            .collect();
        assert_eq!(backups.len(), 2);
        assert!(backups.contains(&b"{\n  \"user\": 1,\n  \"key\": \"old\"\n}".to_vec()));
    }
}
