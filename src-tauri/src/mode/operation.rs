//! 一次写客户端文件的操作：按文件记录写前意图（pending），逐个发布，崩溃后前滚或丢弃。
//!
//! 把一次切换当成一个操作来提交：
//! - 发布前的失败（解析失败、路由校验不通过、并发冲突）什么都不改；
//! - 一旦开始发布，就以 pending 为准前滚补完。写到一半失败或进程崩溃，下次操作这个
//!   应用或下次启动时按 pending 补完剩下的文件和状态（指针等）。
//!
//! 只调换「先写文件、后改指针」的顺序不够：文件写成 B、指针更新失败，照样不一致。
//! 恢复规则（按文件比对当前内容和写前、写后的 hash）：
//! - 每个文件都还是写前内容：还没开始发布，丢弃；
//! - 每个文件都是写前或写后内容：前滚，用备好的临时文件补完剩下的文件，再落定状态；
//! - 有文件两者都不是：已被外部修改，放弃这次操作，不自动处理。

use std::fs;
use std::path::PathBuf;

use crate::config::commit_staged;
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
    /// 文件被外部改过（或临时文件丢了），放弃这次操作。
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

    // 2. 备好所有临时文件；失败就清掉，什么都没改。
    let mut staged = Vec::with_capacity(plans.len());
    for (planned, _) in &plans {
        match stage(planned) {
            Ok(write) => staged.push(write.tmp_path().to_path_buf()),
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
                commit_staged(&pending.files[index].staged, &current_planned.file.path)?;
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
            let replanned = plan_from(&current_planned.file, *patch, current)?;
            if replanned.is_noop() {
                // 外部写入的结果恰好就是目标内容。
                let _ = fs::remove_file(&pending.files[index].staged);
                pending.files[index] =
                    pending_file(&replanned, pending.files[index].staged.clone());
                state::set_pending(store, guard.app(), Some(pending.clone()))?;
                break;
            }
            let old_staged = pending.files[index].staged.clone();
            let new_staged = stage(&replanned)?.tmp_path().to_path_buf();
            pending.files[index] = pending_file(&replanned, new_staged);
            state::set_pending(store, guard.app(), Some(pending.clone()))?;
            let _ = fs::remove_file(old_staged);
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
        positions.push(if current.as_deref() == Some(file.planned.as_str()) {
            At::Planned
        } else if current == file.pre {
            At::Pre
        } else {
            At::Elsewhere
        });
    }

    let changed_elsewhere: Vec<PathBuf> = pending
        .files
        .iter()
        .zip(&positions)
        .filter(|(_, at)| matches!(at, At::Elsewhere))
        .map(|(file, _)| file.path.clone())
        .collect();
    if !changed_elsewhere.is_empty() {
        return abandon(store, guard, &pending, changed_elsewhere);
    }

    if positions.iter().all(|at| matches!(at, At::Pre)) {
        discard_pending_files(&pending);
        state::set_pending(store, guard.app(), None)?;
        log::info!("[{}] 丢弃未开始发布的操作 {}", guard.app(), pending.op);
        return Ok(Some(RecoveryOutcome::Discarded));
    }

    for (file, at) in pending.files.iter().zip(&positions) {
        if !matches!(at, At::Pre) {
            continue;
        }
        let staged = read_current(&file.staged)?;
        if digest(staged.as_deref()).as_deref() != Some(file.planned.as_str()) {
            return abandon(store, guard, &pending, vec![file.path.clone()]);
        }
        let current = read_current(&file.path)?;
        ensure_first_write_backup(store, &file.path, current.as_deref())?;
        commit_staged(&file.staged, &file.path)?;
    }
    commit_target(&pending.target)?;
    state::set_pending(store, guard.app(), None)?;
    log::info!("[{}] 已补完上次未完成的操作 {}", guard.app(), pending.op);
    Ok(Some(RecoveryOutcome::RolledForward))
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

/// 落定直连指针：设备本地的 `current_provider_*` 和 DB 的 `is_current`，和现有切换
/// 用的是同一套机制。
pub fn commit_pointer(
    db: &crate::database::Database,
    app: &str,
    target: &PendingTarget,
) -> Result<(), AppError> {
    let Some(id) = target.pointer.as_deref() else {
        return Ok(());
    };
    let app_type: crate::app_config::AppType = app.parse()?;
    crate::settings::set_current_provider(&app_type, Some(id))?;
    db.set_current_provider(app, id)
}

/// 启动时调用：补完上次崩溃留下的客户端文件写入。要在任何写客户端文件的启动步骤之前。
pub fn recover_on_startup(db: &crate::database::Database) {
    let store = DeviceStore::for_device();
    for (app, outcome) in recover_all(&store, &|app, target| commit_pointer(db, app, target)) {
        match outcome {
            Ok(RecoveryOutcome::Abandoned { paths }) => {
                log::warn!("[{app}] 上次未完成的写入无法补完，这些文件已被外部修改: {paths:?}")
            }
            Ok(outcome) => log::info!("[{app}] 上次未完成的写入: {outcome:?}"),
            Err(err) => log::error!("[{app}] 补完上次未完成的写入失败: {err}"),
        }
    }
}

fn pending_file(planned: &Planned, staged: PathBuf) -> PendingFile {
    PendingFile {
        path: planned.file.path.clone(),
        pre: planned.pre.clone(),
        planned: planned.planned.clone(),
        staged,
    }
}

fn discard_all(staged: &[PathBuf]) {
    for path in staged {
        let _ = fs::remove_file(path);
    }
}

fn discard_pending_files(pending: &Pending) {
    for file in &pending.files {
        let _ = fs::remove_file(&file.staged);
    }
}

fn abandon(
    store: &DeviceStore,
    guard: &AppWriteGuard,
    pending: &Pending,
    paths: Vec<PathBuf>,
) -> Result<Option<RecoveryOutcome>, AppError> {
    discard_pending_files(pending);
    state::set_pending(store, guard.app(), None)?;
    log::warn!(
        "[{}] 上次未完成的操作 {} 无法补完，这些文件已被外部修改: {:?}",
        guard.app(),
        pending.op,
        paths
    );
    Ok(Some(RecoveryOutcome::Abandoned { paths }))
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
        discard_pending_files(pending);
        if let Err(err) = state::set_pending(store, guard.app(), None) {
            log::warn!("清除写前意图失败: {err}");
        }
    }
    LiveWriteError::Conflict {
        path: path.to_path_buf(),
    }
    .into()
}

/// 测试用的故障注入点：模拟进程在某一步崩溃（直接返回错误，不做任何清理）。
mod failpoint {
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
            PendingTarget {
                pointer: Some("B".into()),
                ..PendingTarget::default()
            },
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
    fn a_file_changed_after_a_crash_is_left_alone() {
        let fx = Fixture::new();
        let pointer = RefCell::new(None);
        failpoint::crash_at(Some("published:0"));
        switch(&fx, &pointer).expect_err("crash");
        fs::write(&fx.b, "{\"key\": \"user edit\"}").unwrap();

        assert_eq!(
            recover_now(&fx, &pointer),
            Some(RecoveryOutcome::Abandoned {
                paths: vec![fx.b.clone()]
            })
        );
        assert_eq!(fx.read(&fx.b), json!({"key": "user edit"}));
        assert_eq!(*pointer.borrow(), None, "target is not committed");
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
