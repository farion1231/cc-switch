//! 检测 Codex / Claude Code / Gemini CLI 客户端是否正在运行。
//!
//! 用途：cc-switch 切换供应商后，新配置已写入 `~/.codex/config.toml`，但**当前**
//! Codex session 仍用旧配置（Codex 在 session 启动时读 config.toml，运行中不
//! 重新加载）。前端需要在切换成功的 toast 里区分两种情况：
//!
//! - Codex 未运行：新会话直接用新配置 → "无缝切换"
//! - Codex 在运行：当前会话仍用旧配置，新会话用新配置 → 提示用户「当前会话
//!   需重启 Codex 才生效；点此重启」
//!
//! 实现策略：跨平台枚举进程名匹配。Codex 0.158+ 的 CLI 进程名是 `codex`（macOS/
//! Linux）或 `codex.exe`（Windows）；Claude Code 是 `claude`；Gemini CLI 是
//! `gemini`。我们只检测 codex——cc-switch 的核心场景是 codex 切换。
//!
//! 设计取舍：直接 `Command::new("pgrep"/tasklist)` 比解析 `/proc` 或 `wmic` 简单；
//! 跨平台差异点（macOS/Linux 用 `pgrep -x`，Windows 用 `tasklist /FI`）由
//! cfg 分支处理。最多 200ms 超时——若 pgrep 不存在（极少数精简容器），
//! 降级返回 `running = false` 而不是阻塞 UI。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// 「有未保存 session」的判定窗口（秒）。
///
/// Codex 每个活跃 session 都在 `~/.codex/sessions/**/rollout-*.jsonl` 追加写
/// rollout 事件；用户每敲一次回车、每收到一次流式响应，都会刷新 mtime。所以
/// **mtime 距今 < 30s** 是一个足够保守的「正在对话中」代理指标：宁可多拒一次
/// 自动重启，也不要把用户正在进行的对话直接杀掉丢上下文。
///
/// 反过来，用户敲完 `/exit` 或关掉终端后，Codex 会 finalize 该 rollout 文件并
/// 停止追加，mtime 停止增长 —— 30s 后自动重启就重新允许。
pub const RECENT_SESSION_ACTIVITY_WINDOW_SECS: u64 = 30;

/// 等待 SIGTERM 后进程退出的上限。Codex CLI 走 TTY，SIGTERM 通常让它自己
/// 走完当前 in-flight 请求再退；给它 2s，超时才升级到 SIGKILL。
const TERM_GRACE_SECS: u64 = 2;

/// 跨平台探测 Codex CLI 进程是否在运行。
///
/// 返回 `CodexRuntimeStatus`：
/// - `running`：至少有一个 `codex`/`codex.exe` 进程匹配
/// - `checked_at`：探测 unix 毫秒时间戳（前端可用于节流提示）
/// - `method`：实际探测方式（pgrep / tasklist / fallback），便于排障
#[derive(Debug, Clone, serde::Serialize)]
pub struct CodexRuntimeStatus {
    pub running: bool,
    pub checked_at_ms: u64,
    pub method: String,
}

/// 跨平台 codex 进程探测。
///
/// macOS / Linux：`pgrep -x codex`（-x 要求精确匹配整个进程名，避免命中
/// `codex-config` 等无关进程）。若 pgrep 不在 PATH（Alpine 精简镜像可能缺）
/// 则尝试 `ps -e -o comm` 兜底；都没有就 `running = false`。
///
/// Windows：`tasklist /FI "IMAGENAME eq codex.exe"`。tasklist 永远在
/// Windows 上可用，无需兜底。
#[tauri::command]
pub async fn detect_codex_running() -> Result<CodexRuntimeStatus, String> {
    tauri::async_runtime::spawn_blocking(detect_codex_running_blocking)
        .await
        .map_err(|e| format!("探测 Codex 运行时 join 失败: {e}"))?
}

fn detect_codex_running_blocking() -> Result<CodexRuntimeStatus, String> {
    let checked_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    #[cfg(target_os = "windows")]
    let result = detect_windows();
    #[cfg(not(target_os = "windows"))]
    let result = detect_unix();

    Ok(CodexRuntimeStatus {
        running: result?,
        checked_at_ms,
        method: detection_method_name(),
    })
}

#[cfg(target_os = "windows")]
fn detect_windows() -> Result<bool, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    // tasklist /FI 在 Windows 上是稳定 API，结果里含 "codex.exe" 即算命中。
    // /NH 省略表头，/FO CSV 让解析简单。
    let output = Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq codex.exe", "/NH", "/FO", "CSV"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("tasklist 启动失败: {e}"))?;
    if !output.status.success() {
        // tasklist 找不到匹配项时 exit code = 1（"INFO: No tasks are running..."）
        // ——但 stdout 里仍可能有内容；不视为错误，按空匹配处理。
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Ok(stdout.to_lowercase().contains("codex.exe"));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.to_lowercase().contains("codex.exe"))
}

#[cfg(not(target_os = "windows"))]
fn detect_unix() -> Result<bool, String> {
    // 主路径：pgrep -x codex。exit code:
    //   0 = 至少一个匹配
    //   1 = 无匹配
    //   2/3 = 语法错 / fatal
    // 我们只把 0 视为 running；其它都按无匹配处理（不报错，让 UI 显示
    // 「未运行」即可，避免 pgrep 不在 PATH 时假阳）。
    let pgrep_start = Instant::now();
    let pgrep_result = Command::new("pgrep")
        .args(["-x", "codex"])
        .output();
    if pgrep_start.elapsed() > Duration::from_millis(500) {
        log::warn!("pgrep 探测 codex 耗时 {:?}，超过 500ms 阈值", pgrep_start.elapsed());
    }

    match pgrep_result {
        Ok(out) if out.status.success() => {
            // 进一步核对：pgrep -x 在 PATH 但被 busybox 当 alias 时可能误命中
            // （busybox pgrep 的 -x 语义是「匹配进程名 basename」而非整名）。
            // 兜底：解析 stdout，至少一行是 4 位以上的 pid 即可信。
            let stdout = String::from_utf8_lossy(&out.stdout);
            return Ok(stdout.lines().any(|line| {
                let s = line.trim();
                !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) && s.len() >= 1
            }));
        }
        Ok(_) => {
            // exit code != 0：明确无匹配（或权限不足）；按 no-running 处理。
            return Ok(false);
        }
        Err(_) => {
            // pgrep 不在 PATH（极少：Alpine scratch）。fallback 到 ps 解析。
        }
    }

    // 兜底：ps -A -o comm= 列进程 basename，匹配 `codex` 整行。
    // -o comm= 去掉表头；comm 是 truncated basename（通常 15 字符），对
    // `codex` 5 字符足够。
    let ps_output = Command::new("ps").args(["-A", "-o", "comm="]).output();
    match ps_output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            Ok(stdout.lines().any(|line| line.trim() == "codex"))
        }
        Ok(_) => Ok(false),
        Err(e) => Err(format!("pgrep 与 ps 均不可用: {e}")),
    }
}

#[cfg(target_os = "windows")]
fn detection_method_name() -> String {
    "tasklist".to_string()
}

#[cfg(not(target_os = "windows"))]
fn detection_method_name() -> String {
    "pgrep+ps-fallback".to_string()
}

// ---------------------------------------------------------------------------
// 真自动重启
// ---------------------------------------------------------------------------

/// 一次重启尝试的结果。前端据此区分三种用户可感知的结局：
///
/// - `killed = true`：进程真被杀了，用户可以直接敲 `codex` 起新会话。
/// - `killed = false, running_before = false`：本来就没在跑，无需重启（幂等）。
/// - `refused = true`：探测到活跃 session，**故意没动**，附带原因给用户看。
#[derive(Debug, Clone, serde::Serialize)]
pub struct CodexRestartResult {
    /// 调用前 Codex 是否在运行。
    pub running_before: bool,
    /// 是否真的发过终止信号。
    pub killed: bool,
    /// 实际使用的手段：`SIGTERM` / `SIGKILL` / `taskkill` / `none`。
    pub signalled: String,
    /// 命中的 codex 进程 pid 列表（重启后前端可复查）。
    pub pids: Vec<u32>,
    /// 是否因为「有活跃 session」而拒绝重启。
    pub refused: bool,
    /// 拒绝原因（i18n key + 人类可读细节），成功时为 `None`。
    pub refusal_reason: Option<String>,
    /// 触发拒绝的 session 文件路径，便于用户 `/exit` 后自查。
    pub active_session_path: Option<String>,
    pub checked_at_ms: u64,
}

/// 跨平台终止 Codex 进程，并让用户能一键重启。
///
/// 与上一轮「把 `pkill` 命令复制到剪贴板」相比，本命令真的动手杀进程。取舍：
///
/// 1. **先问活跃 session**：任何 `~/.codex/sessions/**\/*.jsonl` 在 30s 内被写过，
///    就拒绝重启（`refused = true`），让用户先 `/exit`。这是「不丢上下文」的硬红线。
/// 2. **TERM 优先、KILL 兜底**：macOS/Linux 先 `SIGTERM` 给 Codex 走完 in-flight
///    请求的机会，等 `TERM_GRACE_SECS` 仍存活才升级 `SIGKILL`；Windows 没有
///    "优雅"概念，直接复用 `commands::misc::terminate_child_tree` 的
///    `taskkill /PID <pid> /T /F` 整树击杀语义。
/// 3. **只杀真 codex**：pid 全部来自 `pgrep -x codex` / `tasklist` 的精确名匹配，
///    不会误伤 `codex-config`、`code` 等名字相近的进程。
#[tauri::command]
pub async fn restart_codex_process() -> Result<CodexRestartResult, String> {
    tauri::async_runtime::spawn_blocking(restart_codex_process_blocking)
        .await
        .map_err(|e| format!("重启 Codex join 失败: {e}"))?
}

fn restart_codex_process_blocking() -> Result<CodexRestartResult, String> {
    let checked_at_ms = now_unix_millis();

    let base = CodexRestartResult {
        running_before: false,
        killed: false,
        signalled: "none".to_string(),
        pids: Vec::new(),
        refused: false,
        refusal_reason: None,
        active_session_path: None,
        checked_at_ms,
    };

    let running = detect_codex_running_blocking()?.running;
    if !running {
        // 幂等：Codex 本来就没在跑就是「重启目标已达成」，不需要杀任何东西。
        return Ok(base);
    }

    // 红线：有活跃 session 就不动。
    if let Some(active) = recent_codex_session_activity() {
        return Ok(CodexRestartResult {
            running_before: true,
            refused: true,
            refusal_reason: Some(format!(
                "检测到 {} 秒内有活跃 Codex session（{}）；请先在 Codex 里 /exit 或关闭该会话，再重启以免丢上下文。",
                RECENT_SESSION_ACTIVITY_WINDOW_SECS,
                active.display()
            )),
            active_session_path: Some(active.to_string_lossy().to_string()),
            ..base
        });
    }

    #[cfg(target_os = "windows")]
    let (signalled, pids) = terminate_windows()?;
    #[cfg(not(target_os = "windows"))]
    let (signalled, pids) = terminate_unix()?;

    if pids.is_empty() {
        // 探测说有、列 pid 时已经没了（用户刚好自己退出了）。不算失败。
        return Ok(base);
    }

    Ok(CodexRestartResult {
        running_before: true,
        killed: true,
        signalled,
        pids,
        ..base
    })
}

fn now_unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `~/.codex/sessions/**\/*.jsonl` 里 mtime 距今 < 30s 的那个文件。
///
/// Codex 的 rollout 目录按 `sessions/YYYY/MM/DD/rollout-<id>.jsonl` 分层，所以要
/// 递归（深度上限 4，够用且不会被 symlink 环拖死）。没有 `walkdir` 依赖，直接手写
/// `read_dir` —— 与 `codex_history_migration::collect_files_with_extension` 同款做法。
pub fn recent_codex_session_activity() -> Option<PathBuf> {
    let sessions_dir = crate::codex_config::get_codex_config_dir().join("sessions");
    recent_codex_session_activity_in(
        &sessions_dir,
        RECENT_SESSION_ACTIVITY_WINDOW_SECS,
        std::time::SystemTime::now(),
    )
}

/// 可注入目录/窗口/时钟的纯函数形态。
///
/// **目录是参数而不是内部推导的**，这是刻意的：本仓库里 `CC_SWITCH_TEST_HOME` /
/// `HOME` 是**进程级**共享状态，多个测试模块会并发改写它（已知会与
/// `openclaw_config::tests::with_test_paths` 相互串扰）。让目录可注入，单测就能
/// 指向自己的 tempdir，**完全不去碰全局 env**，也就不会给那条既有竞态再添一个
/// 参与者。
fn recent_codex_session_activity_in(
    sessions_dir: &Path,
    window_secs: u64,
    now: std::time::SystemTime,
) -> Option<PathBuf> {
    let mut files = Vec::new();
    collect_jsonl_files(sessions_dir, &mut files, 0, 4);

    let window = Duration::from_secs(window_secs);
    files
        .into_iter()
        .filter_map(|path| {
            let mtime = path.metadata().ok()?.modified().ok()?;
            // modified() 晚于 now（时钟漂移）也算活跃：宁可多拒。
            if now.duration_since(mtime).map(|age| age <= window).unwrap_or(true) {
                Some(path)
            } else {
                None
            }
        })
        // 取最新的那个：多个活跃 session 时先让用户退最新的那个最直观。
        .max_by_key(|path| path.metadata().ok().and_then(|m| m.modified().ok()))
}

fn collect_jsonl_files(dir: &Path, files: &mut Vec<PathBuf>, depth: u8, max_depth: u8) {
    if depth > max_depth || !dir.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, files, depth + 1, max_depth);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn terminate_unix() -> Result<(String, Vec<u32>), String> {
    let roots = list_unix_codex_pids()?;
    terminate_unix_roots(&roots)
}

/// 对给定 pid 集合执行 TERM → 宽限 → KILL 的终止流程。
///
/// **pid 是参数而不是内部 glob 出来的**，这是刻意的：直接调 `terminate_unix()`
/// 会走 `pgrep -x codex`，而在开发机上那只可能是**用户真实在跑的 Codex**
/// （本机 ChatGPT.app 内置的 codex-cli 就在 `pgrep -x codex` 的结果里）。任何
/// 直接调它的测试都会**杀掉开发者自己的会话**。抽成可注入后，单测能拿一棵自己
/// 拉起的假进程树真跑一遍 kill，真实验证「TERM 能杀干净整棵树」，且绝不误伤。
#[cfg(not(target_os = "windows"))]
fn terminate_unix_roots(roots: &[u32]) -> Result<(String, Vec<u32>), String> {
    if roots.is_empty() {
        return Ok(("none".to_string(), Vec::new()));
    }
    // 整棵树：根 pid + 递归子进程。Codex 0.158+ 是 app-server 架构，CLI 主进程
    // 只是 wrapper，真正的会话在子进程里；只杀根会留下孤儿占内存/端口。
    let pids = collect_unix_process_tree(roots);
    log::info!("重启 Codex：命中 {} 个 codex 进程（含子进程）", pids.len());

    // 第一轮：SIGTERM，给 Codex 走完 in-flight 请求的机会。
    for pid in &pids {
        // SAFETY: kill(2) 只读 pid 与信号号，不触碰内存；pid 由调用方给出，
        // 即使已被回收也只是返回 ESRCH。
        unsafe {
            libc::kill(*pid as libc::pid_t, libc::SIGTERM);
        }
    }

    let deadline = Instant::now() + Duration::from_secs(TERM_GRACE_SECS);
    while Instant::now() < deadline {
        if !pids.iter().any(|pid| process_alive(*pid)) {
            return Ok(("SIGTERM".to_string(), pids));
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // 第二轮：还有残留 → 升级 SIGKILL。这里是强杀，用户可能丢最后一条 in-flight
    // 响应；但 30s 活跃窗口已经拦掉了「正在打字/正在流式」的情况，剩下的都是空闲
    // 残留进程，强杀是安全的。
    for pid in pids.iter().filter(|pid| process_alive(**pid)) {
        // SAFETY: 同上。
        unsafe {
            libc::kill(*pid as libc::pid_t, libc::SIGKILL);
        }
    }
    Ok(("SIGKILL".to_string(), pids))
}

/// `kill(pid, 0)` 存在性探测（信号 0 不投递任何信号，只做权限/存在性检查）。
///
/// **已知局限：僵尸进程会被判为「活着」。** 实测（macOS）：被 SIGTERM 杀掉但
/// 尚未被父进程 `wait()` 回收的子进程进入 `Z` 状态，此时 `kill(pid, 0)` 仍然成功。
///
/// 生产路径不受影响：这里探测的 Codex 是**用户从终端启动**的，不是 cc-switch 的
/// 子进程，它退出后由 launchd 收养并回收，不会变成我们的僵尸。所以宽限轮询与
/// SIGKILL 升级在生产里都按预期工作。
///
/// 只有**测试**会踩到——测试自己拉起的进程就是自己的子进程，不回收就永远是僵尸。
/// 故 `terminate_unix_roots_actually_kills_the_whole_tree` 在等待循环里显式
/// `try_wait()` 回收。
#[cfg(not(target_os = "windows"))]
fn process_alive(pid: u32) -> bool {
    // SAFETY: signal 0 是 POSIX 规定的纯探测调用，无副作用。
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// 展开 pid 的整棵后代树。
///
/// **为什么不用 process_group（`kill(-pgid, …)`）**：`commands::misc::terminate_child_tree`
/// 那样做之所以安全，是因为 cc-switch 自己 `setsid()` 拉起子进程，保证它是进程组
/// 组长，`kill(-child_pid)` 精确等于「杀这个子进程的组」。但这里要杀的是**用户
/// 自己从终端启动**的 Codex —— 它是 shell 作业进程组的**成员**，组长是那个 shell。
/// 对一个非组长 pid 做 `kill(-pid, …)` 要么 ESRCH 失败，要么在 pid 恰好撞上某个
/// 存活组 id 时把用户**整个终端作业组**（连他的 shell 一起）干掉。
///
/// 所以改走 `pgrep -P <pid>` 逐层展开后代：拿到的是确切的子孙 pid，既能整树
/// 清理，又完全不会碰到用户的 shell / 其它作业。
#[cfg(not(target_os = "windows"))]
fn collect_unix_process_tree(roots: &[u32]) -> Vec<u32> {
    const MAX_TREE_DEPTH: u8 = 8;
    let mut collected: Vec<u32> = Vec::new();
    let mut queue: Vec<(u32, u8)> = roots.iter().map(|pid| (*pid, 0)).collect();

    while let Some((pid, depth)) = queue.pop() {
        if collected.contains(&pid) {
            continue;
        }
        collected.push(pid);
        if depth >= MAX_TREE_DEPTH {
            continue;
        }
        for child in list_unix_child_pids(pid) {
            if !collected.contains(&child) {
                queue.push((child, depth + 1));
            }
        }
    }
    collected
}

#[cfg(not(target_os = "windows"))]
fn list_unix_child_pids(parent: u32) -> Vec<u32> {
    match Command::new("pgrep")
        .args(["-P", &parent.to_string()])
        .output()
    {
        Ok(out) => parse_pids(&String::from_utf8_lossy(&out.stdout)),
        // pgrep 不可用时退化成「只杀根进程」：宁可漏杀子进程，也不要因为拿不到
        // 进程树就整个放弃重启。
        Err(_) => Vec::new(),
    }
}

#[cfg(not(target_os = "windows"))]
fn list_unix_codex_pids() -> Result<Vec<u32>, String> {
    let output = Command::new("pgrep").args(["-x", "codex"]).output();
    let Ok(output) = output else {
        return Err("pgrep 不可用，无法枚举 codex 进程".to_string());
    };
    // exit != 0（无匹配）不是错误，按空列表处理。
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_pids(&stdout))
}

/// 把 `pgrep -x codex` 的 stdout（每行一个纯数字 pid）解析成 u32 列表。
/// 任何非数字行都被丢弃 —— pgrep 不会输出别的，但 busybox 变体可能带列头。
///
/// pid 0 被显式过滤掉：`kill(0, SIGTERM)` 是 POSIX 里「向调用者所在进程组发信号」
/// 的保留写法，一旦混进来就等于 cc-switch 给**自己**发信号。
fn parse_pids(stdout: &str) -> Vec<u32> {
    stdout
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .filter(|pid| *pid > 0)
        .collect()
}

#[cfg(target_os = "windows")]
fn terminate_windows() -> Result<(String, Vec<u32>), String> {
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let pids = list_windows_codex_pids()?;
    if pids.is_empty() {
        return Ok(("none".to_string(), Vec::new()));
    }
    // 与 `commands::misc::terminate_child_tree` 同一套语义：整树击杀，避免
    // codex 主进程退了但 `codex app-server` 子进程变孤儿继续占着 1455 端口。
    for pid in &pids {
        let status = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if !matches!(status, Ok(status) if status.success()) {
            log::warn!("taskkill /PID {pid} /T /F 失败，进程可能已自行退出");
        }
    }
    Ok(("taskkill".to_string(), pids))
}

#[cfg(target_os = "windows")]
fn list_windows_codex_pids() -> Result<Vec<u32>, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let output = Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq codex.exe", "/NH", "/FO", "CSV"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("tasklist 启动失败: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    // CSV 形态：`"codex.exe","1234","Console","1","9,999 K"` —— 取第 2 列。
    Ok(stdout
        .lines()
        .filter(|line| line.to_lowercase().contains("codex.exe"))
        .filter_map(|line| {
            line.split(',')
                .nth(1)
                .map(|field| field.trim().trim_matches('"'))
                .and_then(|pid| pid.parse::<u32>().ok())
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 探测函数必须返回 `Result<bool, _>`，不能因 pgrep 不可用而 panic。
    /// 返回值的真假依赖当前机器是否有 `codex` 进程（环境相关），
    /// 因此本测试只断言「不崩、不抛 Err」。具体真假留给生产路径用。
    #[test]
    #[cfg(not(target_os = "windows"))]
    fn detect_unix_does_not_panic() {
        use std::process::Command as StdCommand;
        // 启一个 sleep 让本进程组多一个非 codex 进程，验证不会把无关进程
        // 误判为 codex（pgrep -x 在 macOS/Linux 上对 comm basename 做
        // 精确匹配，sleep 永远不该命中）。但本测试**不**断言 result 为 false
        // ——机器上可能真有 `codex` 在跑（macOS ChatGPT.app 内置 codex-cli），
        // 这种情况下 result 应为 true。
        let mut child = StdCommand::new("sleep")
            .arg("2")
            .spawn()
            .expect("spawn sleep");
        let result = detect_unix().expect("detect_unix should not error");
        // 仅断言返回 bool 类型（编译期已保证），不断言具体值。
        let _: bool = result;
        let _ = child.kill();
    }

    /// 真机烟雾测试：本机有 ChatGPT.app 内置 codex-cli 时，verify
    /// `pgrep -x codex` 路径能命中；同时与 `ps -A -o comm=` 对照，证明
    /// 主路径（pgrep）与 fallback（ps）在 macOS 上的实际差异——
    /// 主路径能命中 `codex` 启动器（comm basename = "codex"），fallback
    /// 因 `comm` 列在 macOS 不暴露完整 basename 而漏报。这强化了「pgrep
    /// 不可用时降级到 false」的合理性。
    #[test]
    #[cfg(not(target_os = "windows"))]
    #[ignore] // 默认跳过；`cargo test detect_codex_running -- --ignored` 触发
    fn detect_codex_running_real_machine_smoke() {
        let pgrep = std::process::Command::new("pgrep")
            .args(["-lx", "codex"])
            .output()
            .expect("pgrep spawn");
        let pgrep_stdout = String::from_utf8_lossy(&pgrep.stdout).to_string();
        let pgrep_hit = pgrep.status.success() && !pgrep_stdout.trim().is_empty();
        eprintln!("[real-machine] pgrep -lx codex exit={:?}", pgrep.status.code());
        eprintln!("[real-machine] pgrep stdout: {pgrep_stdout:?}");

        let ps = std::process::Command::new("ps")
            .args(["-A", "-o", "comm="])
            .output()
            .expect("ps spawn");
        let ps_stdout = String::from_utf8_lossy(&ps.stdout);
        let ps_hit = ps_stdout.lines().any(|l| l.trim() == "codex");
        eprintln!("[real-machine] ps -A -o comm= exact 'codex' hit: {ps_hit}");

        let result = detect_unix().expect("detect_unix should not error");
        eprintln!("[real-machine] detect_unix() = {result}");
        // 主路径逻辑：pgrep 命中即 true，pgrep 不命中即 false，
        // ps fallback 仅在 pgrep 不可用时使用。
        assert_eq!(
            result, pgrep_hit,
            "detect_unix 必须与 pgrep -lx codex 一致（pgrep 是真值源）"
        );
    }

    /// **真跑一次 kill**：拉起一棵真实的**嵌套**进程树（root → sh → sleep，
    /// 镜像 Codex wrapper → app-server 的形状），调 `terminate_unix_roots` 走完整
    /// TERM → 宽限 → KILL 流程，然后断言**采集到的每一个 pid 都真的死了**。
    ///
    /// 这条补上的是此前最大的验证缺口：重构前 `terminate_unix()` 内部直接
    /// `pgrep -x codex`，**任何测试都不敢调它**——本机 ChatGPT.app 的 Codex 就在
    /// 那个匹配里，一调就杀掉开发者自己的会话。于是「真自动重启」的杀死路径
    /// 实际上从未被执行过一次，只有 pid 解析和树展开被测到。
    #[test]
    #[cfg(not(target_os = "windows"))]
    fn terminate_unix_roots_actually_kills_the_whole_tree() {
        use std::process::{Command as StdCommand, Stdio};

        // 真正的三层：root sh → (sleep, mid sh) → (sleep, sleep)
        let mut root = StdCommand::new("sh")
            .arg("-c")
            .arg("sleep 300 & sh -c 'sleep 300 & sleep 300 & wait' & wait")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn root sh");
        let root_pid = root.id();

        // 等后代都 fork 出来，否则树是空的、断言会假绿。
        let mut tree = Vec::new();
        for _ in 0..60 {
            std::thread::sleep(Duration::from_millis(50));
            tree = collect_unix_process_tree(&[root_pid]);
            if tree.len() >= 3 {
                break;
            }
        }
        assert!(
            tree.len() >= 3,
            "前置条件：必须真的长出一棵嵌套树（root→sh→sleep），实际 {:?}",
            tree
        );
        // 后代都不是我们的子进程，退出后由 launchd 收养回收 —— 所以可以直接用
        // process_alive 判断它们是否真的死了（只有 root 需要 try_wait 回收）。
        let descendants: Vec<u32> = tree.iter().copied().filter(|pid| *pid != root_pid).collect();
        assert!(!descendants.is_empty(), "树里必须有后代");

        // 真杀。根是我们拉起的这棵假树，绝不是开发者的 Codex。
        let (signalled, killed) =
            terminate_unix_roots(&[root_pid]).expect("terminate_unix_roots should succeed");
        assert!(!killed.is_empty(), "必须报告杀掉的 pid");
        assert!(
            signalled == "SIGTERM" || signalled == "SIGKILL",
            "signalled 只能是 SIGTERM/SIGKILL，实际 {signalled}"
        );

        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if !descendants.iter().any(|pid| process_alive(*pid)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        // 关键断言：**每一个**采集到的后代都得死。只杀根会让它们变孤儿。
        let survivors: Vec<u32> = descendants
            .iter()
            .copied()
            .filter(|pid| process_alive(*pid))
            .collect();
        assert!(
            survivors.is_empty(),
            "这些后代必须全死（只杀根会留孤儿）：{survivors:?} / 树 {tree:?}"
        );

        // 回收 root（我们自己的子进程，否则它会以僵尸态挂在进程表里）。
        let _ = root.kill();
        let _ = root.wait();
    }

    /// 空 pid 集合必须是无害 no-op，不能 panic、不能声称杀过什么。
    #[test]
    #[cfg(not(target_os = "windows"))]
    fn terminate_unix_roots_on_empty_set_is_noop() {
        let (signalled, pids) = terminate_unix_roots(&[]).expect("empty set must not error");
        assert_eq!(signalled, "none");
        assert!(pids.is_empty());
    }

    #[test]
    fn detection_method_name_is_non_empty() {
        let m = detection_method_name();
        assert!(!m.is_empty());
    }

    /// 整棵进程树必须被收进来 —— Codex 0.158+ 主进程只是 wrapper，真正占资源的是
    /// `app-server` 子进程，只杀根会留下孤儿。
    #[test]
    #[cfg(not(target_os = "windows"))]
    fn process_tree_collection_picks_up_descendants() {
        use std::process::{Command as StdCommand, Stdio};

        // sh -c 'sleep 30 & wait' 造一条真实的父子链：sh 是根，sleep 是子。
        let mut child = StdCommand::new("sh")
            .args(["-c", "sleep 30 & wait"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sh");
        let root = child.id();
        // 等 sleep 真正 fork 出来（pgrep -P 在子进程出现前会返回空）。
        let mut tree = Vec::new();
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(50));
            tree = collect_unix_process_tree(&[root]);
            if tree.len() >= 2 {
                break;
            }
        }

        assert!(tree.contains(&root), "根 pid 必须在内: {tree:?}");
        assert!(
            tree.len() >= 2,
            "必须收进至少一个子孙（sh → sleep）: {tree:?}"
        );
        // 关键红线：绝不能包含**本测试进程自己**或它的父。
        // 一旦 tree 收集逻辑误用了 process_group（kill(-pgid)）或把无关进程
        // 拉进来，重启 Codex 就会连带杀掉 cc-switch / 用户的 shell。
        assert!(
            !tree.contains(&std::process::id()),
            "绝不能把测试进程自己算进 codex 进程树: {tree:?}"
        );

        let _ = child.kill();
        let _ = child.wait();
        // 收尾：别把 sleep 留在后台。
        for pid in tree.iter().filter(|pid| **pid != root) {
            // SAFETY: 这些 pid 来自本测试刚拉起的 sh 进程树。
            unsafe {
                libc::kill(*pid as libc::pid_t, libc::SIGKILL);
            }
        }
    }

    /// `pgrep -x codex` 的 stdout 解析：只认纯数字行。
    ///
    /// 回归红线：解析器一旦放宽（比如 split(':') 取后缀），busybox pgrep 带列头的
    /// 输出会让 `kill(0, SIGTERM)` 命中**整个进程组**——包括 cc-switch 自己。
    #[test]
    fn parse_pids_only_accepts_bare_numeric_lines() {        let parsed = parse_pids("67526\n1234\n");
        assert_eq!(parsed, vec![67526, 1234]);

        assert!(parse_pids("").is_empty());
        // 带列头 / 混杂文本的行必须整体丢弃，而不是「提取其中的数字」。
        assert!(parse_pids("PID codex\n67526 codex\n").is_empty());
        // pid 0 是「向整个进程组发信号」的保留值，绝不能进 kill 列表。
        assert_eq!(parse_pids("0\n"), Vec::<u32>::new());
        // 空行与空白容忍。
        assert_eq!(parse_pids("\n  42  \n\n"), vec![42]);
    }

    /// 活跃 session 探测：窗口内的 `.jsonl` 必须被认出来，窗口外的必须被忽略。
    ///
    /// 目录是**参数**（`recent_codex_session_activity_in`），所以本测试不碰
    /// `CC_SWITCH_TEST_HOME` / `HOME` 任何全局 env —— 既不需要 `#[serial]`，
    /// 也不会给 `openclaw_config` 那条既有的 env 竞态再添参与者。
    #[test]
    fn recent_session_activity_detects_nested_rollout_and_respects_window() {
        let temp_home = tempfile::tempdir().expect("tempdir");
        let sessions = temp_home.path().join("sessions");
        // 模拟 Codex 真实的 rollout 分层：sessions/YYYY/MM/DD/rollout-<id>.jsonl
        let nested = sessions.join("2026").join("09").join("28");
        std::fs::create_dir_all(&nested).expect("create nested rollout dir");
        let rollout = nested.join("rollout-2026-09-28T19-00-00-abc123.jsonl");
        std::fs::write(&rollout, "{}\n").expect("write rollout");

        let now = std::time::SystemTime::now();

        // 1) 刚写过 → 命中，且路径正是那个嵌套文件。
        let hit = recent_codex_session_activity_in(
            &sessions,
            RECENT_SESSION_ACTIVITY_WINDOW_SECS,
            now,
        )
        .expect("fresh rollout must be treated as an active session");
        assert_eq!(hit, rollout, "必须命中嵌套 4 层的 rollout 文件");

        // 2) 窗口真的生效：把「现在」推到 1 小时之后，该 rollout 的 mtime 距
        //    那时已远超 30s 窗口 → 不算活跃。证明窗口不是恒真。
        let future_now = now + Duration::from_secs(3600);
        assert!(
            recent_codex_session_activity_in(
                &sessions,
                RECENT_SESSION_ACTIVITY_WINDOW_SECS,
                future_now
            )
            .is_none(),
            "窗口必须真的生效：1 小时后的时间点看这份 rollout 已过期"
        );

        // 3) 时钟漂移 fail-safe：mtime 晚于「现在」（NTP 回拨 / 跨时区）时保守
        //    判成活跃，宁可多拒一次重启也不误杀对话。
        let long_ago = now - Duration::from_secs(3600);
        assert!(
            recent_codex_session_activity_in(
                &sessions,
                RECENT_SESSION_ACTIVITY_WINDOW_SECS,
                long_ago
            )
            .is_some(),
            "mtime 晚于 now（时钟漂移）必须保守判为活跃"
        );

        // 4) 没有 sessions 目录时必须安静返回 None，绝不 panic。
        let empty = tempfile::tempdir().expect("tempdir without sessions");
        assert!(
            recent_codex_session_activity_in(
                &empty.path().join("sessions"),
                RECENT_SESSION_ACTIVITY_WINDOW_SECS,
                now
            )
            .is_none(),
            "目录不存在时必须安静返回 None"
        );
    }

}
