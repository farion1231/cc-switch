//! Codex 客户端是否还在用旧模型目录或缓存切换前的文件登录，以及按用户确认重启守护进程。
//!
//! Codex 的 app-server 只在启动时读一次模型目录（codex-rs `app-server/src/model_catalog.rs`：
//! 「retained startup model catalog」），之后每个请求重读 `config.toml`：路由跟着变，模型列表
//! 不变。0.159 起 `codex` TUI 默认连一个托管守护进程（`codex app-server --managed-daemon`），
//! 它只在 Codex 升级时重启，关掉再开 `codex` 不会重读；桌面版、编辑器插件自带的 app-server
//! 也要整个重开才会重读。
//!
//! 判断一个进程读到的是哪份目录，不能比文件时间：内容没变时引擎不重写文件，时间不动；退出
//! CC Switch 时撤掉目录指针、下次启动再写回，时间变了，早先启动的进程读到的却正是现在这份。
//! 所以记下目录的代次：从哪个时刻起，新启动的 Codex 会读到哪份目录（[`HISTORY_FILENAME`]）。
//! 一个进程读到的，是它启动之前开始的最后一代。
//!
//! 进程只看不动：用户在界面上确认之后，才调 Codex 自己的 `codex app-server daemon restart`。
//! 桌面版和编辑器插件不替用户重启，只提示彻底退出再开。进程表只在 macOS、Linux 上读（`ps`）。
//! Windows 上没有进程表：守护进程按它自己的 `daemon.pid` 打开进程、比对创建时间来判断；其余
//! app-server 看不到，改成「变了就提醒」——目录或文件登录和用户上次确认时（[`ACK_FILENAME`]）
//! 不同就提示可能还在用旧的，用户关掉提示时记下现在这份。`ps` 读不出来时也这样退回。

use std::process::Output;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::codex_config::{
    codex_config_auth_store_mode, extract_codex_auth_user_identity, get_codex_auth_path,
    get_codex_config_dir, get_codex_model_catalog_path, CodexAuthStoreMode,
};
use crate::live::engine::{sha256_hex, DeviceStore};
use crate::live::project::codex::live_catalog_is_ours;

use super::codex_direct::read_config_text;

pub(crate) const HISTORY_FILENAME: &str = "codex-catalog-history.json";
const LOGIN_HISTORY_FILENAME: &str = "codex-login-history.json";
/// 看不到进程时，用户上次确认（关掉提示）时的目录和文件登录。
const ACK_FILENAME: &str = "codex-client-ack.json";
/// 最多记这么多代。更早启动的进程判断不了，按旧的算。
const HISTORY_LIMIT: usize = 32;
/// 进程的启动时刻只精确到秒（`ps` 的 etime），每一代又是写完之后才记下的：启动时刻离一代的
/// 开始不到这么久，就当它读到的是上一代。拿不准时宁可多提示一次。
const MARGIN_MS: u64 = 2_000;
/// 新启动的 Codex 不读 CC Switch 的目录（没有目录指针，或者指向别人的目录）。
const NO_CATALOG: &str = "none";
/// 守护进程停机宽限期的缺省值和上限（codex-rs `app-server-daemon/src/settings.rs`）。
const DEFAULT_SHUTDOWN_GRACE_SECS: u64 = 60;
const MAX_SHUTDOWN_GRACE_SECS: u64 = 300;
/// 宽限期之外，起新进程、等它就绪的余量（codex-rs 的 `OPERATION_LOCK_TIMEOUT` 也是在宽限期上
/// 加 75 秒）。
const RESTART_MARGIN: Duration = Duration::from_secs(75);

/// 一代目录：从 `since_ms`（Unix 毫秒）起，新启动的 Codex 读到的目录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Generation {
    since_ms: u64,
    fingerprint: String,
}

/// 可能还在用旧模型列表或旧文件登录的 Codex 客户端（给前端）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StaleClients {
    /// 托管守护进程（`codex` TUI 连的那个）：可以替用户重启。
    pub daemon: bool,
    /// 其余 app-server（桌面版、编辑器插件）：要用户自己彻底退出再开。
    pub others: bool,
    /// 文件登录的身份变了，进程可能还缓存着旧账号。不是查询进程内存得到的确认。
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub auth: bool,
    /// 看不到桌面版、编辑器插件的进程（Windows，或 `ps` 读不出来）：`others` 只表示用户上次确认
    /// 之后目录或登录变了，不知道有没有开着。用户关掉提示时调 [`acknowledge`]。
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub unverified: bool,
}

/// 重启守护进程的结果（给前端）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RestartOutcome {
    Restarted,
    /// 守护进程没在跑，什么都没做：`restart` 会替用户起一个新的，下次开 `codex` 时它自己会起。
    NotRunning,
}

/// 正在跑的 app-server 的启动时刻（Unix 毫秒）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct AppServers {
    daemon: Option<u64>,
    others: Vec<u64>,
    /// 没有进程表：`others` 是空的不代表没开着。
    others_unknown: bool,
}

/// 用户上次确认时的目录和文件登录（[`ACK_FILENAME`]）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Acknowledged {
    catalog: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    login: Option<String>,
}

/// 外部依赖：进程表、时钟、重启命令。测试里换成假的。
pub(crate) struct Env {
    /// `ps -Ao pid=,etime=,command=` 的输出；读不到（或在 Windows 上）是 `None`。
    pub process_table: Box<dyn Fn() -> Option<String> + Send + Sync>,
    /// 没有进程表时，`daemon.pid` 记的守护进程还活着的话它的启动时刻（Unix 毫秒）。只有
    /// Windows 能这样查，别的平台是 `None`。
    pub daemon_started: Box<dyn Fn() -> Option<u64> + Send + Sync>,
    /// Unix 毫秒。
    pub now_ms: Box<dyn Fn() -> u64 + Send + Sync>,
    /// 执行 `codex app-server daemon restart`，参数是超时。
    pub restart: Box<dyn Fn(Duration) -> Result<Output, String> + Send + Sync>,
}

impl Env {
    fn real() -> Self {
        Self {
            process_table: Box::new(read_process_table),
            daemon_started: Box::new(read_daemon_started),
            now_ms: Box::new(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_millis() as u64)
                    .unwrap_or_default()
            }),
            restart: Box::new(run_restart),
        }
    }
}

fn env_slot() -> &'static Mutex<Option<Arc<Env>>> {
    static SLOT: OnceLock<Mutex<Option<Arc<Env>>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn env() -> Arc<Env> {
    let mut slot = env_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    slot.get_or_insert_with(|| Arc::new(Env::real())).clone()
}

#[cfg(test)]
pub(crate) fn set_test_env(env: Env) {
    *env_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::new(env));
}

#[cfg(test)]
pub(crate) fn reset_test_env() {
    *env_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

#[cfg(unix)]
fn read_process_table() -> Option<String> {
    // `-ww`：输出不是终端时 Linux 的 procps 也会截断命令行。
    let output = std::process::Command::new("ps")
        .args(["-ww", "-Ao", "pid=,etime=,command="])
        .env("LC_ALL", "C")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| log::debug!("读取进程表失败: {error}"))
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(not(unix))]
fn read_process_table() -> Option<String> {
    None
}

/// Windows：`daemon.pid` 里记着守护进程的 pid 和创建时间（codex-rs `backend/pid.rs` 的
/// `processStartTime`，Windows 上是 FILETIME 的十进制）。按 pid 打开进程，还在运行、创建时间
/// 也对得上（pid 没被别的进程复用）才算它在跑，返回它的启动时刻。
#[cfg(windows)]
fn read_daemon_started() -> Option<u64> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let (pid, recorded) = daemon_record()?;
    // SAFETY: 只查询；句柄在下面关掉。
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let zero = || FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero(), zero(), zero(), zero());
    let mut exit_code = 0u32;
    // SAFETY: 句柄有效，输出参数都指向本地变量。
    let queried = unsafe {
        GetExitCodeProcess(handle, &mut exit_code) != 0
            && GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) != 0
    };
    // SAFETY: OpenProcess 返回的句柄只关这一次。
    unsafe { CloseHandle(handle) };
    let created = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
    if !queried || exit_code != STILL_ACTIVE as u32 || created != recorded {
        return None;
    }
    filetime_to_unix_ms(created)
}

#[cfg(not(windows))]
fn read_daemon_started() -> Option<u64> {
    None
}

/// `daemon.pid` 的 pid 和 `processStartTime`（Windows 上的 FILETIME）。
#[cfg(any(windows, test))]
fn daemon_record() -> Option<(u32, u64)> {
    let bytes = std::fs::read(
        get_codex_config_dir()
            .join("app-server-daemon")
            .join("daemon.pid"),
    )
    .ok()?;
    let record: Value = serde_json::from_slice(&bytes).ok()?;
    let pid = u32::try_from(record.get("pid")?.as_u64()?).ok()?;
    let started = record.get("processStartTime")?.as_str()?.parse().ok()?;
    Some((pid, started))
}

/// FILETIME（从 1601 年起的 100 纳秒数）换成 Unix 毫秒。
#[cfg(any(windows, test))]
fn filetime_to_unix_ms(filetime: u64) -> Option<u64> {
    const UNIX_EPOCH_AS_FILETIME_MS: u64 = 11_644_473_600_000;
    (filetime / 10_000).checked_sub(UNIX_EPOCH_AS_FILETIME_MS)
}

fn run_restart(timeout: Duration) -> Result<Output, String> {
    // 重启的必须是读 CC Switch 写的这份配置的守护进程（配置目录可能被覆盖到别处）。
    let codex_dir = get_codex_config_dir();
    let extra_env = [("CODEX_HOME", codex_dir.to_string_lossy().into_owned())];
    crate::commands::run_detected_tool_command_with_timeout(
        "codex",
        &["app-server", "daemon", "restart"],
        Some(timeout),
        &extra_env,
        &codex_dir,
    )
}

/// 历史文件的读改写只在一个线程里做（写入和查询会同时记）。
fn history_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn read_history(store: &DeviceStore, filename: &str) -> Vec<Generation> {
    let path = store.file(filename);
    let Ok(bytes) = std::fs::read(&path) else {
        return Vec::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        log::warn!(
            "Codex 客户端状态的变化记录 {} 无法解析，当作空的: {error}",
            path.display()
        );
        Vec::new()
    })
}

/// 现在的目录和最后一代不同就记成新的一代，返回记完之后的历史。尽力而为：写不进去只打日志，
/// 下次再记（判断会偏向「旧」）。
fn record(store: &DeviceStore, fingerprint: &str, now_ms: u64) -> Vec<Generation> {
    record_history(store, HISTORY_FILENAME, fingerprint, now_ms)
}

fn record_history(
    store: &DeviceStore,
    filename: &str,
    fingerprint: &str,
    now_ms: u64,
) -> Vec<Generation> {
    let _guard = history_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut history = read_history(store, filename);
    if history
        .last()
        .is_some_and(|last| last.fingerprint == fingerprint)
    {
        return history;
    }
    history.push(Generation {
        since_ms: now_ms,
        fingerprint: fingerprint.to_string(),
    });
    let excess = history.len().saturating_sub(HISTORY_LIMIT);
    history.drain(..excess);
    if let Err(error) = crate::config::write_json_file(&store.file(filename), &history) {
        log::warn!("记录 Codex 客户端状态的变化失败: {error}");
    }
    history
}

/// 新启动的 Codex 现在会读到的目录：`config.toml` 顶层指向 CC Switch 的目录时是文件内容的
/// hash，否则是 [`NO_CATALOG`]。
fn current_fingerprint() -> String {
    if !live_catalog_is_ours(&read_config_text()) {
        return NO_CATALOG.to_string();
    }
    std::fs::read(get_codex_model_catalog_path())
        .map(|bytes| sha256_hex(&bytes))
        .unwrap_or_else(|_| NO_CATALOG.to_string())
}

/// 记下新启动的 Codex 会读到的目录和文件登录身份。客户端文件每写一次（直连、进出代理、Stack
/// 增删、退出时写回）和启动接上之后各调一次。
pub(crate) fn observe(store: &DeviceStore) {
    let now = (env().now_ms)();
    record(store, &current_fingerprint(), now);
    if let Some(login) = current_login_fingerprint() {
        record_history(store, LOGIN_HISTORY_FILENAME, &login, now);
    }
}

/// 只跟踪 file 存储里的稳定身份，不持久化 token，不把 token 轮换误判成切号。
/// keyring/auto/ephemeral 的实际身份不能从 auth.json 推断，交给它们各自的登录流程。
fn current_login_fingerprint() -> Option<String> {
    if codex_config_auth_store_mode(&read_config_text()) != CodexAuthStoreMode::File {
        return None;
    }
    let bytes = std::fs::read(get_codex_auth_path()).ok()?;
    let auth: Value = serde_json::from_slice(&bytes).ok()?;
    let account = auth.pointer("/tokens/account_id")?.as_str()?;
    if account.trim().is_empty() {
        return None;
    }
    let user = extract_codex_auth_user_identity(&auth);
    Some(sha256_hex(&serde_json::to_vec(&(account, user)).ok()?))
}

/// 可能缓存旧账号或目录的客户端。`check_catalog` 只影响目录，账号检查不受模式限制。
/// 要读进程表，放到阻塞线程池里调；不查询进程的实际认证，也不自动重启。
pub(crate) fn stale_clients(store: &DeviceStore, check_catalog: bool) -> Option<StaleClients> {
    let env = env();
    let current = current_fingerprint();
    // 顺手记一次：兜住在 CC Switch 之外改了目录的情况。
    let history = record(store, &current, (env.now_ms)());
    let servers = probe(&env);
    let mut stale = if check_catalog {
        judge(&history, &current, &servers)
    } else {
        None
    };
    let login = current_login_fingerprint();
    if let Some(login) = &login {
        let history = record_history(store, LOGIN_HISTORY_FILENAME, login, (env.now_ms)());
        // 首次观察不是切号；不能因进程早于安装 CC Switch 就声称它缓存了别的账号。
        if history.len() > 1 {
            if let Some(auth_stale) = judge(&history, login, &servers) {
                let clients = stale.get_or_insert(StaleClients {
                    daemon: false,
                    others: false,
                    auth: false,
                    unverified: false,
                });
                clients.daemon |= auth_stale.daemon;
                clients.others |= auth_stale.others;
                clients.auth = true;
            }
        }
    }
    if servers.others_unknown {
        let ack = acknowledged(store, &current, login.as_deref());
        let catalog_changed = check_catalog && ack.catalog != current;
        // 确认时不是文件登录（没记下身份）不算切号，和首次观察一样。
        let login_changed =
            matches!((&ack.login, &login), (Some(before), Some(now)) if before != now);
        if catalog_changed || login_changed {
            let clients = stale.get_or_insert(StaleClients {
                daemon: false,
                others: false,
                auth: false,
                unverified: false,
            });
            clients.others = true;
            clients.unverified = true;
            clients.auth |= login_changed;
        }
    }
    stale
}

/// 看不到进程时，用户上次确认的目录和登录。从没确认过就把现在这份当作确认过的：装好或升级
/// 之后第一次不提示，不能因为以前切过就声称开着的客户端还在用旧的。
fn acknowledged(store: &DeviceStore, catalog: &str, login: Option<&str>) -> Acknowledged {
    let _guard = history_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let path = store.file(ACK_FILENAME);
    if let Some(mut ack) = std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Acknowledged>(&bytes).ok())
    {
        // 确认时还没有文件登录：第一次看到的登录记作基准（登录不是切号），之后再换才提示。
        if ack.login.is_none() && login.is_some() {
            ack.login = login.map(str::to_string);
            write_ack(store, &ack);
        }
        return ack;
    }
    let ack = Acknowledged {
        catalog: catalog.to_string(),
        login: login.map(str::to_string),
    };
    write_ack(store, &ack);
    ack
}

/// 提示对应的是哪一份目录和登录（给前端）：用户关掉提示之后，这个值变了（目录或登录又改了）
/// 才是新的提示，要重新显示。
pub(crate) fn revision() -> String {
    sha256_hex(
        format!(
            "{}\n{:?}",
            current_fingerprint(),
            current_login_fingerprint()
        )
        .as_bytes(),
    )
}

/// 用户关掉「可能还在用旧的」提示：记下现在的目录和登录，同一份不再提示，再变才提示。
pub(crate) fn acknowledge(store: &DeviceStore) {
    let ack = Acknowledged {
        catalog: current_fingerprint(),
        login: current_login_fingerprint(),
    };
    let _guard = history_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    write_ack(store, &ack);
}

fn write_ack(store: &DeviceStore, ack: &Acknowledged) {
    if let Err(error) = crate::config::write_json_file(&store.file(ACK_FILENAME), ack) {
        log::warn!("记录 Codex 客户端提示的确认失败: {error}");
    }
}

fn judge(history: &[Generation], current: &str, servers: &AppServers) -> Option<StaleClients> {
    let daemon = servers
        .daemon
        .is_some_and(|started| is_stale(history, current, started));
    let others = servers
        .others
        .iter()
        .any(|&started| is_stale(history, current, started));
    (daemon || others).then_some(StaleClients {
        daemon,
        others,
        auth: false,
        unverified: false,
    })
}

/// 启动于 `started_ms` 的进程读到的是不是别的目录。早于记下的第一代、判断不了的：现在
/// 不读 CC Switch 的目录（[`NO_CATALOG`]）时按新的算，否则从没进过代理的直连用户也会被
/// 报；其余按旧的算。
fn is_stale(history: &[Generation], current: &str, started_ms: u64) -> bool {
    history
        .iter()
        .rev()
        .find(|generation| generation.since_ms.saturating_add(MARGIN_MS) <= started_ms)
        .map_or(current != NO_CATALOG, |generation| {
            generation.fingerprint != current
        })
}

fn probe(env: &Env) -> AppServers {
    let Some(table) = (env.process_table)() else {
        return AppServers {
            daemon: (env.daemon_started)(),
            others: Vec::new(),
            others_unknown: true,
        };
    };
    let plugins = format!("{}/", get_codex_config_dir().join("plugins").display());
    classify(&table, daemon_pid(), &plugins, (env.now_ms)())
}

/// 守护进程自己记的 pid（`<Codex 目录>/app-server-daemon/daemon.pid`）。
fn daemon_pid() -> Option<u32> {
    let path = get_codex_config_dir()
        .join("app-server-daemon")
        .join("daemon.pid");
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice::<Value>(&bytes)
        .ok()?
        .get("pid")?
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
}

/// 从进程表里挑出 app-server。守护进程要 pid 和 `daemon.pid` 对得上、命令行还是
/// `--managed-daemon`（防止 pid 被别的进程复用），别的配置目录的守护进程不算。`plugins_dir`
/// 下的是 Chrome 插件自带的，没有模型选择器，不算。
fn classify(table: &str, daemon_pid: Option<u32>, plugins_dir: &str, now_ms: u64) -> AppServers {
    let mut servers = AppServers::default();
    for line in table.lines() {
        let Some((pid, elapsed_secs, command)) = split_row(line) else {
            continue;
        };
        let Some(role) = app_server_role(command) else {
            continue;
        };
        let started = now_ms.saturating_sub(elapsed_secs.saturating_mul(1000));
        match role {
            Role::Daemon if Some(pid) == daemon_pid => servers.daemon = Some(started),
            Role::Daemon => {}
            Role::Other if command.starts_with(plugins_dir) => {}
            Role::Other => servers.others.push(started),
        }
    }
    servers
}

/// `pid etime command` 一行。
fn split_row(line: &str) -> Option<(u32, u64, &str)> {
    let (pid, rest) = line.trim_start().split_once(char::is_whitespace)?;
    let (etime, command) = rest.trim_start().split_once(char::is_whitespace)?;
    Some((pid.parse().ok()?, parse_etime(etime)?, command.trim_start()))
}

/// `ps` 的 etime：`[[dd-]hh:]mm:ss`，换成秒。
fn parse_etime(value: &str) -> Option<u64> {
    let (days, clock) = match value.split_once('-') {
        Some((days, clock)) => (days.parse::<u64>().ok()?, clock),
        None => (0, value),
    };
    let parts: Vec<&str> = clock.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let mut secs = 0u64;
    for part in parts {
        secs = secs * 60 + part.parse::<u64>().ok()?;
    }
    Some(days * 86_400 + secs)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Daemon,
    Other,
}

/// `…/codex [-c 键=值 …] app-server …` 是 app-server；`app-server daemon …`、`app-server proxy`
/// 是管理守护进程的子命令，不读模型目录。命令行是 `ps` 用空格拼起来的，分不出路径里的空格，
/// 所以先找可执行文件名 `codex`，再看它后面的参数。
fn app_server_role(command: &str) -> Option<Role> {
    let mut words = after_codex_executable(command)?.split_whitespace();
    let subcommand = loop {
        match words.next()? {
            "-c" | "--config" => {
                words.next()?;
            }
            flag if flag.starts_with('-') => {}
            word => break word,
        }
    };
    if subcommand != "app-server" {
        return None;
    }
    let rest: Vec<&str> = words.collect();
    if matches!(rest.first(), Some(&"daemon") | Some(&"proxy")) {
        return None;
    }
    Some(if rest.contains(&"--managed-daemon") {
        Role::Daemon
    } else {
        Role::Other
    })
}

/// 命令行里可执行文件 `codex`（`codex` 或 `…/codex`）之后的部分。
fn after_codex_executable(command: &str) -> Option<&str> {
    const NAME: &str = "codex";
    let mut from = 0;
    while let Some(found) = command[from..].find(NAME) {
        let start = from + found;
        let end = start + NAME.len();
        let rest = &command[end..];
        if (start == 0 || command[..start].ends_with('/'))
            && (rest.is_empty() || rest.starts_with(' '))
        {
            return Some(rest);
        }
        from = end;
    }
    None
}

/// 用户确认之后重启托管守护进程：Codex 自己的 `codex app-server daemon restart`，先等守护进程
/// 里在跑的任务收尾（最多一个宽限期），再起新的。守护进程没在跑就什么都不做。
pub(crate) fn restart_daemon() -> Result<RestartOutcome, String> {
    let env = env();
    if probe(&env).daemon.is_none() {
        return Ok(RestartOutcome::NotRunning);
    }
    let output = (env.restart)(restart_timeout())?;
    let stdout = crate::commands::decode_command_output(&output.stdout);
    if !output.status.success() {
        let stderr = crate::commands::decode_command_output(&output.stderr);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        return Err(if detail.is_empty() {
            "重启 Codex 守护进程失败 (Failed to restart the Codex daemon)".to_string()
        } else {
            format!("重启 Codex 守护进程失败 (Failed to restart the Codex daemon): {detail}")
        });
    }
    Ok(match lifecycle_status(&stdout).as_deref() {
        Some("notRunning") => RestartOutcome::NotRunning,
        _ => RestartOutcome::Restarted,
    })
}

/// `codex app-server daemon` 的子命令在 stdout 最后一行打一个 JSON（codex-rs `LifecycleOutput`）。
fn lifecycle_status(stdout: &str) -> Option<String> {
    let line = stdout.lines().rev().find(|line| !line.trim().is_empty())?;
    serde_json::from_str::<Value>(line.trim())
        .ok()?
        .get("status")?
        .as_str()
        .map(str::to_string)
}

/// 守护进程的停机宽限期（`<Codex 目录>/app-server-daemon/settings.json` 的
/// `shutdownGraceSeconds`）加上起新进程的余量。
fn restart_timeout() -> Duration {
    let grace = std::fs::read(
        get_codex_config_dir()
            .join("app-server-daemon")
            .join("settings.json"),
    )
    .ok()
    .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    .and_then(|settings| settings.get("shutdownGraceSeconds")?.as_u64())
    .unwrap_or(DEFAULT_SHUTDOWN_GRACE_SECS)
    .min(MAX_SHUTDOWN_GRACE_SECS);
    Duration::from_secs(grace) + RESTART_MARGIN
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    const DAEMON: &str =
        "/Users/me/.codex/packages/app-server-daemon/releases/0.159.2-aarch64-apple-darwin/bin/codex";

    /// 临时 home（历史文件和 Codex 目录都落在这里），结束时换回真的依赖。
    struct Scope {
        dir: tempfile::TempDir,
        saved: Option<std::ffi::OsString>,
    }

    impl Scope {
        fn new() -> Self {
            let dir = tempfile::TempDir::new().unwrap();
            let saved = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            std::fs::create_dir_all(get_codex_config_dir()).unwrap();
            Self { dir, saved }
        }

        fn store(&self) -> DeviceStore {
            DeviceStore::at(self.dir.path().join(".cc-switch"))
        }
    }

    impl Drop for Scope {
        fn drop(&mut self) {
            reset_test_env();
            match self.saved.take() {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn generation(since_ms: u64, fingerprint: &str) -> Generation {
        Generation {
            since_ms,
            fingerprint: fingerprint.to_string(),
        }
    }

    /// 假的时钟和进程表：`clock` 是现在（Unix 毫秒），`table` 是 `ps` 的输出。
    fn fake_env(clock: Arc<AtomicU64>, table: Arc<Mutex<String>>, restarted: Arc<AtomicBool>) {
        let now = clock.clone();
        set_test_env(Env {
            process_table: Box::new(move || Some(table.lock().unwrap().clone())),
            daemon_started: Box::new(|| None),
            now_ms: Box::new(move || now.load(Ordering::SeqCst)),
            restart: Box::new(move |_| {
                restarted.store(true, Ordering::SeqCst);
                Ok(success(r#"{"status":"restarted","pid":1}"#))
            }),
        });
    }

    /// 看不到进程表的环境（Windows）：`daemon` 是按 `daemon.pid` 查到的守护进程启动时刻。
    fn fake_env_without_table(
        clock: Arc<AtomicU64>,
        daemon: Arc<Mutex<Option<u64>>>,
        restarted: Arc<AtomicBool>,
    ) {
        let now = clock.clone();
        set_test_env(Env {
            process_table: Box::new(|| None),
            daemon_started: Box::new(move || *daemon.lock().unwrap()),
            now_ms: Box::new(move || now.load(Ordering::SeqCst)),
            restart: Box::new(move |_| {
                restarted.store(true, Ordering::SeqCst);
                Ok(success(r#"{"status":"restarted","pid":1}"#))
            }),
        });
    }

    fn write_file_login(account: &str) {
        std::fs::write(
            get_codex_config_dir().join("auth.json"),
            serde_json::json!({"tokens": {"account_id": account, "access_token": "token"}})
                .to_string(),
        )
        .unwrap();
    }

    fn unverified(daemon: bool, auth: bool) -> Option<StaleClients> {
        Some(StaleClients {
            daemon,
            others: true,
            auth,
            unverified: true,
        })
    }

    #[cfg(unix)]
    fn success(stdout: &str) -> Output {
        use std::os::unix::process::ExitStatusExt;
        Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: format!("{stdout}\n").into_bytes(),
            stderr: Vec::new(),
        }
    }

    #[cfg(windows)]
    fn success(stdout: &str) -> Output {
        use std::os::windows::process::ExitStatusExt;
        Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: format!("{stdout}\n").into_bytes(),
            stderr: Vec::new(),
        }
    }

    fn write_daemon_pid(pid: u32) {
        let dir = get_codex_config_dir().join("app-server-daemon");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("daemon.pid"), format!(r#"{{"pid":{pid}}}"#)).unwrap();
    }

    /// 让新启动的 Codex 读 CC Switch 的目录（`catalog` 是目录内容），`None` 是撤掉指针。
    fn point_at_catalog(catalog: Option<&str>) {
        let config = get_codex_config_dir().join("config.toml");
        match catalog {
            Some(content) => {
                std::fs::write(
                    &config,
                    "openai_base_url = \"http://127.0.0.1:15721/v1\"\nmodel_catalog_json = \"cc-switch-model-catalog.json\"\n",
                )
                .unwrap();
                std::fs::write(get_codex_model_catalog_path(), content).unwrap();
            }
            None => std::fs::write(&config, "model = \"gpt-6-astra\"\n").unwrap(),
        }
    }

    #[test]
    fn parses_etime() {
        assert_eq!(parse_etime("05:03"), Some(303));
        assert_eq!(parse_etime("02:53:10"), Some(2 * 3600 + 53 * 60 + 10));
        assert_eq!(parse_etime("3-01:02:03"), Some(3 * 86_400 + 3723));
        assert_eq!(
            parse_etime("02-11:19:30"),
            Some(2 * 86_400 + 11 * 3600 + 19 * 60 + 30)
        );
        assert_eq!(parse_etime("12"), None);
        assert_eq!(parse_etime("a:b"), None);
    }

    #[test]
    fn classifies_app_servers() {
        let table = [
            format!("35946 02-11:19:30 {DAEMON} app-server daemon pid-update-loop"),
            format!("59013       07:19 {DAEMON} app-server --listen unix:// --managed-daemon"),
            // 另一个配置目录的守护进程：pid 对不上，不算。
            format!("70000       00:10 {DAEMON} app-server --listen unix:// --managed-daemon"),
            "62347       04:57 /Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex -c features.code_mode_host=true app-server --analytics-default-enabled -c plugins.codex-app-tools@openai-bundled.mcp_servers.codex_app.enabled=true".to_string(),
            "83919    02:11:54 /Users/me/.codex/plugins/.plugin-appserver/codex-cli/CodexCLI.app/Contents/MacOS/codex app-server --analytics-default-enabled".to_string(),
            "81234       01:00 /Users/me/Library/Application Support/Code/User/globalStorage/openai.chatgpt/bin/codex app-server".to_string(),
            "90000       00:05 /opt/homebrew/bin/codex app-server proxy".to_string(),
            "91000       00:05 /opt/homebrew/bin/codex --no-daemon".to_string(),
            "92000       00:05 /usr/bin/vim /Users/me/.codex/config.toml".to_string(),
            "93000       00:05 /Users/me/codex-tools/run app-server".to_string(),
        ]
        .join("\n");
        let now = 10_000_000;
        let servers = classify(&table, Some(59013), "/Users/me/.codex/plugins/", now);
        assert_eq!(servers.daemon, Some(now - 439_000));
        assert_eq!(servers.others, vec![now - 297_000, now - 60_000]);
        // daemon.pid 指向的进程不是守护进程（pid 被复用了）：不算守护进程。
        assert_eq!(
            classify(&table, Some(62347), "/Users/me/.codex/plugins/", now).daemon,
            None
        );
    }

    #[test]
    fn judges_by_generation_started_before() {
        let history = [generation(1_000_000, "a"), generation(2_000_000, "b")];
        // 在 b 之后启动：读到的是现在这份。
        assert!(!is_stale(&history, "b", 2_000_000 + MARGIN_MS));
        // 离 b 开始不到余量：当它读到的是 a。
        assert!(is_stale(&history, "b", 2_000_000 + MARGIN_MS - 1));
        assert!(is_stale(&history, "b", 1_500_000));
        // 早于第一代：判断不了，按旧的算。
        assert!(is_stale(&history, "b", 500_000));
        // 现在不读 CC Switch 的目录：早于第一代的按新的算；在我们的目录时启动的仍是旧的。
        let history = [
            generation(1_000_000, "a"),
            generation(2_000_000, NO_CATALOG),
        ];
        assert!(!is_stale(&history, NO_CATALOG, 500_000));
        assert!(is_stale(&history, NO_CATALOG, 1_500_000));
        // 来回切：a → b → a，在第一次 a 时启动的进程读到的正是现在这份。
        let history = [
            generation(1_000_000, "a"),
            generation(2_000_000, "b"),
            generation(3_000_000, "a"),
        ];
        assert!(!is_stale(&history, "a", 1_500_000));
        assert!(is_stale(&history, "a", 2_500_000));
    }

    #[test]
    #[serial]
    fn records_only_changes_and_trims() {
        let scope = Scope::new();
        let store = scope.store();
        assert_eq!(record(&store, "a", 1).len(), 1);
        assert_eq!(record(&store, "a", 2), vec![generation(1, "a")]);
        for step in 0..40u64 {
            record(&store, &format!("f{step}"), 10 + step);
        }
        let history = read_history(&store, HISTORY_FILENAME);
        assert_eq!(history.len(), HISTORY_LIMIT);
        assert_eq!(history.last(), Some(&generation(49, "f39")));
        assert_eq!(history.first(), Some(&generation(18, "f8")));
    }

    #[test]
    #[serial]
    fn fingerprint_follows_the_catalog_pointer() {
        let _scope = Scope::new();
        assert_eq!(current_fingerprint(), NO_CATALOG);
        point_at_catalog(Some("{\"models\":[]}"));
        let fingerprint = current_fingerprint();
        assert_eq!(fingerprint, sha256_hex(b"{\"models\":[]}"));
        // 指向别人的目录：新启动的 Codex 不读 CC Switch 的目录。
        std::fs::write(
            get_codex_config_dir().join("config.toml"),
            "model_catalog_json = \"/elsewhere/models.json\"\n",
        )
        .unwrap();
        assert_eq!(current_fingerprint(), NO_CATALOG);
        point_at_catalog(None);
        assert_eq!(current_fingerprint(), NO_CATALOG);
    }

    /// 进 Stack → 守护进程启动 → 退出 CC Switch（撤掉指针）→ 再打开（写回同一份目录）：守护进程
    /// 读到的正是现在这份，不提示。退出期间重启过的守护进程读到的是没有目录的配置，要提示。
    #[test]
    #[serial]
    fn quitting_and_reopening_cc_switch_is_not_stale() {
        let scope = Scope::new();
        let store = scope.store();
        let clock = Arc::new(AtomicU64::new(1_000_000));
        let table = Arc::new(Mutex::new(String::new()));
        fake_env(
            clock.clone(),
            table.clone(),
            Arc::new(AtomicBool::new(false)),
        );
        let daemon_row = |elapsed: &str| {
            format!("59013 {elapsed} {DAEMON} app-server --listen unix:// --managed-daemon")
        };
        write_daemon_pid(59013);

        point_at_catalog(Some("stack"));
        observe(&store);
        // 进 Stack 之后 10 秒守护进程启动。
        clock.store(1_010_000, Ordering::SeqCst);
        *table.lock().unwrap() = daemon_row("00:00");
        clock.store(1_060_000, Ordering::SeqCst);
        *table.lock().unwrap() = daemon_row("00:50");
        assert_eq!(stale_clients(&store, true), None);

        // 退出 CC Switch，一分钟后再打开。
        point_at_catalog(None);
        observe(&store);
        clock.store(1_120_000, Ordering::SeqCst);
        point_at_catalog(Some("stack"));
        observe(&store);
        *table.lock().unwrap() = daemon_row("01:50");
        assert_eq!(stale_clients(&store, true), None);

        // 目录变了（Stack 增删）：守护进程还拿着旧的。
        clock.store(1_200_000, Ordering::SeqCst);
        point_at_catalog(Some("stack + kimi"));
        observe(&store);
        *table.lock().unwrap() = daemon_row("03:10");
        assert_eq!(
            stale_clients(&store, true),
            Some(StaleClients {
                daemon: true,
                others: false,
                auth: false,
                unverified: false
            })
        );

        // 退出期间守护进程重启过（比如 Codex 自动升级）。
        point_at_catalog(None);
        observe(&store);
        clock.store(1_300_000, Ordering::SeqCst);
        *table.lock().unwrap() = daemon_row("00:30");
        clock.store(1_400_000, Ordering::SeqCst);
        point_at_catalog(Some("stack + kimi"));
        observe(&store);
        *table.lock().unwrap() = daemon_row("02:10");
        assert_eq!(
            stale_clients(&store, true),
            Some(StaleClients {
                daemon: true,
                others: false,
                auth: false,
                unverified: false
            })
        );
    }

    #[test]
    #[serial]
    fn desktop_app_servers_are_reported_separately() {
        let scope = Scope::new();
        let store = scope.store();
        let clock = Arc::new(AtomicU64::new(1_000_000));
        let table = Arc::new(Mutex::new(String::new()));
        fake_env(
            clock.clone(),
            table.clone(),
            Arc::new(AtomicBool::new(false)),
        );
        point_at_catalog(Some("old"));
        observe(&store);
        clock.store(1_100_000, Ordering::SeqCst);
        point_at_catalog(Some("new"));
        observe(&store);
        clock.store(1_200_000, Ordering::SeqCst);
        // 桌面版在旧目录时启动；守护进程没在跑。
        *table.lock().unwrap() =
            "62347 02:30 /Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex -c features.code_mode_host=true app-server".to_string();
        assert_eq!(
            stale_clients(&store, true),
            Some(StaleClients {
                daemon: false,
                others: true,
                auth: false,
                unverified: false
            })
        );
        // 撤掉指针之后仍提示：桌面版还拿着 CC Switch 的旧目录，新启动的 Codex 读的是内置列表。
        point_at_catalog(None);
        assert_eq!(
            stale_clients(&store, true),
            Some(StaleClients {
                daemon: false,
                others: true,
                auth: false,
                unverified: false
            })
        );
    }

    #[test]
    #[serial]
    fn account_switch_is_reported_without_a_catalog_and_refresh_is_not() {
        let scope = Scope::new();
        let store = scope.store();
        let clock = Arc::new(AtomicU64::new(1_000_000));
        let table = Arc::new(Mutex::new(String::new()));
        let restarted = Arc::new(AtomicBool::new(false));
        fake_env(clock.clone(), table.clone(), restarted.clone());
        point_at_catalog(None);
        let auth = |account: &str, token: &str| {
            std::fs::write(
                get_codex_config_dir().join("auth.json"),
                serde_json::json!({"tokens": {"account_id": account, "access_token": token}})
                    .to_string(),
            )
            .unwrap();
        };
        auth("a", "old-token");
        observe(&store);
        clock.store(1_100_000, Ordering::SeqCst);
        *table.lock().unwrap() = "62347 01:30 /opt/bin/codex app-server".to_string();
        assert_eq!(
            stale_clients(&store, true),
            None,
            "initial observation is not a switch"
        );
        auth("a", "refreshed-token");
        observe(&store);
        assert_eq!(
            stale_clients(&store, true),
            None,
            "token refresh is not an account switch"
        );

        auth("b", "target-token");
        observe(&store);
        assert!(
            stale_clients(&store, true).is_some(),
            "the running server still caches account a"
        );
        assert!(
            !restarted.load(Ordering::SeqCst),
            "observing must not restart anything"
        );
        clock.store(1_200_000, Ordering::SeqCst);
        *table.lock().unwrap() = "62347 00:30 /opt/bin/codex app-server".to_string();
        assert_eq!(
            stale_clients(&store, true),
            None,
            "a new server reads account b"
        );
        // 切回 a：最早读 a 的进程不需要重启，读 b 的进程需要。
        auth("a", "another-token");
        observe(&store);
        *table.lock().unwrap() = "62347 03:10 /opt/bin/codex app-server".to_string();
        assert_eq!(stale_clients(&store, false), None);
        *table.lock().unwrap() = "62347 00:30 /opt/bin/codex app-server".to_string();
        let notice = stale_clients(&store, false).unwrap();
        assert!(notice.auth && notice.others && !notice.daemon);
        write_daemon_pid(59013);
        *table.lock().unwrap() = format!("59013 00:30 {DAEMON} app-server --managed-daemon");
        let notice = stale_clients(&store, false).unwrap();
        assert!(notice.auth && notice.daemon && !notice.others);
        assert!(!restarted.load(Ordering::SeqCst));
        for mode in ["keyring", "auto", "ephemeral"] {
            std::fs::write(
                get_codex_config_dir().join("config.toml"),
                format!("cli_auth_credentials_store = \"{mode}\"\n"),
            )
            .unwrap();
            assert_eq!(
                stale_clients(&store, false),
                None,
                "{mode} is not file auth"
            );
        }
    }

    #[test]
    #[serial]
    fn file_login_fingerprint_separates_users_in_the_same_workspace() {
        use base64::Engine;
        let scope = Scope::new();
        point_at_catalog(None);
        let login = |user: &str| {
            let claims = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(serde_json::json!({"sub": user}).to_string());
            std::fs::write(
                get_codex_auth_path(),
                serde_json::json!({"tokens": {
                    "account_id": "shared-workspace",
                    "id_token": format!("eyJhbGciOiJub25lIn0.{claims}.synthetic"),
                    "access_token": "not-persisted"
                }})
                .to_string(),
            )
            .unwrap();
        };
        login("a");
        let first = current_login_fingerprint().unwrap();
        observe(&scope.store());
        login("b");
        assert_ne!(current_login_fingerprint().unwrap(), first);
        let history = std::fs::read_to_string(scope.store().file(LOGIN_HISTORY_FILENAME)).unwrap();
        assert!(!history.contains("not-persisted"));
        assert!(!history.contains("shared-workspace"));
    }

    #[test]
    #[serial]
    fn restart_skips_when_the_daemon_is_not_running() {
        let _scope = Scope::new();
        let restarted = Arc::new(AtomicBool::new(false));
        let table = Arc::new(Mutex::new(String::new()));
        fake_env(
            Arc::new(AtomicU64::new(1_000_000)),
            table.clone(),
            restarted.clone(),
        );
        assert_eq!(restart_daemon(), Ok(RestartOutcome::NotRunning));
        assert!(!restarted.load(Ordering::SeqCst));

        write_daemon_pid(59013);
        *table.lock().unwrap() =
            format!("59013 07:19 {DAEMON} app-server --listen unix:// --managed-daemon");
        assert_eq!(restart_daemon(), Ok(RestartOutcome::Restarted));
        assert!(restarted.load(Ordering::SeqCst));
    }

    #[test]
    #[serial]
    fn without_a_process_table_changes_since_the_last_acknowledgement_are_reported() {
        let scope = Scope::new();
        let store = scope.store();
        let clock = Arc::new(AtomicU64::new(1_000_000));
        let daemon = Arc::new(Mutex::new(None));
        fake_env_without_table(clock.clone(), daemon, Arc::new(AtomicBool::new(false)));
        point_at_catalog(Some("a"));
        observe(&store);
        // 第一次查：没确认过，把现在这份当作确认过的，不提示。
        assert_eq!(stale_clients(&store, true), None);

        clock.store(1_100_000, Ordering::SeqCst);
        point_at_catalog(Some("b"));
        observe(&store);
        assert_eq!(stale_clients(&store, true), unverified(false, false));
        // 路由那家自己管理目录时不查目录。
        assert_eq!(stale_clients(&store, false), None);

        // 用户关掉提示：同一份不再提示。
        acknowledge(&store);
        assert_eq!(stale_clients(&store, true), None);

        // 撤掉指针也是变了：开着的客户端还拿着 CC Switch 的目录。
        point_at_catalog(None);
        observe(&store);
        assert_eq!(stale_clients(&store, true), unverified(false, false));
        acknowledge(&store);

        // 退出 CC Switch 撤掉、再启动写回同一份：最后和确认时一样，不提示。
        point_at_catalog(Some("b"));
        observe(&store);
        point_at_catalog(None);
        observe(&store);
        assert_eq!(stale_clients(&store, true), None);
    }

    #[test]
    #[serial]
    fn without_a_process_table_the_daemon_is_judged_by_its_start_time() {
        let scope = Scope::new();
        let store = scope.store();
        let clock = Arc::new(AtomicU64::new(1_000_000));
        let daemon = Arc::new(Mutex::new(None));
        fake_env_without_table(
            clock.clone(),
            daemon.clone(),
            Arc::new(AtomicBool::new(false)),
        );
        point_at_catalog(Some("a"));
        observe(&store);
        clock.store(1_100_000, Ordering::SeqCst);
        // 守护进程在 a 时启动。
        *daemon.lock().unwrap() = Some(1_050_000);
        assert_eq!(stale_clients(&store, true), None);

        clock.store(1_200_000, Ordering::SeqCst);
        point_at_catalog(Some("b"));
        observe(&store);
        assert_eq!(stale_clients(&store, true), unverified(true, false));

        // 用户关掉提示：桌面版那部分不再提示，守护进程照样是旧的。
        acknowledge(&store);
        assert_eq!(
            stale_clients(&store, true),
            Some(StaleClients {
                daemon: true,
                others: false,
                auth: false,
                unverified: false
            })
        );
        // 守护进程重启之后：读到的是现在这份。
        *daemon.lock().unwrap() = Some(1_300_000);
        clock.store(1_400_000, Ordering::SeqCst);
        assert_eq!(stale_clients(&store, true), None);
    }

    #[test]
    #[serial]
    fn without_a_process_table_an_account_switch_is_reported_but_a_first_login_is_not() {
        let scope = Scope::new();
        let store = scope.store();
        let clock = Arc::new(AtomicU64::new(1_000_000));
        fake_env_without_table(
            clock.clone(),
            Arc::new(Mutex::new(None)),
            Arc::new(AtomicBool::new(false)),
        );
        point_at_catalog(None);
        // 第一次检查时还没有文件登录：之后登录不算切号，但要记下它，再换账号才能提示。
        assert_eq!(stale_clients(&store, true), None);
        write_file_login("a");
        observe(&store);
        assert_eq!(stale_clients(&store, true), None);

        write_file_login("b");
        observe(&store);
        assert_eq!(stale_clients(&store, true), unverified(false, true));
        acknowledge(&store);
        assert_eq!(stale_clients(&store, true), None);
    }

    #[test]
    #[serial]
    fn without_a_process_table_restart_needs_a_live_daemon() {
        let _scope = Scope::new();
        let restarted = Arc::new(AtomicBool::new(false));
        let daemon = Arc::new(Mutex::new(None));
        fake_env_without_table(
            Arc::new(AtomicU64::new(1_000_000)),
            daemon.clone(),
            restarted.clone(),
        );
        // `codex app-server daemon restart` 在守护进程没跑时会起一个新的：不调。
        assert_eq!(restart_daemon(), Ok(RestartOutcome::NotRunning));
        assert!(!restarted.load(Ordering::SeqCst));

        *daemon.lock().unwrap() = Some(900_000);
        assert_eq!(restart_daemon(), Ok(RestartOutcome::Restarted));
        assert!(restarted.load(Ordering::SeqCst));
    }

    #[test]
    #[serial]
    fn the_revision_changes_with_the_catalog_and_the_login() {
        let _scope = Scope::new();
        point_at_catalog(Some("{\"models\":[1]}"));
        let first = revision();
        assert_eq!(revision(), first, "same catalog, same notice");

        point_at_catalog(Some("{\"models\":[2]}"));
        let second = revision();
        assert_ne!(second, first, "the catalog changed again");

        write_file_login("a");
        assert_ne!(revision(), second, "the login changed");
    }

    #[test]
    #[serial]
    fn reads_the_daemon_record_and_converts_filetime() {
        let _scope = Scope::new();
        assert_eq!(daemon_record(), None);
        let dir = get_codex_config_dir().join("app-server-daemon");
        std::fs::create_dir_all(&dir).unwrap();
        // 2026-10-10T00:00:00Z
        std::fs::write(
            dir.join("daemon.pid"),
            r#"{"pid":4242,"processStartTime":"134360640000000000"}"#,
        )
        .unwrap();
        assert_eq!(daemon_record(), Some((4242, 134_360_640_000_000_000)));
        assert_eq!(
            filetime_to_unix_ms(134_360_640_000_000_000),
            Some(1_791_590_400_000)
        );
        assert_eq!(filetime_to_unix_ms(0), None);
        // Unix 上 processStartTime 是 `ps` 的 lstart 文本，不是数字：认不出来。
        std::fs::write(
            dir.join("daemon.pid"),
            r#"{"pid":4242,"processStartTime":"Sat Oct 10 08:00:00 2026"}"#,
        )
        .unwrap();
        assert_eq!(daemon_record(), None);
    }

    #[test]
    fn reads_the_lifecycle_status() {
        assert_eq!(
            lifecycle_status("warning: x\n{\"status\":\"restarted\",\"pid\":1}\n\n").as_deref(),
            Some("restarted")
        );
        assert_eq!(lifecycle_status("not json"), None);
    }
}
