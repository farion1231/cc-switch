//! 写 Claude Code 的 `settings.json`：只替换关键字段和独有字段，其余字节不碰。
//!
//! 写 Claude live 的入口（切换、新增第一个供应商、编辑当前供应商、同步、统一供应商、
//! 进入 / 退出代理）都走这里：先拿应用写锁，再经 `mode::operation` 记下 pending、发布。
//! 不回填、不合并通用配置片段、不注入上下文默认值：用户的设置本来就留在 live 里。

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::app_config::AppType;
use crate::config::get_claude_settings_path;
use crate::database::Database;
use crate::error::AppError;
use crate::live::engine::{AppWriteGuard, DeviceStore, LiveFile};
use crate::live::patch::json::JsonPatch;
use crate::live::patch::KeyPath;
use crate::live::project::claude::{
    direct_patch, tombstone_keys, tombstone_sweep, with_tombstones, ClaudeProjection,
};
use crate::mode::operation::{AppWrite, FileChange, OperationReport};
use crate::mode::state::{op, PendingTarget};
use crate::provider::Provider;

fn app() -> &'static str {
    AppType::Claude.as_str()
}

/// `settings.json`（旧安装可能是 `claude.json`）。里面有 Key，按 0600 写。
pub(crate) fn settings_file() -> LiveFile {
    LiveFile::private(get_claude_settings_path())
}

/// 现在的 `settings.json`。不存在、读不了、解析不了都当空对象（读不了、解析不了的
/// 写入本身也会失败）。
fn read_settings() -> Value {
    let path = get_claude_settings_path();
    std::fs::read(&path)
        .ok()
        .and_then(|bytes| crate::live::patch::json::parse(&path, Some(&bytes)).ok())
        .map_or_else(|| Value::Object(Default::default()), |(doc, _)| doc)
}

/// 从 `prev` 切到 `target`：同一个操作里写 live、再把当前供应商改成 `target`。
///
/// `prev` 是 live 现在对应的供应商（直连指针指向的那家），用来删它带进来的独有字段。
pub(crate) fn switch_to(
    db: &Database,
    prev: Option<&Provider>,
    target: &Provider,
) -> Result<OperationReport, AppError> {
    write(db, prev, target, Some(&target.id))
}

/// 把当前供应商 `target` 重新投影到 live，不改指针。`prev` 是 live 现在对应的那一版
/// 行（编辑前的行；没改过就是它自己）。
pub(crate) fn reapply(
    db: &Database,
    prev: Option<&Provider>,
    target: &Provider,
) -> Result<OperationReport, AppError> {
    write(db, prev, target, None)
}

fn write(
    db: &Database,
    prev: Option<&Provider>,
    target: &Provider,
    pointer: Option<&str>,
) -> Result<OperationReport, AppError> {
    let prev = prev.map(|provider| ClaudeProjection::of(&provider.settings_config));
    let mut patch = direct_patch(
        prev.as_ref(),
        &ClaudeProjection::of(&target.settings_config),
    );
    // 删掉以前开发版写进去的账号目录（见 claude_official）
    super::claude_official::strip_from_patch(&mut patch);
    // 先换登录（卡绑了本机账号、没设配置目录覆盖时）：失败就整个不切，指针和
    // settings.json 都不动；登录换了、后面写失败时再换回去
    super::claude_official::with_login_switch(Some(target), || {
        run(
            db,
            if pointer.is_some() {
                op::SWITCH
            } else {
                op::APPLY
            },
            Some(&patch),
            PendingTarget::pointer(pointer.map(str::to_string)),
        )
    })
}

/// 用 `patch` 改写 `settings.json`，和 `target` 在同一个操作里提交；`patch` 为空时只
/// 落定状态、不读也不写文件。
pub(crate) fn run(
    db: &Database,
    op: &str,
    patch: Option<&JsonPatch>,
    target: PendingTarget,
) -> Result<OperationReport, AppError> {
    let patch = patch.map(|patch| for_live(op, patch));
    // 补丁是按调用方读到的指针算的（要删上一家的独有字段）：先补完上一次的操作。
    let write = AppWrite::begin(db, app())?;
    let tombstones = note_tombstones(&write.store, patch.as_ref());
    let changes: Vec<FileChange<'_>> = patch
        .iter()
        .map(|patch| FileChange {
            file: settings_file(),
            patch,
        })
        .collect();
    let result = write.run(op, &changes, target);
    after_live_write(
        &write.guard,
        patch.as_ref(),
        result.as_ref().ok(),
        tombstones,
    );
    result
}

/// 正在运行的 Claude Code 能跟着切换的补丁：删掉的连接字段、独有字段先写成 `""`（见
/// [`with_tombstones`]）。
///
/// 退出 CC Switch 时的分离不加：进程马上退出，开着的会话这时指着的是马上要停的代理，
/// 照旧删键，下次启动接管时再写。
pub(crate) fn for_live(op: &str, patch: &JsonPatch) -> JsonPatch {
    if op == op::DETACH {
        return patch.clone();
    }
    with_tombstones(patch.clone())
}

/// 墓碑在文件里留多久：Claude Code 的 settings 监听等写入稳定 1 秒、每 500 毫秒查一次
/// （2.1.318），开着的会话 2 秒内就会读到。留足余量，忙着的会话晚一点读也赶得上。
const TOMBSTONE_TTL: Duration = Duration::from_secs(10);

/// 退出 CC Switch 时墓碑至少要留多久才删：开着的会话来得及读到就行，不让退出等满
/// [`TOMBSTONE_TTL`]。
const TOMBSTONE_MIN_VISIBLE: Duration = Duration::from_secs(3);

/// 每次写 Claude live 加一。删墓碑前核对：之后又写过就不删，由最后那次写入删。
static LIVE_WRITES: AtomicU64 = AtomicU64::new(0);

/// 还没删的墓碑记在这台设备的状态目录里：CC Switch 在墓碑删掉之前退出或崩溃，下次启动时
/// 接着删，空串不会一直留在 `settings.json` 里。
const TOMBSTONE_MARKER: &str = "claude-tombstones.json";

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TombstoneMarker {
    /// 可能是墓碑的 `env` 键：删的时候只删值还是 `""` 的。
    keys: Vec<String>,
    /// 最近一次写进墓碑的时间（Unix 毫秒）。
    written_at_ms: u64,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

fn read_marker(store: &DeviceStore) -> Option<TombstoneMarker> {
    let bytes = std::fs::read(store.file(TOMBSTONE_MARKER)).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(marker) => Some(marker),
        Err(error) => {
            log::warn!("[Claude] 墓碑记录读不了，按没有处理: {error}");
            None
        }
    }
}

fn remove_marker(store: &DeviceStore) {
    if let Err(error) = std::fs::remove_file(store.file(TOMBSTONE_MARKER)) {
        if error.kind() != std::io::ErrorKind::NotFound {
            log::warn!("[Claude] 删除墓碑记录失败: {error}");
        }
    }
}

/// 写 live 之前（拿着写锁）：这次可能写进的墓碑和还没删的旧墓碑合在一起记下来，返回写完
/// 之后要删的键。先记后写：写到一半崩溃的话，启动时补完的文件里的墓碑也有记录。
///
/// 这次写入要写的键不算（目标卡自己就设成了 `""` 的不删）。没有要删的键时返回 `None`。
pub(crate) fn note_tombstones(
    store: &DeviceStore,
    patch: Option<&JsonPatch>,
) -> Option<Vec<String>> {
    let patch = patch?;
    let fresh = if patch.tombstone.is_empty() {
        Vec::new()
    } else {
        // 拿着写锁、上一次操作已补完：这就是这次写入的写前内容
        tombstone_keys(patch, &read_settings())
    };
    let old = read_marker(store);
    if fresh.is_empty() && old.is_none() {
        return None;
    }
    let env = KeyPath::new(&["env"]);
    let set: HashSet<&KeyPath> = patch.set.iter().map(|(path, _)| path).collect();
    let mut keys: Vec<String> = Vec::new();
    for key in old.iter().flat_map(|old| old.keys.iter()).chain(&fresh) {
        if !set.contains(&env.child(key)) && !keys.contains(key) {
            keys.push(key.clone());
        }
    }
    if keys.is_empty() {
        remove_marker(store);
        return None;
    }
    // 这次没有新墓碑时沿用旧的时间：退出时按它算墓碑留够了没有
    let written_at_ms = match (&old, fresh.is_empty()) {
        (Some(old), true) => old.written_at_ms,
        _ => now_ms(),
    };
    let marker = TombstoneMarker {
        keys: keys.clone(),
        written_at_ms,
    };
    if let Err(error) = crate::config::write_json_file(&store.file(TOMBSTONE_MARKER), &marker) {
        // 记不下来照样切：几秒后的删除不靠它，只是退出、崩溃后没法接着删
        log::warn!("[Claude] 记录墓碑失败，CC Switch 提前退出时可能留下空值: {error}");
    }
    Some(keys)
}

/// 写完 `settings.json` 之后（还拿着写锁）：记下这次写入，有墓碑就安排几秒后删掉。
/// `patch` 是这次写入的关键字段补丁（`None` 表示没动关键字段），`report` 为 `None` 表示
/// 这次写入失败了，`tombstones` 是 [`note_tombstones`] 记下的键。
///
/// 计数在写锁里加：更早安排的删除要拿到写锁才核对，一定看得到这次写入，不会删掉这次
/// 刚写、开着的会话还没读到的墓碑。失败的写入也算：它可能已经发布了一部分、之后由补完
/// 前滚，更早的删除不能再按自己那次的目标去删（墓碑记录还在，退出或下次启动时删）。
/// 成功、但没动关键字段的写入（编辑器里只改了别的设置）不算：它不写墓碑，目标卡也没变，
/// 更早安排的删除照常做。
pub(crate) fn after_live_write(
    _guard: &AppWriteGuard,
    patch: Option<&JsonPatch>,
    report: Option<&OperationReport>,
    tombstones: Option<Vec<String>>,
) {
    if report.is_some_and(|report| report.changed.is_empty() || patch.is_none()) {
        return;
    }
    let generation = LIVE_WRITES.fetch_add(1, Ordering::SeqCst) + 1;
    if report.is_none() {
        return;
    }
    let Some(keys) = tombstones else {
        return;
    };
    // 文件和状态目录在写入这一刻定下来，不在几秒后按当时的环境重新解析
    schedule(Sweep {
        generation: Some(generation),
        due: Instant::now() + TOMBSTONE_TTL,
        file: settings_file(),
        store: DeviceStore::for_device(),
        keys,
    });
}

/// 一次待删的墓碑。`generation` 是写进它们的那次写入（`None`：不核对，退出时用），
/// `keys` 是那次记下的键（墓碑记录读不到时用）。
struct Sweep {
    generation: Option<u64>,
    #[cfg_attr(test, allow(dead_code))]
    due: Instant,
    file: LiveFile,
    store: DeviceStore,
    keys: Vec<String>,
}

/// 安排下、还没做的删除。到期的后台线程只做还在这里的那一笔；
/// [`settle_claude_tombstones`] 把它们全部提前做掉，到期的线程就什么都不做。
static SCHEDULED: Mutex<Vec<(u64, Sweep)>> = Mutex::new(Vec::new());
static NEXT_SWEEP: AtomicU64 = AtomicU64::new(0);

fn scheduled() -> std::sync::MutexGuard<'static, Vec<(u64, Sweep)>> {
    SCHEDULED.lock().unwrap_or_else(|e| e.into_inner())
}

/// 取走所有安排下的删除（不做）。
fn take_scheduled() -> Vec<Sweep> {
    std::mem::take(&mut *scheduled())
        .into_iter()
        .map(|(_, sweep)| sweep)
        .collect()
}

fn schedule(sweep: Sweep) {
    let id = NEXT_SWEEP.fetch_add(1, Ordering::SeqCst);
    #[cfg(not(test))]
    let due = sweep.due;
    scheduled().push((id, sweep));
    // 单测里不起后台线程（几秒后它会写到别的测试的临时目录里）：由测试自己调
    // [`sweep_tombstones`] 或 [`settle_claude_tombstones`]
    #[cfg(not(test))]
    {
        let spawned = std::thread::Builder::new()
            .name("claude-tombstone-sweep".into())
            .spawn(move || {
                std::thread::sleep(due.saturating_duration_since(Instant::now()));
                let sweep = {
                    let mut queue = scheduled();
                    let Some(index) = queue.iter().position(|(queued, _)| *queued == id) else {
                        return;
                    };
                    queue.remove(index).1
                };
                if let Err(error) = sweep_tombstones(&sweep) {
                    // 删不掉也无害：Claude Code 读到空串等于没设；记录还在，退出或下次启动时再删
                    log::warn!("[Claude] 删除切换留下的空值失败，稍后再删: {error}");
                }
            });
        if let Err(error) = spawned {
            scheduled().retain(|(queued, _)| *queued != id);
            log::warn!("[Claude] 无法安排删除切换留下的空值，退出或下次启动时再删: {error}");
        }
    }
}

/// 不等有效期，把安排下的删除都做完，之后读到的就是落定的 `settings.json`。
/// 给测试用（集成测试切换后马上读文件）；正常运行时不调。
#[doc(hidden)]
pub fn settle_claude_tombstones() -> Result<(), AppError> {
    for sweep in take_scheduled() {
        sweep_tombstones(&sweep)?;
    }
    Ok(())
}

/// 单测用：同 [`settle_claude_tombstones`]，失败直接报。
#[cfg(test)]
pub(crate) fn settle_tombstones() {
    settle_claude_tombstones().expect("sweep tombstones");
}

/// 启动时（补完上次没做完的操作之后）：上次退出或崩溃前没删的墓碑，等到有效期满接着删。
pub(crate) fn resume_tombstone_sweep() {
    let store = DeviceStore::for_device();
    let Some(marker) = read_marker(&store) else {
        return;
    };
    let elapsed = Duration::from_millis(now_ms().saturating_sub(marker.written_at_ms));
    log::info!("[Claude] 上次留下的空值墓碑稍后删除: {:?}", marker.keys);
    schedule(Sweep {
        // 之后的写入会把这些键并进它自己的记录、自己安排删除
        generation: Some(LIVE_WRITES.load(Ordering::SeqCst)),
        due: Instant::now() + TOMBSTONE_TTL.saturating_sub(elapsed),
        file: settings_file(),
        store,
        keys: marker.keys,
    });
}

/// 退出 CC Switch 时（指回直连之后）：还没删的墓碑现在删掉，不留到下次启动。刚写进去不到
/// [`TOMBSTONE_MIN_VISIBLE`] 的，先等开着的会话读到（最多等这么久）。会阻塞，在阻塞线程上调。
pub(crate) fn finish_tombstones_before_exit() {
    let store = DeviceStore::for_device();
    let Some(marker) = read_marker(&store) else {
        return;
    };
    let elapsed = Duration::from_millis(now_ms().saturating_sub(marker.written_at_ms));
    std::thread::sleep(TOMBSTONE_MIN_VISIBLE.saturating_sub(elapsed));
    let sweep = Sweep {
        generation: None,
        due: Instant::now(),
        file: settings_file(),
        store,
        keys: marker.keys,
    };
    match sweep_tombstones(&sweep) {
        Ok(()) => log::info!("[Claude] 退出前删掉了切换留下的空值"),
        Err(error) => log::warn!("[Claude] 退出前删除切换留下的空值失败，下次启动时再删: {error}"),
    }
}

/// 删掉墓碑。和别的写入一样拿写锁、走操作日志；这期间又写过 live（计数变了）、或上一次
/// 操作还没补完，就什么都不做，交给之后的写入、退出或启动。删完清掉墓碑记录。
///
/// 删哪些键以拿锁之后读到的记录为准（之后的写入会把键并进去）；读不到记录就用安排时的键。
/// 不改指针、模式这些状态（目标为空），所以不需要数据库：补完别的操作要用数据库，
/// 遇到没补完的操作直接让开。
fn sweep_tombstones(sweep: &Sweep) -> Result<(), AppError> {
    let guard = crate::live::engine::lock_app(app());
    if sweep
        .generation
        .is_some_and(|generation| LIVE_WRITES.load(Ordering::SeqCst) != generation)
    {
        return Ok(());
    }
    if crate::mode::state::pending(&sweep.store, app())?.is_some() {
        return Ok(());
    }
    let keys = read_marker(&sweep.store).map_or_else(|| sweep.keys.clone(), |marker| marker.keys);
    if sweep.file.path.exists() {
        let patch = tombstone_sweep(&keys);
        let changes = [FileChange {
            file: sweep.file.clone(),
            patch: &patch,
        }];
        crate::mode::operation::run(
            &sweep.store,
            &guard,
            op::APPLY,
            &changes,
            PendingTarget::default(),
            &|_| Ok(()),
        )?;
    }
    remove_marker(&sweep.store);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use serial_test::serial;

    struct TestHome {
        _dir: tempfile::TempDir,
        original: Option<std::ffi::OsString>,
    }

    impl TestHome {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("temp home");
            let original = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            crate::settings::reload_settings().expect("reload settings");
            take_scheduled();
            Self {
                _dir: dir,
                original,
            }
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            match &self.original {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            let _ = crate::settings::reload_settings();
        }
    }

    fn write_live(value: &Value) {
        let path = get_claude_settings_path();
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, serde_json::to_string_pretty(value).unwrap()).expect("write live");
    }

    fn read_live() -> Value {
        serde_json::from_slice(&std::fs::read(get_claude_settings_path()).expect("read live"))
            .expect("live json")
    }

    fn provider(id: &str, category: &str, settings: Value) -> Provider {
        let mut provider = Provider::with_id(id.into(), id.into(), settings, None);
        provider.category = Some(category.into());
        provider
    }

    fn custom() -> Provider {
        provider(
            "custom",
            "custom",
            json!({ "env": {
                "ANTHROPIC_BASE_URL": "https://gateway.example",
                "ANTHROPIC_AUTH_TOKEN": "sk-custom",
                "ANTHROPIC_MODEL": "gateway-model",
                "AWS_REGION": "us-east-1"
            }}),
        )
    }

    fn official() -> Provider {
        provider("official", "official", json!({ "env": {} }))
    }

    /// 第三方卡正在用时的 live，带一个用户自己的键。
    fn custom_live() -> Value {
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://gateway.example",
                "ANTHROPIC_AUTH_TOKEN": "sk-custom",
                "ANTHROPIC_MODEL": "gateway-model",
                "AWS_REGION": "us-east-1",
                "API_TIMEOUT_MS": "300000"
            },
            "permissions": { "allow": ["Bash"] }
        })
    }

    #[test]
    #[serial]
    fn switching_to_official_blanks_routing_keys_then_removes_them() {
        let _home = TestHome::new();
        let db = Database::memory().unwrap();
        write_live(&custom_live());

        switch_to(&db, Some(&custom()), &official()).expect("switch");

        // 开着的会话读到空串，立刻丢掉第三方的地址、凭据和模型
        let live = read_live();
        assert_eq!(
            live["env"],
            json!({
                "ANTHROPIC_BASE_URL": "",
                "ANTHROPIC_AUTH_TOKEN": "",
                "ANTHROPIC_MODEL": "",
                "API_TIMEOUT_MS": "300000"
            }),
            "routing keys are blanked in place; AWS_* is still deleted"
        );
        assert_eq!(live["permissions"], json!({ "allow": ["Bash"] }));

        // 几秒后删掉墓碑：文件和直接删掉一样
        let scheduled = take_scheduled();
        assert_eq!(scheduled.len(), 1);
        sweep_tombstones(&scheduled[0]).expect("sweep");
        assert_eq!(
            read_live(),
            json!({
                "env": { "API_TIMEOUT_MS": "300000" },
                "permissions": { "allow": ["Bash"] }
            })
        );
    }

    #[test]
    #[serial]
    fn a_later_write_takes_over_the_earlier_sweep() {
        let _home = TestHome::new();
        let db = Database::memory().unwrap();
        write_live(&custom_live());

        switch_to(&db, Some(&custom()), &official()).expect("to official");
        let first = take_scheduled();
        // 墓碑还没删就切回第三方：这次写入的结果不能被上一次安排的删除动
        switch_to(&db, Some(&official()), &custom()).expect("back to custom");
        let before = read_live();
        sweep_tombstones(&first[0]).expect("stale sweep is a no-op");
        assert_eq!(read_live(), before);
        assert_eq!(
            before["env"]["ANTHROPIC_BASE_URL"],
            json!("https://gateway.example")
        );
    }

    #[test]
    #[serial]
    fn sweep_keeps_values_written_after_the_tombstone() {
        let _home = TestHome::new();
        let db = Database::memory().unwrap();
        write_live(&custom_live());

        switch_to(&db, Some(&custom()), &official()).expect("switch");
        // 用户（或别的工具）在这几秒里自己把地址写回了一个真值：不删
        let mut live = read_live();
        live["env"]["ANTHROPIC_BASE_URL"] = json!("https://user.example");
        write_live(&live);

        let scheduled = take_scheduled();
        sweep_tombstones(&scheduled[0]).expect("sweep");
        assert_eq!(
            read_live()["env"],
            json!({
                "ANTHROPIC_BASE_URL": "https://user.example",
                "API_TIMEOUT_MS": "300000"
            })
        );
    }

    #[test]
    #[serial]
    fn detach_deletes_without_tombstones() {
        let _home = TestHome::new();
        let db = Database::memory().unwrap();
        write_live(&custom_live());

        let patch = direct_patch(
            Some(&ClaudeProjection::of(&custom().settings_config)),
            &ClaudeProjection::of(&official().settings_config),
        );
        run(&db, op::DETACH, Some(&patch), PendingTarget::default()).expect("detach");

        // 退出 CC Switch 时没人删墓碑：照旧直接删，不留空串
        assert_eq!(read_live()["env"], json!({ "API_TIMEOUT_MS": "300000" }));
        assert!(take_scheduled().is_empty());
    }

    #[test]
    #[serial]
    fn switches_that_blank_nothing_record_and_schedule_nothing() {
        let _home = TestHome::new();
        let db = Database::memory().unwrap();
        write_live(&json!({ "env": {} }));

        // 官方 → 第三方：没有要删的连接字段，没有墓碑，也不记、不排删除（退出时不用等）
        switch_to(&db, Some(&official()), &custom()).expect("switch");
        let live = read_live();
        assert!(live["env"]
            .as_object()
            .unwrap()
            .values()
            .all(|value| value != ""));
        assert!(marker().is_none());
        assert!(take_scheduled().is_empty());

        // 已经是官方再切一次官方：没有值可撤，同样什么都不记
        write_live(&json!({ "env": { "API_TIMEOUT_MS": "300000" } }));
        switch_to(&db, Some(&official()), &official()).expect("official again");
        assert!(marker().is_none());
        assert!(take_scheduled().is_empty());
    }

    fn marker() -> Option<TombstoneMarker> {
        read_marker(&DeviceStore::for_device())
    }

    #[test]
    #[serial]
    fn switching_records_tombstones_and_the_sweep_clears_the_record() {
        let _home = TestHome::new();
        let db = Database::memory().unwrap();
        write_live(&custom_live());

        switch_to(&db, Some(&custom()), &official()).expect("switch");
        let recorded = marker().expect("tombstones recorded before the write");
        assert!(recorded.keys.contains(&"ANTHROPIC_BASE_URL".to_string()));
        assert!(!recorded.keys.iter().any(|key| key == "AWS_REGION"));

        settle_tombstones();
        assert!(marker().is_none(), "record is cleared once the sweep ran");
    }

    #[test]
    #[serial]
    fn startup_finishes_tombstones_left_by_the_last_run() {
        let _home = TestHome::new();
        let db = Database::memory().unwrap();
        write_live(&custom_live());
        switch_to(&db, Some(&custom()), &official()).expect("switch");
        // 上次运行在删除之前就退出了：安排下的删除没了，只剩文件里的空值和记录
        take_scheduled();
        assert_eq!(read_live()["env"]["ANTHROPIC_BASE_URL"], json!(""));

        resume_tombstone_sweep();
        settle_tombstones();
        assert_eq!(read_live()["env"], json!({ "API_TIMEOUT_MS": "300000" }));
        assert!(marker().is_none());
    }

    #[test]
    #[serial]
    fn exit_finishes_tombstones_without_waiting_once_they_were_visible() {
        let _home = TestHome::new();
        let db = Database::memory().unwrap();
        write_live(&custom_live());
        switch_to(&db, Some(&custom()), &official()).expect("switch");
        take_scheduled();
        // 墓碑早就写进去了（开着的会话读到过）：退出时不用等
        let store = DeviceStore::for_device();
        let mut recorded = marker().unwrap();
        recorded.written_at_ms = 0;
        crate::config::write_json_file(&store.file(TOMBSTONE_MARKER), &recorded).unwrap();

        let started = Instant::now();
        finish_tombstones_before_exit();
        assert!(started.elapsed() < TOMBSTONE_MIN_VISIBLE);
        assert_eq!(read_live()["env"], json!({ "API_TIMEOUT_MS": "300000" }));
        assert!(marker().is_none());
    }

    #[test]
    #[serial]
    fn an_unswept_record_is_carried_into_the_next_write() {
        let _home = TestHome::new();
        let db = Database::memory().unwrap();
        let mut gateway = custom();
        gateway.settings_config["env"]["CLAUDE_CODE_DISABLE_THINKING"] = json!("1");
        let mut live = custom_live();
        live["env"]["CLAUDE_CODE_DISABLE_THINKING"] = json!("1");
        write_live(&live);

        // 第三方 → 官方：独有字段也写成空串，开着的会话重新打开思考
        switch_to(&db, Some(&gateway), &official()).expect("to official");
        assert_eq!(
            read_live()["env"]["CLAUDE_CODE_DISABLE_THINKING"],
            json!("")
        );
        // 墓碑还没删就切到另一张不带这个键的第三方卡：独有字段的空串不归 `clear` 管，
        // 靠记录并进这次写入的删除
        let other = provider(
            "other",
            "custom",
            json!({ "env": {
                "ANTHROPIC_BASE_URL": "https://other.example",
                "ANTHROPIC_AUTH_TOKEN": "sk-other"
            }}),
        );
        switch_to(&db, Some(&official()), &other).expect("to other");
        let carried = marker().unwrap();
        assert!(carried
            .keys
            .contains(&"CLAUDE_CODE_DISABLE_THINKING".to_string()));
        assert!(!carried.keys.contains(&"ANTHROPIC_BASE_URL".to_string()));

        settle_tombstones();
        assert_eq!(
            read_live()["env"],
            json!({
                "ANTHROPIC_BASE_URL": "https://other.example",
                "ANTHROPIC_AUTH_TOKEN": "sk-other",
                "API_TIMEOUT_MS": "300000"
            })
        );
        assert!(marker().is_none());
    }
}
