//! tailnet 身份证据（`tailscale whois`）
//!
//! 目标：给审批链路一个**可判定、可降级、有限时**的对端身份来源。
//!
//! 本机 Tailscale 是 GUI 变体：`/usr/local/bin/tailscale` 是 shim，进程名是
//! `Tailscale`，`tailscaled.sock` 不存在，LocalAPI 不在 unix socket 上。照抄
//! 「非 GUI 变体」那套走 LocalAPI 的文档，在这台机器上整段是死代码——唯一的通用
//! 入口就是 CLI。所以本文件只认 CLI。
//!
//! 三条约束决定了这里的形状：
//!
//! 1. **不引入依赖**：`tokio` 没有 `process` feature，且本轮不改 `Cargo.toml`。
//!    所以用 `std::process::Command` + 同任务内 `try_wait()` 轮询，整体被
//!    `tokio::time::timeout` 包住。**禁止**写成「`spawn_blocking(child.wait())`
//!    外面再套 timeout」——那种写法一旦超时，JoinHandle 被丢弃，子进程就再也没人
//!    kill/wait，2 秒的等待换成了一个永久泄漏。
//! 2. **判定必须三态**：查得到 / 明确不在 tailnet / 判不了。第三态不是错误，是
//!    「照常审批、不 panic、不阻塞」的合法结果。
//! 3. **超时必须真的杀掉子进程**：光靠 `timeout` 返回、而把 child 留在原地，等于
//!    把一次有界的等待换成一次无界的泄漏。这里用 [`ReapOnDrop`] 兜底：正常路径由
//!    [`run_with_deadline`] 在超时时**显式** kill + wait；万一外层 future 被取消、
//!    代码根本跑不到那一行，Drop 会接手收尸。

use super::events::SourceKind;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// 判定超时的墙钟上限。
///
/// 对齐 `mod.rs` 里外层 3s 等待的风格，这里取更紧的 2s：审批是交互路径，对端早就
/// 该应答了；等满 2 秒还没有结论，就按「判不了」处理，而不是让人继续等。
pub const WHOIS_TIMEOUT: Duration = Duration::from_secs(2);

/// 轮询间隔。只影响「进程结束了」的感知精度（10ms），不影响总耗时上限。
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// 二进制名：PATH 层拼 `dir/tailscale` 时用。
const BIN_NAME: &str = "tailscale";

/// 显式指定 tailscale 可执行文件的覆盖变量。
///
/// GUI 启动的进程常常不带 `/usr/local/bin`（见 [`resolve_bin_with`] 的阶梯说明），
/// 运维需要一个不依赖 PATH 的注入口。
pub const BIN_OVERRIDE_ENV: &str = "CC_SWITCH_TAILSCALE_BIN";

/// 绝对候选路径。本机装的是 GUI 变体，CLI shim 在这两个位置。
#[cfg(not(windows))]
const DEFAULT_CANDIDATES: [&str; 2] = ["/usr/local/bin/tailscale", "/opt/homebrew/bin/tailscale"];

/// Windows 上的绝对候选路径。
///
/// 阶梯②排在 PATH 之前的原因与 unix 侧完全一致：GUI 启动的进程常常不带
/// `C:\Program Files\Tailscale` 的 PATH 项，那不是罕见情况而是每次都会发生。
/// 路径写死成默认值而不是拼 `ProgramFiles` 环境变量：与 unix 侧同构，且
/// Program Files 被重定向时仍是同一个可执行文件。
#[cfg(windows)]
const DEFAULT_CANDIDATES: [&str; 2] = [
    r"C:\Program Files\Tailscale\tailscale.exe",
    r"C:\Program Files (x86)\Tailscale\tailscale.exe",
];

/// `tailscale whois --json` 提取出的对端身份字段。
///
/// 字段一律是 `String` 而非 `Option<String>`：CLI 的 JSON 在版本之间换过键名大小写
/// 与嵌套位置，取不到的留空串即可，由调用方按「有值才算证据」处理。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhoisFields {
    /// 节点名（tailnet 内唯一）
    pub node_name: String,
    /// 节点稳定 ID（改名不变，可作长期标识）
    pub node_stable_id: String,
    /// 登录名（如 alice@example.com）
    pub login_name: String,
    /// 账号 ID（用于比对 `is_foreign_account`）
    pub user_id: String,
}

/// 一次子进程调用的结果。
///
/// `reaped` / 失败原因只在测试与诊断日志里被读，非测试构建下是 dead field。
#[derive(Debug)]
#[allow(dead_code)]
enum Run {
    /// 进程自己跑完了。`code` 是退出码；Unix 上被信号杀死时是 `None`。
    Exited {
        code: Option<i32>,
        stdout: String,
        stderr: String,
    },
    /// 超过 deadline。`reaped` 记录是否观察到 `try_wait()` 返回 `Ok(Some(_))`
    /// ——这是「子进程确实被收走、没留僵尸」的唯一可观测证据。
    TimedOut { reaped: bool },
    /// 起不来（找不到 bin、没有执行权限等），或 `try_wait()` 报错。
    Failed(String),
}

/// 持有 `Child`，Drop 时 kill + wait。
///
/// 这是「超时真的杀掉子进程」这条保证的兜底。正常路径由 [`run_with_deadline`]
/// 在超时时显式 kill + wait（同一任务内，不依赖 Drop）；而当外层 future 被
/// [`tokio::time::timeout`] 取消时，代码根本执行不到那一行，只能靠 Drop 收尸。
/// 两条路径都覆盖，才谈得上「不留僵尸」。
struct ReapOnDrop(Option<Child>);

impl ReapOnDrop {
    fn new(child: Child) -> Self {
        Self(Some(child))
    }

    /// 取出 child 并解除 Drop 义务，由调用方自己 kill + wait
    /// （需要在收尸之后继续观测 `try_wait()` 时用）。
    fn take(&mut self) -> Option<Child> {
        self.0.take()
    }
}

impl Drop for ReapOnDrop {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// 把子进程的 stdout / stderr 抽到独立线程上读干。
///
/// 不这么做，`try_wait()` 看到的是假的「还没结束」：子进程写满 64KB 管道缓冲后会
/// 一直阻塞在 write 上、永远不退出，轮询就只能一路等到超时。抽走之后进程结束与
/// 否就只取决于它自己想不想退出。被 kill 时两个线程都会拿到 EOF。
fn spawn_drainers(
    child: &mut Child,
) -> (
    Option<mpsc::Receiver<String>>,
    Option<mpsc::Receiver<String>>,
) {
    fn drain<R: std::io::Read + Send + 'static>(
        reader: Option<R>,
    ) -> Option<mpsc::Receiver<String>> {
        let mut reader = reader?;
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = reader.read_to_end(&mut buf);
            let _ = tx.send(String::from_utf8_lossy(&buf).into_owned());
        });
        Some(rx)
    }

    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    (stdout, stderr)
}

fn recv_all(rx: Option<mpsc::Receiver<String>>) -> String {
    rx.map(|rx| rx.recv().unwrap_or_default())
        .unwrap_or_default()
}

/// 起一个子进程，在 `timeout` 内只靠非阻塞的 `try_wait()` 轮询它的结束。
///
/// 刻意不用 `Command::output()`：它内部就是 `wait()`，一旦被外层 timeout 取消，
/// 子进程连同那个 wait 一起被丢下。spawn 之后这里只碰 `try_wait()`；
/// 等到了就回收 stdout/stderr，等不到就 kill + wait。
async fn run_with_deadline(bin: &Path, args: &[&str], timeout: Duration) -> Run {
    let mut child = match Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => return Run::Failed(format!("spawn {bin:?} failed: {e}")),
    };
    let (stdout_rx, stderr_rx) = spawn_drainers(&mut child);

    let deadline = Instant::now() + timeout;
    let mut guard = ReapOnDrop::new(child);
    loop {
        match guard
            .0
            .as_mut()
            .expect("循环期间 child 未被取走")
            .try_wait()
        {
            Ok(Some(status)) => {
                // try_wait 已经收过尸，这里只是把状态补齐。
                let Some(mut child) = guard.take() else {
                    return Run::Failed("child 已被取走".into());
                };
                let _ = child.wait();
                return Run::Exited {
                    code: status.code(),
                    stdout: recv_all(stdout_rx),
                    stderr: recv_all(stderr_rx),
                };
            }
            Ok(None) => {}
            Err(e) => return Run::Failed(format!("try_wait failed: {e}")),
        }

        if Instant::now() >= deadline {
            // 超时：同一任务内显式 kill + wait。仅仅让外层 future 返回是收不了尸的。
            let Some(mut child) = guard.take() else {
                return Run::Failed("child 已被取走".into());
            };
            let _ = child.kill();
            let _ = child.wait();
            let reaped = matches!(child.try_wait(), Ok(Some(_)));
            return Run::TimedOut { reaped };
        }

        // 只有 spawn 本身是阻塞的；轮询间隙用异步 sleep 让出执行权。
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// [`run_with_deadline`] 的有界版本：外面再兜一层 `timeout`。
///
/// 内层自带 deadline，所以绝大多数情况下是内层先收敛；真走到外层这个分支（内层
/// future 被别的取消路径打断等），`ReapOnDrop` 的 Drop 已经把子进程收走了，但此时
/// `Child` 已经不在我们手里——看不到的东西不能当证据，故 `reaped` 记 `false`。
async fn run_bounded(bin: &Path, args: &[&str], timeout: Duration) -> Run {
    match tokio::time::timeout(timeout, run_with_deadline(bin, args, timeout)).await {
        Ok(run) => run,
        Err(_) => Run::TimedOut { reaped: false },
    }
}

/// 大小写不敏感地在 JSON object 里取一个键。
fn get_ci<'a>(obj: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    let map = obj.as_object()?;
    map.get(key).or_else(|| {
        map.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    })
}

/// 按若干条候选路径依次取值。
///
/// CLI 的 JSON 键名在版本之间挪过位置（`Node.Name` ↔ `node.name`），穷举候选比赌
/// 其中一种写法稳。
fn dig<'a>(root: &'a serde_json::Value, paths: &[&[&str]]) -> Option<&'a serde_json::Value> {
    for path in paths {
        let mut cur = root;
        let mut matched = true;
        for key in *path {
            match get_ci(cur, key) {
                Some(next) => cur = next,
                None => {
                    matched = false;
                    break;
                }
            }
        }
        if matched {
            return Some(cur);
        }
    }
    None
}

fn dig_str(root: &serde_json::Value, paths: &[&[&str]]) -> String {
    dig(root, paths)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

/// 从 `tailscale whois --json` 的输出里取身份字段。
///
/// `None` 表示「JSON 不合法」：解析失败，或者顶层不是 object（数组/标量/null）。
pub fn parse_whois_json(stdout: &str) -> Option<WhoisFields> {
    let root: serde_json::Value = serde_json::from_str(stdout).ok()?;
    if !root.is_object() {
        return None;
    }
    Some(WhoisFields {
        node_name: dig_str(
            &root,
            &[&["Node", "Name"], &["node", "name"], &["NodeName"]],
        ),
        node_stable_id: dig_str(
            &root,
            &[
                &["Node", "StableID"],
                &["node", "stable_id"],
                &["StableID"],
                &["node_stable_id"],
            ],
        ),
        login_name: dig_str(
            &root,
            &[
                &["Node", "User", "LoginName"],
                &["node", "user", "login_name"],
                &["User", "LoginName"],
                &["login_name"],
            ],
        ),
        user_id: dig_str(
            &root,
            &[
                &["Node", "User", "ID"],
                &["node", "user", "id"],
                &["User", "ID"],
                &["user_id"],
                &["ID"],
            ],
        ),
    })
}

/// 判定表：
///
/// | 条件 | 结论 |
/// |---|---|
/// | exit 0 + JSON 合法 | [`SourceKind::Tailnet`] |
/// | exit 1 且 stderr 含 `peer not found` | [`SourceKind::NotInTailnet`] |
/// | 其它非零 / 超时 / JSON 解析失败 / 找不到 bin | [`SourceKind::Unavailable`] |
fn classify(run: Run) -> (SourceKind, Option<WhoisFields>) {
    match run {
        Run::Exited {
            code: Some(0),
            stdout,
            ..
        } => match parse_whois_json(&stdout) {
            Some(fields) => (SourceKind::Tailnet, Some(fields)),
            None => (SourceKind::Unavailable, None),
        },
        Run::Exited {
            code: Some(1),
            stderr,
            ..
        } => {
            if stderr.to_ascii_lowercase().contains("peer not found") {
                (SourceKind::NotInTailnet, None)
            } else {
                (SourceKind::Unavailable, None)
            }
        }
        Run::Exited { .. } => (SourceKind::Unavailable, None),
        Run::TimedOut { .. } => (SourceKind::Unavailable, None),
        Run::Failed(_) => (SourceKind::Unavailable, None),
    }
}

/// 本机账号 ID，供调用方比对 `is_foreign_account`。
///
/// 同样受三级解析阶梯与超时约束。取不到就是 `None`，由调用方按「无法判定」处理——
/// 不能把「查不到」当成「就是本机账号」。
pub async fn self_user_id() -> Option<String> {
    let bin = resolve_bin()?;
    let Run::Exited {
        code: Some(0),
        stdout,
        ..
    } = run_bounded(&bin, &["status", "--json"], WHOIS_TIMEOUT).await
    else {
        return None;
    };
    let root: serde_json::Value = serde_json::from_str(&stdout).ok()?;
    let id = dig_str(
        &root,
        &[
            &["Self", "User", "ID"],
            &["self", "user", "id"],
            &["User", "ID"],
            &["UserID"],
            &["user_id"],
        ],
    );
    if id.is_empty() {
        None
    } else {
        Some(id)
    }
}

/// 对端 `peer` 的身份证据。
///
/// 找不到 bin / 超时 / 判不出来，一律返回 [`SourceKind::Unavailable`]：审批照常继续，
/// 不 panic、不阻塞。
pub async fn whois_evidence(peer: IpAddr) -> (SourceKind, Option<WhoisFields>) {
    let override_env = std::env::var_os(BIN_OVERRIDE_ENV);
    whois_evidence_with(peer, override_env).await
}

/// [`whois_evidence`] 的可注入版本：覆盖值由调用方给。
///
/// 单独拆出来是因为测试用例在同一进程里并发跑，改进程级环境变量会互相串
/// （`std::env::set_var` 在多线程下本身就是不安全的）。
async fn whois_evidence_with(
    peer: IpAddr,
    override_env: Option<OsString>,
) -> (SourceKind, Option<WhoisFields>) {
    let Some(bin) = resolve_bin_with(override_env) else {
        return (SourceKind::Unavailable, None);
    };
    let run = run_bounded(&bin, &["whois", "--json", &peer.to_string()], WHOIS_TIMEOUT).await;
    classify(run)
}

/// 三级解析阶梯，顺序不可调换：
///
/// 1. `CC_SWITCH_TAILSCALE_BIN` 覆盖值；
/// 2. 绝对候选路径；
/// 3. `PATH` 逐目录找 `tailscale`。
///
/// 覆盖值命中就**直接返回、不再往下查**。
///
/// 覆盖值不存在或不可执行时视为「没设置」，继续走阶梯：宁可降级到一个能跑的候选，
/// 也不要因为一个坏的环境变量把身份证据整个废掉。
pub fn tailscale_bin(
    override_env: Option<OsString>,
    candidates: &[PathBuf],
    path_dirs: &[PathBuf],
) -> Option<PathBuf> {
    if let Some(raw) = override_env {
        if !raw.is_empty() {
            let path = PathBuf::from(raw);
            if is_executable(&path) {
                return Some(path);
            }
        }
    }
    for candidate in candidates {
        if is_executable(candidate) {
            return Some(candidate.clone());
        }
    }
    for dir in path_dirs {
        for candidate in bin_candidates_in_dir(dir) {
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// 一个 `PATH` 目录里、为 `tailscale` 应当依次尝试的文件。
///
/// unix 上只有裸名，行为与从前逐字节一致。Windows 上可执行文件必带扩展名、且
/// 扩展名由 `PATHEXT` 决定（装出来是 `tailscale.exe`），所以光 `dir.join("tailscale")`
/// 永远探不到，`resolve_bin` 恒 `None`、`BindScope::Tailscale` 静默降级 —— 这就是
/// 本函数存在的理由。裸名放在最后兜底：万一装出来的是一个无扩展名的可执行文件，
/// 多试一次的成本是一次 `metadata`，省掉它则整条阶梯失效。
fn bin_candidates_in_dir(dir: &Path) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let mut names: Vec<String> = path_extensions(&path_ext_env())
            .into_iter()
            .map(|ext| format!("{BIN_NAME}{ext}"))
            .collect();
        names.push(BIN_NAME.to_string());
        names.into_iter().map(|name| dir.join(name)).collect()
    }
    #[cfg(not(windows))]
    {
        vec![dir.join(BIN_NAME)]
    }
}

/// 读出来的 `PATHEXT` 原值。
#[cfg(windows)]
fn path_ext_env() -> String {
    std::env::var("PATHEXT").unwrap_or_default()
}

/// 把 `PATHEXT`（形如 `.COM;.EXE;.BAT`）解析成带前导点的扩展名列表。
///
/// 分隔符是 `;`，与 Windows 的 `PATH` 分隔符一致 —— 不是 `:`，所以这里不做任何
/// 平台分支。`EXE` 这种没有前导点的写法也接受，补一个点：不同工具写出来的
/// `PATHEXT` 大小写、前导点并不统一。
///
/// **读不到 / 解析不出任何一项时退回 `.exe`**，这是承重的兜底：tailscale 的
/// Windows 安装包装出来的就是 `tailscale.exe`，缺了这一条，缺 `PATHEXT` 的环境
/// （服务、被裁剪的环境、定时任务）上 `BindScope::Tailscale` 会静默降级。
#[cfg(any(windows, test))]
fn path_extensions(raw: &str) -> Vec<String> {
    let parsed: Vec<String> = raw
        .split(';')
        .map(str::trim)
        .filter(|ext| !ext.is_empty())
        .map(|ext| {
            if ext.starts_with('.') {
                ext.to_string()
            } else {
                format!(".{ext}")
            }
        })
        .collect();

    if parsed.is_empty() {
        vec![".exe".to_string()]
    } else {
        parsed
    }
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// 用真实环境的输入跑一遍三级阶梯。
pub fn resolve_bin() -> Option<PathBuf> {
    resolve_bin_with(std::env::var_os(BIN_OVERRIDE_ENV))
}

/// [`resolve_bin`] 的可注入版本。
///
/// ② 必须在 ③ 之前：GUI 启动时 `PATH` 不含 `/usr/local/bin` 是**每次都会发生**的
/// 降级，不是罕见情况。反过来（先查 PATH）的话，`PATH` 里恰好躺着一个同名旧版
/// shim 时，它会赢过真正在跑的那个。
pub fn resolve_bin_with(override_env: Option<OsString>) -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = DEFAULT_CANDIDATES.iter().map(PathBuf::from).collect();
    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    tailscale_bin(override_env, &candidates, &path_dirs)
}

// 只有两条用例真的依赖 POSIX 语义（`sh` 脚本 + `libc::kill` 探活），它们各自
// 单独门控。**不是**给整个模块加 `#[cfg(unix)]`：那会让 Windows 上这 8 条一条
// 都不编译，而新增的 PATHEXT / 候选路径逻辑恰恰只在那条路上有分支 —— 那等于
// 把新代码的断言数量降到 0。「在那个平台上不测」不是让门禁变绿的理由。
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // 只有 unix 那条端到端用例构造 `IpAddr::V4`；Windows 上没有别的用例碰它，
    // 留在模块级会被 `-D warnings` 当成未使用 import。
    #[cfg(unix)]
    use std::net::Ipv4Addr;

    /// 写一个「可执行文件」，内容只是占位 —— 用到它的阶梯用例**从不执行**它，
    /// 只让 [`is_executable`] 认。
    ///
    /// 权限位那一半是 unix 独有的：`set_mode` 来自 `PermissionsExt`，Windows 上
    /// 不存在；而 [`is_executable`] 在非 unix 平台只判 `is_file()`（见它自己的
    /// `cfg(not(unix))` 分支），所以不设权限位照样会被认成可执行 —— 阶梯用例在
    /// Windows 上因此断言的是同一个契约：**阶梯的顺序**，不是权限位。
    fn write_exe(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms).unwrap();
        }
    }

    /// `timeout_kills_subprocess_and_reaps` 用的被测截止时间。
    ///
    /// 慢脚脚本 `sleep 30`，所以 3s 照样把判定推进超时分支（断言的形状没变）；
    /// 取值变大是为了让「子进程有没有被调度到」不再与断言抢同一个时间窗：
    /// 截止时间一到就 `kill()`，而一个尚未被调度的子进程拿到信号后连脚本的第一条
    /// 命令都执行不到，pidfile 永远不会出现 —— 原用例的 500ms 就是在机器满载
    /// （三个并发任务）时这样偶发挂掉的。
    #[cfg(unix)]
    const RUN_DEADLINE: Duration = Duration::from_secs(3);

    /// pidfile 的有限等待预算，与 [`RUN_DEADLINE`] 解耦。
    ///
    /// pidfile 是子进程留在磁盘上的文件、不随进程一起消失，所以这一路不必赶在
    /// kill 之前读：预算耗尽仍读不到，判的是「子进程真的从头到尾没写」这个真问题。
    #[cfg(unix)]
    const PIDFILE_WAIT: Duration = Duration::from_secs(2);

    /// 阶梯③在一个 `PATH` 目录里的候选，末位必须仍是裸名 `dir/tailscale`。
    ///
    /// unix 上候选恰好只有这一个（`.exe`/PATHEXT 那套被 `cfg` 掉，老行为逐字节不变）；
    /// Windows 上它是 `PATHEXT` 各项在前、裸名在后的兜底。这条断言跨平台都成立，
    /// 因此它能同时钉住两件事：新逻辑没有泄漏回 unix，以及 Windows 侧没有为了加
    /// `.exe` 把裸名那条路整个删掉。
    #[test]
    fn path_dir_candidates_always_end_with_the_bare_name() {
        let dir = Path::new("/usr/local/bin");
        let candidates = bin_candidates_in_dir(dir);
        assert_eq!(
            candidates.last(),
            Some(&dir.join(BIN_NAME)),
            "候选列表末位必须是裸名，实际: {candidates:?}"
        );
    }

    /// unix 上候选必须**只有**裸名一个：多拼任何东西都会让阶梯③的探测次数漂移。
    #[cfg(not(windows))]
    #[test]
    fn path_dir_candidates_are_the_bare_name_alone_on_unix() {
        let dir = Path::new("/usr/local/bin");
        assert_eq!(bin_candidates_in_dir(dir), vec![dir.join(BIN_NAME)]);
    }

    /// `PATHEXT` 缺失时的兜底必须是 `.exe`。
    ///
    /// tailscale 的 Windows 安装包装出来的就是 `tailscale.exe`。兜底一旦没了，
    /// 缺 `PATHEXT` 的环境（Windows 服务、被裁剪的环境、计划任务）上
    /// `resolve_bin` 恒 `None`，`BindScope::Tailscale` 静默降级成「没装 tailscale」。
    #[test]
    fn missing_pathext_still_offers_exe() {
        assert_eq!(path_extensions(""), vec![".exe".to_string()]);
        assert_eq!(path_extensions(";;"), vec![".exe".to_string()]);
    }

    /// `PATHEXT` 的分隔符是 `;`，没有前导点的写法也要补一个点。
    ///
    /// 不做大小写归一：Windows 文件系统不区分大小写，`tailscale.EXE` 与
    /// `tailscale.exe` 是同一个文件，而 `PATHEXT` 写成什么大小写由装它的那个
    /// 工具决定，改写它反而会让「保留原样」这一层凭空多出一个失配面。
    #[test]
    fn pathext_entries_are_normalized() {
        assert_eq!(
            path_extensions(".COM;.EXE;BAT; ;"),
            vec![".COM", ".EXE", ".BAT"]
        );
    }

    /// 阶梯①：覆盖变量命中，且**不再查 PATH**。
    ///
    /// PATH 层故意放一个同样存在、同样可执行的 `tailscale`：一旦结果不是覆盖值，
    /// 就说明覆盖没生效或者覆盖之后还去查了 PATH——两者都错。
    #[test]
    fn bin_ladder_prefers_override_without_path_lookup() {
        let tmp = tempfile::tempdir().unwrap();
        let override_bin = tmp.path().join("override-tailscale");
        write_exe(&override_bin, "#!/bin/sh\nexit 0\n");

        let path_dir = tmp.path().join("pathdir");
        write_exe(&path_dir.join(BIN_NAME), "#!/bin/sh\nexit 0\n");

        let got = tailscale_bin(
            Some(override_bin.clone().into_os_string()),
            &[],
            std::slice::from_ref(&path_dir),
        );

        assert_eq!(got, Some(override_bin));
    }

    /// 阶梯②：没有覆盖值时落到绝对候选路径。
    ///
    /// 顺带钉住「覆盖值不可执行不许把整条阶梯废掉」——坏环境变量应当降级，
    /// 应当作数而不是让身份证据整个失效。
    #[test]
    fn bin_ladder_falls_back_to_absolute_candidate() {
        let tmp = tempfile::tempdir().unwrap();
        let candidate = tmp.path().join(BIN_NAME);
        write_exe(&candidate, "#!/bin/sh\nexit 0\n");

        let got = tailscale_bin(None, std::slice::from_ref(&candidate), &[]);
        assert_eq!(got, Some(candidate.clone()));

        let missing = tmp.path().join("nope");
        let got = tailscale_bin(
            Some(missing.into_os_string()),
            std::slice::from_ref(&candidate),
            &[],
        );
        assert_eq!(got, Some(candidate));
    }

    /// 写一个**自报 PID** 的慢脚脚本：`echo $$ > pidfile` 之后 `exec sleep 30`。
    ///
    /// `$$` 是 `Command::spawn` 出来的那个 `sh` 自己的 PID；`exec` 只换映像、保持
    /// PID 不变，所以 pidfile 里记下的恰好是被 kill 的那个**直接子进程**，不会记成
    /// exec 之后的 `sleep`，更不会记到孙进程上。旁路证据由被测子进程自己产生，
    /// 不需要给 `Run` 塞载荷，也不需要改 `run_with_deadline` 的控制流。
    #[cfg(unix)]
    fn write_self_reporting_slow(path: &Path, pidfile: &Path) {
        write_exe(
            path,
            &format!(
                "#!/bin/sh\necho $$ > \"{}\"\nexec sleep 30\n",
                pidfile.display()
            ),
        );
    }

    /// 有界等待 pidfile 落盘并读出内容。
    ///
    /// 为什么必须轮询而不是读一次：`Command::spawn` 返回只说明**内核已经创建了
    /// 这个进程**，不说明它已经被调度到、跑完了 `echo $$ > pidfile`。机器满载时
    /// 这两件事之间隔着一次调度延迟 —— 原用例正是在这里偶发失败（整仓验收命令
    /// 连跑 11 次挂 1 次，报 `No such file or directory`）。
    ///
    /// `budget` 是「子进程真的永远不写」时的**失败时限**，不是无限等待：预算耗尽
    /// 就把最后一次的 `io::Error` 原样交出去，由调用方转成断言失败。绝不静默降级
    /// —— `unwrap_or(0)` 之类会让 ESRCH 断言探到 PID 0 上空转。
    ///
    /// pidfile 是被测子进程留在磁盘上的文件，不随进程一起消失，所以这里**不必**
    /// 与被测的截止时间赛跑：读到的那份 PID 必然属于那个已被 kill 的进程，两者
    /// 之间没有时序耦合。
    #[cfg(unix)]
    async fn wait_for_pidfile(path: &Path, budget: Duration) -> Result<String, std::io::Error> {
        let deadline = Instant::now() + budget;
        loop {
            match std::fs::read_to_string(path) {
                Ok(content) => return Ok(content),
                Err(e) => {
                    if Instant::now() >= deadline {
                        return Err(e);
                    }
                }
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// `kill(pid, 0)` 探活：true = 该 PID 仍在进程表里。
    ///
    /// 探针本身的可靠性也进断言：只有 `ESRCH` 才算「已消失」，`EPERM` 之类的返回
    /// 说明探针给不出可信结论，直接判失败——否则一个恒返回 ESRCH 的坏探针就能让
    /// 调用方把任何进程都判成已死。
    #[cfg(unix)]
    fn pid_alive(pid: i32) -> bool {
        // SAFETY: signal 0 只做存在性/权限探测，不投递任何信号，也不解引用指针。
        let rc = unsafe { libc::kill(pid, 0) };
        if rc == 0 {
            return true;
        }
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH),
            "kill({pid}, 0) 只应以 ESRCH 表示「该 PID 不存在」，\
             其他 errno 说明探针本身不可信，不能据此判定进程已死"
        );
        false
    }

    /// 超时必须真的杀掉子进程：`try_wait()` 要观察到 `Ok(Some(_))`，**且**被杀的
    /// 那个 PID 必须已经从进程表里消失。
    ///
    /// 第一条断言直接拦截「外层 timeout 包 `spawn_blocking(child.wait())`」那种写法：
    /// 那种写法超时后 child 还在跑（本用例的 `sleep 30` 会被留住），`reaped` 为 false，
    /// 进程还会一直泄漏到测试进程退出为止。
    ///
    /// 但 `reaped` 是 `run_with_deadline` 先 `wait()` 收尸、再对同一个 `Child` 读回
    /// 的缓存退出状态，属于自证，证明不了 PID 已从进程表消失。所以另取一路**旁路
    /// 证据**：被测子进程启动瞬间把自身 PID 写进 pidfile，测试侧只做 OS 层探活。
    ///
    /// 那句「启动瞬间」是本用例唯一一处时序假设，也是它偶发失败的唯一来源：截止
    /// 时间一到 `kill()` 就投出去了，而一个**还没被调度到**的子进程拿到信号后连
    /// `echo` 都执行不到，pidfile 于是永远不出现。故截止时间取 [`RUN_DEADLINE`]
    /// （离 `sleep 30` 仍有两个数量级的余量，超时分支照样被走到），pidfile 那一路
    /// 走 [`wait_for_pidfile`] 的有限重试 —— 不再靠「读一次就一定读得到」。
    ///
    /// 门控在 unix，理由是**证据形态**而不是被测行为：它依赖 `#!/bin/sh` 真的跑
    /// 起来、`exec sleep 30` 保住 PID，以及 `kill(pid, 0)` 探进程表 —— Windows 上
    /// 这三样都不成立（没有 POSIX `sh`、没有 `kill(2)` 探针）。「超时后子进程必须
    /// 被杀掉」这个契约在 Windows 上同样成立，但本仓库选用的这套旁路证据无法在那边
    /// 复现。真正跨平台的那一半在 `timeout_reaps_without_leaking_child` 里。
    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_kills_subprocess_and_reaps() {
        // 正对照：同一个探针作用于测试进程自身必须判「活着」。没有这一条，探针恒假
        // 也能让下面的 ESRCH 断言通过，第二类证据就退化成装饰。
        assert!(
            pid_alive(std::process::id() as i32),
            "探针必须能把活着的进程判为存活，否则 ESRCH 断言不可信"
        );

        let tmp = tempfile::tempdir().unwrap();
        let slow = tmp.path().join(BIN_NAME);
        let pidfile = tmp.path().join("child.pid");
        write_self_reporting_slow(&slow, &pidfile);

        let run = run_with_deadline(&slow, &["whois", "--json"], RUN_DEADLINE).await;
        match run {
            Run::TimedOut { reaped } => assert!(
                reaped,
                "超时后必须由同一任务显式 kill() + wait()，且 try_wait() 应为 Ok(Some(_))"
            ),
            other => panic!("慢脚脚本应判超时，实得 {other:?}"),
        }

        // 第二类证据：pidfile 读不出来 / 不是正整数就直接失败，不做静默降级
        // （`unwrap_or(0)` 之类会让断言空转，探到 PID 0 上再"通过"等于什么都没验）。
        // 有限等待用尽仍读不到 = 子进程从头到尾没写下来 = 真问题，照红；
        // 不是无限等待，也不是被 `#[ignore]` 掉。
        let reported = wait_for_pidfile(&pidfile, PIDFILE_WAIT)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "子进程应在启动瞬间把自身 PID 写进 {}，\
                 等待 {:?} 仍未读到就无法核对进程表：{e}",
                    pidfile.display(),
                    PIDFILE_WAIT
                )
            });
        let reported_pid: i32 = reported
            .trim()
            .parse()
            .unwrap_or_else(|e| panic!("pidfile 内容 {:?} 不是合法 PID：{e}", reported));
        assert!(
            reported_pid > 0,
            "pidfile 内容 {reported_pid:?} 不是一个正的 PID，探针会落到非法目标上"
        );

        assert!(
            !pid_alive(reported_pid),
            "超时的子进程 PID {reported_pid} 仍在进程表里：\
             收到了 Unavailable 只说明调用方不再等，不等于子进程已经被杀掉"
        );

        // 端到端：判不出来只能是 Unavailable，不能 panic，也不能一直挂住。
        let (kind, fields) = whois_evidence_with(
            IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1)),
            Some(slow.into_os_string()),
        )
        .await;
        assert_eq!(kind, SourceKind::Unavailable);
        assert!(fields.is_none());
    }

    /// 慢脚进程：**一条一定会待够久的系统自带件**。
    ///
    /// unix 是 `sleep`，Windows 是 `ping -n 60`（`-n` 是次数不是间隔，靠超时参数
    /// 定挂多久）。两者都不需要仓库里有可执行文件，因此
    /// [`run_with_deadline`] 拿到的 argv 在两边都是「可执行文件路径 + 参数」这一种
    /// 形状 —— 换句话说，被测代码路径逐字���相同，只有**怎么把慢脚造出来**分平台。
    #[cfg(unix)]
    fn slow_helper() -> (&'static Path, Vec<&'static str>) {
        (Path::new("sleep"), vec!["30"])
    }

    /// 见 unix 那条 `slow_helper`。`ping` 在 Windows 上是 `ping.exe`，由 PATHEXT 解析，
    /// 这里给绝对目录里的裸名即可 —— [`is_executable`] 在非 unix 平台只判 `is_file`。
    #[cfg(windows)]
    fn slow_helper() -> (&'static Path, Vec<&'static str>) {
        (Path::new("ping"), vec!["-n", "60", "127.0.0.1"])
    }

    /// 超时分支，**两个平台上都跑**的那一半。
    ///
    /// 被测的是 [`run_with_deadline`] 的 kill + wait 路径，而它只用
    /// `std::process::Command`、没有一处 POSIX 依赖 —— 所以「超时后必须收尸」这个
    /// 契约在所有平台上都成立、也都该被断言。只有造慢脚的方式分平台（见
    /// [`slow_helper`]），没有「Windows 上不测」这回事。
    ///
    /// `reaped` 是这里唯一能跨平台断言的收尸证据：`run_with_deadline` 先 `wait()`
    /// 同一个 `Child` 再读回退出状态，属于自证。unix 那条
    /// `timeout_kills_subprocess_and_reaps` 额外用 `kill(pid, 0)` 探进程表补了旁路
    /// 证据 —— 那套证据形态在 Windows 上不成立（没有 `kill(2)`），所以这条只到
    /// `reaped` 为止。这是**证据形态**的平台限制，被测行为本身不是。
    #[tokio::test]
    async fn timeout_reaps_without_leaking_child() {
        let (slow, args) = slow_helper();

        let run = run_with_deadline(slow, &args, Duration::from_secs(3)).await;
        match run {
            Run::TimedOut { reaped } => assert!(
                reaped,
                "超时后必须由同一任务显式 kill() + wait()，且 try_wait() 应为 Ok(Some(_))；\
                 只让外层 future 返回会把子进程留到测试进程退出为止"
            ),
            other => panic!("慢脚进程应判超时，实得 {other:?}（argv: {slow:?} {args:?}）"),
        }
    }

    /// exit 0 + 合法 JSON → Tailnet；exit 1 + `peer not found` → NotInTailnet。
    ///
    /// [`classify`] 拿到的 [`Run`] 已经在 [`run_with_deadline`] 那一层构造好了 ——
    /// 这条用例因此完全不碰文件系统、进程和平台，两边逐字节同构。
    #[test]
    fn classify_covers_the_three_states() {
        let tailnet = classify(Run::Exited {
            code: Some(0),
            stdout: r#"{"Node":{"Name":"n1","StableID":"s1","HostName":"h1","User":{"LoginName":"a@b.c","ID":"u1"}}}"#.to_string(),
            stderr: String::new(),
        });
        assert_eq!(tailnet.0, SourceKind::Tailnet);
        assert_eq!(
            tailnet.1,
            Some(WhoisFields {
                node_name: "n1".into(),
                node_stable_id: "s1".into(),
                login_name: "a@b.c".into(),
                user_id: "u1".into(),
            })
        );

        let absent = classify(Run::Exited {
            code: Some(1),
            stdout: String::new(),
            stderr: "Peer not found".into(),
        });
        assert_eq!(absent.0, SourceKind::NotInTailnet);
        assert!(absent.1.is_none());

        let garbage = classify(Run::Exited {
            code: Some(0),
            stdout: "not json".into(),
            stderr: String::new(),
        });
        assert_eq!(garbage.0, SourceKind::Unavailable);
    }
}
