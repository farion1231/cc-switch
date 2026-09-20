//! 用户自定义外部脚本插件
//!
//! 目录布局：`<配置目录>/plugins/<插件目录名>/plugin.json`。
//! 两种进程模式（清单 `mode` 字段，缺省 oneshot）：
//! - **oneshot**：每次调用拉起新进程，stdin 写入一个 JSON 对象（写完即关闭），
//!   stdout 期望恰好一个 JSON 对象（`{"body": {…}}` 或 `{}`）；无跨调用状态；
//! - **persistent**：首次调用拉起常驻进程，按行交换 JSON（请求一行、响应一行），
//!   JSON 形状与 oneshot 相同；`sse_chunk` stage 仅此模式支持，请求额外携带
//!   `event` / `data`，响应 body 为 `{"data": "…"}`。崩溃/超时自动重启进程
//!   重试一次；插件重载/卸载时进程随之终止。其余输出必须写到 stderr。
//!
//! oneshot 进程调用通过 [`PluginProcessRunner`] trait 注入（真实实现
//! [`TokioProcessRunner`]）；常驻进程通过 [`PersistentTransport`] trait 注入
//! （真实实现 [`PersistentProcessTransport`]，专属 worker 线程 + 自建 runtime）。
//! 单元测试均使用 mock，不 spawn 真实进程。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};

use super::types::{
    ConfigSchemaItem, PluginError, PluginManifest, PluginMode, PluginRequestContext, PluginStage,
};
use super::ProxyPlugin;

/// 清单文件名
pub const MANIFEST_FILE: &str = "plugin.json";

// ---------------------------------------------------------------------------
// 进程调用（可注入）
// ---------------------------------------------------------------------------

/// 外部插件进程调用抽象（真实实现 + 测试 mock）
///
/// `cwd` 为插件进程的工作目录（= 插件所在目录），使脚本参数等相对路径
/// （如 `["node", "index.js"]`）按插件目录解析。
pub trait PluginProcessRunner: Send + Sync {
    fn run(
        &self,
        cmd: &[String],
        cwd: &Path,
        input: &str,
        timeout: Duration,
    ) -> Result<String, PluginError>;
}

/// 基于 `tokio::process::Command` 的真实实现：
/// stdin 写入后关闭、收集 stdout、超时 kill、退出码检查
pub struct TokioProcessRunner;

/// 进程级错误可读标识（runner 不知道真实插件 id，用 argv[0] 兜底）
fn runner_plugin_id(cmd: &[String]) -> String {
    cmd.first().cloned().unwrap_or_default()
}

impl TokioProcessRunner {
    async fn run_async(
        &self,
        cmd: &[String],
        cwd: &Path,
        input: &str,
        timeout: Duration,
    ) -> Result<String, PluginError> {
        use std::process::Stdio;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        if cmd.is_empty() {
            return Err(PluginError::InvalidManifest("command 不能为空".to_string()));
        }

        // kill_on_drop：即使本 future 被取消（调用方超时放弃），也尽量回收子进程
        let mut child = tokio::process::Command::new(&cmd[0])
            .args(&cmd[1..])
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        let mut stdin = child.stdin.take().ok_or_else(|| PluginError::Execution {
            plugin_id: runner_plugin_id(cmd),
            message: "无法打开子进程 stdin".to_string(),
        })?;
        let mut stdout = child.stdout.take().ok_or_else(|| PluginError::Execution {
            plugin_id: runner_plugin_id(cmd),
            message: "无法打开子进程 stdout".to_string(),
        })?;
        let mut stderr = child.stderr.take().ok_or_else(|| PluginError::Execution {
            plugin_id: runner_plugin_id(cmd),
            message: "无法打开子进程 stderr".to_string(),
        })?;

        // stdin 写入（可能因子进程不读而阻塞）、stdout/stderr 收集都纳入超时范围。
        // stderr 必须并发读取：无人读取时子进程写满管道缓冲区会阻塞，造成假超时。
        let io_work = async {
            let write_stdin = async {
                stdin.write_all(input.as_bytes()).await?;
                stdin.shutdown().await?;
                // 显式 drop 关闭 stdin（协议要求写完即关闭）
                drop(stdin);
                Ok::<(), std::io::Error>(())
            };
            let read_stdout = async {
                let mut buf = Vec::new();
                stdout.read_to_end(&mut buf).await?;
                Ok::<Vec<u8>, std::io::Error>(buf)
            };
            let read_stderr = async {
                let mut buf = Vec::new();
                stderr.read_to_end(&mut buf).await?;
                Ok::<Vec<u8>, std::io::Error>(buf)
            };
            let (r, so, se) = tokio::join!(write_stdin, read_stdout, read_stderr);
            r?;
            let stdout_buf = so?;
            let stderr_buf = se?;
            Ok::<(Vec<u8>, Vec<u8>), std::io::Error>((stdout_buf, stderr_buf))
        };

        let (stdout_data, stderr_data) = match tokio::time::timeout(timeout, io_work).await {
            Ok(result) => result.map_err(PluginError::Io)?,
            Err(_) => {
                // 超时：kill 直接子进程并回收，避免僵尸进程
                // （kill_on_drop 已兜底，这里显式 kill 便于立刻回收）
                let _ = child.start_kill();
                let _ = child.wait().await;
                return Err(PluginError::Timeout {
                    plugin_id: runner_plugin_id(cmd),
                    timeout_ms: timeout.as_millis() as u64,
                });
            }
        };

        let status = child.wait().await?;
        if !status.success() {
            // 失败时附带 stderr 摘要，便于用户排查插件脚本错误
            let stderr_excerpt = String::from_utf8_lossy(&stderr_data);
            let stderr_excerpt = stderr_excerpt.trim();
            let stderr_note = if stderr_excerpt.is_empty() {
                String::new()
            } else {
                let max = 500;
                let excerpt = if stderr_excerpt.len() > max {
                    format!("{}…", &stderr_excerpt[..max])
                } else {
                    stderr_excerpt.to_string()
                };
                format!("，stderr: {excerpt}")
            };
            return Err(PluginError::Execution {
                plugin_id: runner_plugin_id(cmd),
                message: format!("退出码非 0: {status}{stderr_note}"),
            });
        }

        Ok(String::from_utf8_lossy(&stdout_data).into_owned())
    }
}

impl PluginProcessRunner for TokioProcessRunner {
    fn run(
        &self,
        cmd: &[String],
        cwd: &Path,
        input: &str,
        timeout: Duration,
    ) -> Result<String, PluginError> {
        // 管线在同步上下文中执行；按所处运行时形态选择阻塞策略：
        // - 多线程运行时：block_in_place 借出当前 worker 线程
        // - current-thread 运行时（测试等场景）：不能在驱动线程上 block_on，
        //   改到专用线程 + 独立运行时执行
        // - 不在运行时内：一次性临时运行时执行
        match tokio::runtime::Handle::try_current() {
            Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
                let fut = self.run_async(cmd, cwd, input, timeout);
                tokio::task::block_in_place(|| handle.block_on(fut))
            }
            Ok(_) => {
                // 跨线程执行前先克隆为持有数据（借用无法跨 'static）
                let cmd = cmd.to_vec();
                let cwd = cwd.to_path_buf();
                let input = input.to_string();
                let fallback_id = runner_plugin_id(&cmd);
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let result = match tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    {
                        Ok(rt) => {
                            rt.block_on(TokioProcessRunner.run_async(&cmd, &cwd, &input, timeout))
                        }
                        Err(e) => Err(PluginError::Io(e)),
                    };
                    let _ = tx.send(result);
                });
                rx.recv().map_err(|_| PluginError::Execution {
                    plugin_id: fallback_id,
                    message: "插件进程执行线程异常终止".to_string(),
                })?
            }
            Err(_) => {
                let fut = self.run_async(cmd, cwd, input, timeout);
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                rt.block_on(fut)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 常驻插件进程（mode: "persistent"）
// ---------------------------------------------------------------------------

/// 常驻插件传输抽象（真实实现 [`PersistentProcessTransport`]；测试注入 mock）
///
/// 协议：每次调用发送一行请求 JSON（追加 `\n`），插件回复一行 JSON（不带换行）。
/// 请求/响应的 JSON 形状与 oneshot 模式完全一致（`{"body": …}` / `{}`）；
/// `sse_chunk` stage 的请求额外携带 `event` / `data` 字段，响应的 body 为
/// `{"data": "…"}`。插件进程可维护跨调用状态（如流式 per-stream 缓冲）。
/// 核心侧调用天然串行（worker 循环逐条处理），崩溃/超时自动重启进程重试一次。
/// [`PersistentTransport::shutdown`] 用于运行时禁用插件时终止进程；
/// 重新启用后下次 call 应能重新拉起（实现须支持复活）。
pub trait PersistentTransport: Send + Sync {
    fn call(&self, input: &str, timeout: Duration) -> Result<String, PluginError>;

    /// 终止常驻会话（worker/子进程）。之后再次 [`PersistentTransport::call`]
    /// 必须能重新拉起并正常工作（插件被禁用后又启用的场景）。
    fn shutdown(&self) {}
}

/// worker 线程任务
enum PersistentWorkerMsg {
    Call {
        input: String,
        timeout: Duration,
        reply: tokio::sync::oneshot::Sender<Result<String, PluginError>>,
    },
    /// Drop 时投递：worker 退出 → runtime 销毁 → 会话 Drop → kill_on_drop 杀进程
    Shutdown,
}

/// 一次活会话：子进程 + stdin/stdout 句柄（Drop 即杀进程，kill_on_drop 回收）
struct PersistentProcess {
    /// 持有 Child 仅为会话存活期间保住 kill_on_drop 语义（Drop 时杀进程回收），
    /// 日常调用只走 stdin/stdout
    #[allow(dead_code)]
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: tokio::io::BufReader<tokio::process::ChildStdout>,
}

/// 真实常驻传输。
///
/// 进程的异步 IO 绑定在创建它的 runtime 上，而常驻进程的生命周期跨越多次调用
/// （调用可能来自不同 runtime 形态的线程），因此所有进程 IO 固定在一个专属
/// worker 线程的 current-thread runtime 上执行：调用方投递任务并等待回执，
/// worker 循环逐条处理（天然串行，插件无需处理并发）。
pub struct PersistentProcessTransport {
    plugin_id: String,
    command: Vec<String>,
    cwd: PathBuf,
    /// worker 发送端；shutdown 后置 None，下次 call 重新拉起 worker（可复活）。
    /// Mutex 保护：shutdown 与 call 可能来自不同线程。
    worker: std::sync::Mutex<Option<tokio::sync::mpsc::Sender<PersistentWorkerMsg>>>,
}

impl PersistentProcessTransport {
    /// 构造（worker 线程懒拉起：首次调用才创建；进程本体更晚：worker 首个调用才 spawn）
    pub fn new(plugin_id: String, command: Vec<String>, cwd: PathBuf) -> Self {
        Self {
            plugin_id,
            command,
            cwd,
            worker: std::sync::Mutex::new(None),
        }
    }

    fn spawn_worker(&self) -> tokio::sync::mpsc::Sender<PersistentWorkerMsg> {
        let (tx, rx) = tokio::sync::mpsc::channel::<PersistentWorkerMsg>(16);
        let worker_id = self.plugin_id.clone();
        let command = self.command.clone();
        let cwd = self.cwd.clone();
        std::thread::Builder::new()
            .name(format!("plugin-persistent-{}", self.plugin_id))
            .spawn(move || persistent_worker_main(worker_id, command, cwd, rx))
            .expect("启动常驻插件 worker 线程失败");
        tx
    }
}

impl Drop for PersistentProcessTransport {
    fn drop(&mut self) {
        PersistentTransport::shutdown(self);
    }
}

impl PersistentTransport for PersistentProcessTransport {
    fn call(&self, input: &str, timeout: Duration) -> Result<String, PluginError> {
        let tx = {
            let mut guard = self.worker.lock().unwrap();
            guard.get_or_insert_with(|| self.spawn_worker()).clone()
        };
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        tx.try_send(PersistentWorkerMsg::Call {
            input: input.to_string(),
            timeout,
            reply: reply_tx,
        })
        .map_err(|e| PluginError::Execution {
            plugin_id: self.plugin_id.clone(),
            message: format!("常驻插件任务投递失败（worker 已退出？）: {e}"),
        })?;
        // 回执等待按所处运行时形态分派（oneshot channel 跨 runtime 可用）：
        // - 多线程运行时：block_in_place 等待
        // - current-thread 运行时（测试）：不能阻塞驱动线程 → 专用线程等待
        // - 不在运行时内：直接 blocking_recv
        match tokio::runtime::Handle::try_current() {
            Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
                tokio::task::block_in_place(|| handle.block_on(reply_rx)).map_err(|_| {
                    PluginError::Execution {
                        plugin_id: self.plugin_id.clone(),
                        message: "常驻插件 worker 异常终止".to_string(),
                    }
                })?
            }
            Ok(_) => {
                let (tx2, rx2) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let result = tokio::runtime::Builder::new_current_thread()
                        .build()
                        .ok()
                        .and_then(|rt| rt.block_on(reply_rx).ok())
                        .unwrap_or_else(|| {
                            Err(PluginError::Execution {
                                plugin_id: "persistent".to_string(),
                                message: "等待常驻插件回执线程异常终止".to_string(),
                            })
                        });
                    let _ = tx2.send(result);
                });
                rx2.recv().map_err(|_| PluginError::Execution {
                    plugin_id: self.plugin_id.clone(),
                    message: "等待常驻插件回执线程异常终止".to_string(),
                })?
            }
            Err(_) => reply_rx
                .blocking_recv()
                .map_err(|_| PluginError::Execution {
                    plugin_id: self.plugin_id.clone(),
                    message: "常驻插件 worker 异常终止".to_string(),
                })?,
        }
    }

    fn shutdown(&self) {
        if let Some(tx) = self.worker.lock().unwrap().take() {
            let _ = tx.try_send(PersistentWorkerMsg::Shutdown);
            log::info!(
                "[PLUGIN] 常驻插件 {} 会话已终止（禁用/卸载）；重新启用后下次调用自动重启",
                self.plugin_id
            );
        }
    }
}

/// worker 线程主体：自建 current-thread runtime，循环处理调用直至 Shutdown。
/// 会话持有在本 runtime 内，保证子进程 IO 的 runtime 一致性。
fn persistent_worker_main(
    plugin_id: String,
    command: Vec<String>,
    cwd: PathBuf,
    mut rx: tokio::sync::mpsc::Receiver<PersistentWorkerMsg>,
) {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        log::warn!("[PLUGIN] 常驻插件 {plugin_id} worker runtime 创建失败，插件不可用");
        return;
    };
    rt.block_on(async move {
        let mut session: Option<PersistentProcess> = None;
        while let Some(msg) = rx.recv().await {
            match msg {
                PersistentWorkerMsg::Shutdown => break,
                PersistentWorkerMsg::Call {
                    input,
                    timeout,
                    reply,
                } => {
                    let result =
                        persistent_call(&mut session, &plugin_id, &command, &cwd, &input, timeout)
                            .await;
                    let _ = reply.send(result);
                }
            }
        }
        // session 在此 Drop → kill_on_drop 杀死子进程
    });
}

/// 单次调用（带一次崩溃/超时重启重试）：进程不在或刚被杀掉时重新拉起
async fn persistent_call(
    session: &mut Option<PersistentProcess>,
    plugin_id: &str,
    command: &[String],
    cwd: &Path,
    input: &str,
    timeout: Duration,
) -> Result<String, PluginError> {
    for attempt in 0..2usize {
        if session.is_none() {
            *session = Some(persistent_spawn(plugin_id, command, cwd).await?);
            log::debug!(
                "[PLUGIN] 常驻插件 {plugin_id} 进程已启动（第 {} 次拉起）",
                attempt + 1
            );
        }
        let live = session.as_mut().expect("会话已保证存在");
        let exchange = persistent_exchange(plugin_id, live, input);
        match tokio::time::timeout(timeout, exchange).await {
            Ok(Ok(line)) => return Ok(line),
            Ok(Err(e)) => {
                log::warn!(
                    "[PLUGIN] 常驻插件 {plugin_id} 调用失败（第 {} 次）: {e}",
                    attempt + 1
                );
            }
            Err(_) => {
                log::warn!(
                    "[PLUGIN] 常驻插件 {plugin_id} 调用超时（{}ms，第 {} 次），重启进程",
                    timeout.as_millis(),
                    attempt + 1
                );
            }
        }
        // 旧会话 Drop（杀进程）；进程状态在失败后不可信，必须重启
        *session = None;
    }
    Err(PluginError::Execution {
        plugin_id: plugin_id.to_string(),
        message: "常驻插件重启进程重试后仍失败".to_string(),
    })
}

/// 拉起常驻子进程；stderr 由后台任务持续排空（无人读取时写满管道会阻塞进程）
async fn persistent_spawn(
    plugin_id: &str,
    command: &[String],
    cwd: &Path,
) -> Result<PersistentProcess, PluginError> {
    use std::process::Stdio;

    if command.is_empty() {
        return Err(PluginError::InvalidManifest("command 不能为空".to_string()));
    }
    let mut child = tokio::process::Command::new(&command[0])
        .args(&command[1..])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| PluginError::Execution {
            plugin_id: plugin_id.to_string(),
            message: format!("启动常驻插件进程失败: {e}"),
        })?;
    let stdin = child.stdin.take().ok_or_else(|| PluginError::Execution {
        plugin_id: plugin_id.to_string(),
        message: "无法打开常驻插件 stdin".to_string(),
    })?;
    let stdout = child.stdout.take().ok_or_else(|| PluginError::Execution {
        plugin_id: plugin_id.to_string(),
        message: "无法打开常驻插件 stdout".to_string(),
    })?;
    if let Some(mut stderr) = child.stderr.take() {
        let plugin_id = plugin_id.to_string();
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut buf = vec![0u8; 4096];
            loop {
                match stderr.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&buf[..n]);
                        log::debug!("[PLUGIN] 常驻插件 {plugin_id} stderr: {}", text.trim_end());
                    }
                }
            }
            log::debug!("[PLUGIN] 常驻插件 {plugin_id} stderr 流结束");
        });
    }
    Ok(PersistentProcess {
        child,
        stdin,
        stdout: tokio::io::BufReader::new(stdout),
    })
}

/// 写一行请求、读一行响应（超时由调用方包裹；EOF 视为进程退出）
async fn persistent_exchange(
    plugin_id: &str,
    session: &mut PersistentProcess,
    input: &str,
) -> Result<String, PluginError> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    session.stdin.write_all(input.as_bytes()).await?;
    session.stdin.write_all(b"\n").await?;
    session.stdin.flush().await?;
    let mut line = String::new();
    let n = session.stdout.read_line(&mut line).await?;
    if n == 0 {
        return Err(PluginError::Execution {
            plugin_id: plugin_id.to_string(),
            message: "常驻插件 stdout 已关闭（进程退出？）".to_string(),
        });
    }
    Ok(line.trim_end_matches(['\n', '\r']).to_string())
}

// ---------------------------------------------------------------------------
// ExternalPlugin
// ---------------------------------------------------------------------------

/// 用户自定义外部脚本插件（实现 [`ProxyPlugin`]，内部持 runner + manifest）
pub struct ExternalPlugin {
    /// 注册后的 id：`user:<manifest.id>`
    id: String,
    /// manifest（含 settings 等调用期透传字段）
    manifest: PluginManifest,
    /// argv[0] 相对路径已按插件目录解析后的完整命令
    command: Vec<String>,
    /// 插件目录（子进程工作目录：脚本参数等相对路径按插件目录解析）
    plugin_dir: PathBuf,
    /// manifest 路径（PluginInfo.source 展示用）
    source: Option<String>,
    runner: Arc<dyn PluginProcessRunner>,
    /// 常驻传输（`mode: "persistent"` 时持有；oneshot 为 None）
    persistent: Option<Arc<dyn PersistentTransport>>,
}

impl ExternalPlugin {
    /// 构造外部插件（校验清单；argv[0] 相对路径相对于插件目录解析）
    pub fn new(
        manifest: PluginManifest,
        plugin_dir: &Path,
        runner: Arc<dyn PluginProcessRunner>,
    ) -> Result<Self, PluginError> {
        manifest.validate()?;
        let source = Some(plugin_dir.join(MANIFEST_FILE).display().to_string());
        let command = resolve_command(&manifest.command, plugin_dir);
        let persistent = match manifest.mode {
            PluginMode::Persistent => Some(Arc::new(PersistentProcessTransport::new(
                format!("user:{}", manifest.id),
                command.clone(),
                plugin_dir.to_path_buf(),
            )) as Arc<dyn PersistentTransport>),
            PluginMode::Oneshot => None,
        };
        Ok(Self {
            id: format!("user:{}", manifest.id),
            command,
            plugin_dir: plugin_dir.to_path_buf(),
            manifest,
            source,
            runner,
            persistent,
        })
    }

    /// 测试构造器：注入 mock 常驻传输
    #[cfg(test)]
    fn with_persistent_transport(
        manifest: PluginManifest,
        plugin_dir: &Path,
        runner: Arc<dyn PluginProcessRunner>,
        transport: Arc<dyn PersistentTransport>,
    ) -> Result<Self, PluginError> {
        manifest.validate()?;
        let source = Some(plugin_dir.join(MANIFEST_FILE).display().to_string());
        Ok(Self {
            id: format!("user:{}", manifest.id),
            command: resolve_command(&manifest.command, plugin_dir),
            plugin_dir: plugin_dir.to_path_buf(),
            manifest,
            source,
            runner,
            persistent: Some(transport),
        })
    }

    /// manifest 的 stages 是运行时数据，而 trait 返回 &'static；
    /// 注册/重载频率极低且条目极小，这里有意将其提升为 'static（有意泄漏）
    fn leak_stages(stages: &[PluginStage]) -> &'static [PluginStage] {
        Box::leak(stages.to_vec().into_boxed_slice())
    }

    /// 统一传输入口：oneshot 走 runner（每次拉起新进程），
    /// persistent 走常驻会话（按行交换）
    fn dispatch(&self, input_str: &str) -> Result<String, PluginError> {
        let timeout = Duration::from_millis(self.manifest.timeout_ms);
        match &self.persistent {
            Some(transport) => transport.call(input_str, timeout),
            None => self
                .runner
                .run(&self.command, &self.plugin_dir, input_str, timeout)
                .map_err(|e| PluginError::Execution {
                    plugin_id: self.id.clone(),
                    message: e.to_string(),
                }),
        }
    }

    /// 统一的变换入口：组装协议输入 → 调进程 → 解析协议输出
    fn run_transform(
        &self,
        ctx: &PluginRequestContext,
        body: &mut Value,
    ) -> Result<bool, PluginError> {
        // 防御：只在声明的 stage 上执行
        if !self.manifest.stages.contains(&ctx.stage) {
            return Ok(false);
        }

        let input = json!({
            "stage": ctx.stage,
            "app_type": ctx.app_type,
            "session_id": ctx.session_id,
            "request_model": ctx.request_model,
            "provider": ctx.provider,
            "settings": self.manifest.settings,
            "body": body,
        });
        let input_str = serde_json::to_string(&input).map_err(|e| PluginError::Execution {
            plugin_id: self.id.clone(),
            message: format!("序列化协议输入失败: {e}"),
        })?;

        let output = self.dispatch(&input_str)?;
        parse_plugin_output(&self.id, &output, body)
    }

    /// SSE 事件变换（仅 persistent 插件可达：manifest 校验保证 sse_chunk
    /// 只出现在 persistent 模式）。协议：请求额外携带 `event` / `data`，
    /// 响应 `{"body": {"data": "…"}}` 替换 data，`{}` 表示无修改。
    fn run_sse_transform(
        &self,
        ctx: &PluginRequestContext,
        event_name: Option<&str>,
        data: &mut String,
    ) -> Result<bool, PluginError> {
        if !self.manifest.stages.contains(&ctx.stage) {
            return Ok(false);
        }

        let input = json!({
            "stage": ctx.stage,
            "app_type": ctx.app_type,
            "session_id": ctx.session_id,
            "request_model": ctx.request_model,
            "provider": ctx.provider,
            "settings": self.manifest.settings,
            "event": event_name,
            "data": data,
        });
        let input_str = serde_json::to_string(&input).map_err(|e| PluginError::Execution {
            plugin_id: self.id.clone(),
            message: format!("序列化协议输入失败: {e}"),
        })?;

        let output = self.dispatch(&input_str)?;
        let trimmed = output.trim();
        if trimmed.is_empty() {
            return Err(PluginError::Execution {
                plugin_id: self.id.clone(),
                message: "stdout 输出为空".to_string(),
            });
        }
        let parsed: Value = serde_json::from_str(trimmed)?;
        match parsed {
            Value::Object(map) => {
                if map.is_empty() {
                    return Ok(false);
                }
                let new_data = map
                    .get("body")
                    .and_then(|body| body.get("data"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| PluginError::Execution {
                        plugin_id: self.id.clone(),
                        message: "sse_chunk 响应缺少 body.data 字符串".to_string(),
                    })?;
                *data = new_data.to_string();
                Ok(true)
            }
            _ => Err(PluginError::Execution {
                plugin_id: self.id.clone(),
                message: "stdout 不是 JSON 对象".to_string(),
            }),
        }
    }
}

impl ProxyPlugin for ExternalPlugin {
    fn id(&self) -> &str {
        &self.id
    }

    fn display_name(&self) -> &str {
        &self.manifest.name
    }

    fn description(&self) -> &str {
        self.manifest.description.as_deref().unwrap_or("")
    }

    fn is_builtin(&self) -> bool {
        false
    }

    fn stages(&self) -> &'static [PluginStage] {
        Self::leak_stages(&self.manifest.stages)
    }

    fn default_priority(&self) -> i32 {
        self.manifest.priority
    }

    fn default_enabled(&self) -> bool {
        self.manifest.enabled
    }

    fn version(&self) -> Option<String> {
        self.manifest.version.clone()
    }

    fn source(&self) -> Option<String> {
        self.source.clone()
    }

    fn transform_request(
        &self,
        ctx: &PluginRequestContext,
        body: &mut Value,
    ) -> Result<bool, PluginError> {
        self.run_transform(ctx, body)
    }

    fn transform_response(
        &self,
        ctx: &PluginRequestContext,
        body: &mut Value,
    ) -> Result<bool, PluginError> {
        self.run_transform(ctx, body)
    }

    fn transform_sse_event(
        &self,
        ctx: &PluginRequestContext,
        event_name: Option<&str>,
        data: &mut String,
        _state: &mut dyn std::any::Any,
    ) -> Result<bool, PluginError> {
        self.run_sse_transform(ctx, event_name, data)
    }

    fn on_disabled(&self) {
        // 运行时禁用：终止常驻子进程（否则第三方进程会存活到重载/退出）；
        // 重新启用后下次 call 会重新拉起 worker 与进程
        if let Some(transport) = &self.persistent {
            transport.shutdown();
        }
    }

    fn config_schema(&self) -> &[ConfigSchemaItem] {
        &self.manifest.config_schema
    }

    fn config_read(&self) -> Result<Value, PluginError> {
        let input = json!({"stage": "config", "op": "get"});
        let output = self.dispatch(&input.to_string())?;
        let parsed: Value = serde_json::from_str(output.trim())?;
        match parsed {
            Value::Object(map) => match map.get("config") {
                Some(config) => Ok(config.clone()),
                // 空 = 插件未返回任何配置文档
                None => Ok(json!({})),
            },
            _ => Err(PluginError::Execution {
                plugin_id: self.id.clone(),
                message: "config 响应不是 JSON 对象".to_string(),
            }),
        }
    }

    fn config_write(&self, docs: &Value) -> Result<(), PluginError> {
        let input = json!({"stage": "config", "op": "set", "config": docs});
        let output = self.dispatch(&input.to_string())?;
        let parsed: Value = serde_json::from_str(output.trim())?;
        match parsed {
            // 空对象 = 保存成功
            Value::Object(ref map) if map.is_empty() => Ok(()),
            Value::Object(map) => Err(PluginError::Execution {
                plugin_id: self.id.clone(),
                message: map
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("config 保存被插件拒绝（未返回原因）")
                    .to_string(),
            }),
            _ => Err(PluginError::Execution {
                plugin_id: self.id.clone(),
                message: "config 响应不是 JSON 对象".to_string(),
            }),
        }
    }
}

/// 解析命令：argv[0] 含路径分隔符且为相对路径时，相对于插件目录解析；
/// 纯命令名（如 `node`）保持原样交给 PATH 解析
fn resolve_command(argv: &[String], plugin_dir: &Path) -> Vec<String> {
    let mut argv = argv.to_vec();
    if let Some(first) = argv.first_mut() {
        let has_separator = first.contains('/') || first.contains('\\');
        if has_separator && Path::new(first.as_str()).is_relative() {
            *first = plugin_dir.join(first.as_str()).display().to_string();
        }
    }
    argv
}

/// 解析协议输出：`{"body": …}` → 应用并返回 true；`{}` → 无修改；
/// 其余情况（非法 JSON / 缺 body 字段 / 非 JSON 对象）按插件错误处理（fail-open）
pub(crate) fn parse_plugin_output(
    plugin_id: &str,
    output: &str,
    body: &mut Value,
) -> Result<bool, PluginError> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Err(PluginError::Execution {
            plugin_id: plugin_id.to_string(),
            message: "stdout 输出为空".to_string(),
        });
    }

    let parsed: Value = serde_json::from_str(trimmed)?;
    match parsed {
        Value::Object(map) => {
            if let Some(new_body) = map.get("body") {
                *body = new_body.clone();
                Ok(true)
            } else if map.is_empty() {
                Ok(false)
            } else {
                Err(PluginError::Execution {
                    plugin_id: plugin_id.to_string(),
                    message: "stdout 输出缺少 body 字段".to_string(),
                })
            }
        }
        _ => Err(PluginError::Execution {
            plugin_id: plugin_id.to_string(),
            message: "stdout 不是 JSON 对象".to_string(),
        }),
    }
}

// ---------------------------------------------------------------------------
// 加载器
// ---------------------------------------------------------------------------

/// 用户插件加载失败条目（不 panic，收集后供 list 展示）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadError {
    /// 插件目录名（id 可能尚未可知）
    pub dir_name: String,
    /// manifest 路径（可缺失）
    pub manifest_path: Option<String>,
    pub message: String,
}

/// 遍历插件目录下的子目录，读取 `plugin.json` 并构造外部插件。
/// 目录不存在则静默返回空；失败条目收集为 [`LoadError`]，不 panic。
pub fn load_user_plugins(dir: &Path) -> (Vec<Arc<dyn ProxyPlugin>>, Vec<LoadError>) {
    let mut plugins: Vec<Arc<dyn ProxyPlugin>> = Vec::new();
    let mut errors: Vec<LoadError> = Vec::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // 目录不存在/不可读：静默跳过（契约 2.7）
        Err(_) => return (plugins, errors),
    };

    let runner: Arc<dyn PluginProcessRunner> = Arc::new(TokioProcessRunner);

    let mut plugin_dirs: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    plugin_dirs.sort();

    for plugin_dir in plugin_dirs {
        let dir_name = plugin_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let manifest_path = plugin_dir.join(MANIFEST_FILE);

        match std::fs::read_to_string(&manifest_path) {
            Err(e) => errors.push(LoadError {
                dir_name,
                manifest_path: Some(manifest_path.display().to_string()),
                message: format!("读取 plugin.json 失败: {e}"),
            }),
            Ok(content) => match serde_json::from_str::<PluginManifest>(&content) {
                Err(e) => errors.push(LoadError {
                    dir_name,
                    manifest_path: Some(manifest_path.display().to_string()),
                    message: format!("解析 plugin.json 失败: {e}"),
                }),
                Ok(manifest) => match ExternalPlugin::new(manifest, &plugin_dir, runner.clone()) {
                    Err(e) => errors.push(LoadError {
                        dir_name,
                        manifest_path: Some(manifest_path.display().to_string()),
                        message: e.to_string(),
                    }),
                    Ok(plugin) => plugins.push(Arc::new(plugin)),
                },
            },
        }
    }

    (plugins, errors)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Mutex;

    use serde_json::json;

    use super::super::types::PluginProviderInfo;
    use super::*;

    /// 随仓参考插件（plugins-available/*，若存在）的清单必须始终通过校验，
    /// 防止清单格式演进时参考实现悄悄失配；目录不存在时整项跳过
    #[test]
    fn test_shipped_reference_plugin_manifests_are_valid() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugins-available");
        let Ok(entries) = fs::read_dir(&dir) else {
            return; // 目录不存在（打包裁剪等场景）时跳过
        };
        for entry in entries.flatten() {
            let manifest_path = entry.path().join(MANIFEST_FILE);
            if !manifest_path.is_file() {
                continue;
            }
            let content = fs::read_to_string(&manifest_path).expect("read manifest");
            let manifest: PluginManifest = serde_json::from_str(&content).expect("parse manifest");
            manifest
                .validate()
                .unwrap_or_else(|e| panic!("{} 校验失败: {e}", manifest_path.display()));
        }
    }

    // -----------------------------------------------------------------------
    // Mock runner：记录调用参数，返回预设输出（测试不 spawn 真实进程）
    // -----------------------------------------------------------------------

    /// Mock 的预设行为（PluginError 不可 Clone，用枚举在调用时构造结果）
    #[derive(Clone)]
    enum MockOutput {
        Text(String),
        Fail(String),
    }

    /// 一次调用的记录：(argv, 工作目录, 协议输入, 超时毫秒)
    type RecordedCall = (Vec<String>, PathBuf, String, u128);

    struct MockRunner {
        output: MockOutput,
        calls: Mutex<Vec<RecordedCall>>,
    }

    impl MockRunner {
        fn ok(output: &str) -> Self {
            Self {
                output: MockOutput::Text(output.to_string()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn err(message: &str) -> Self {
            Self {
                output: MockOutput::Fail(message.to_string()),
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl PluginProcessRunner for MockRunner {
        fn run(
            &self,
            cmd: &[String],
            cwd: &Path,
            input: &str,
            timeout: Duration,
        ) -> Result<String, PluginError> {
            self.calls.lock().unwrap().push((
                cmd.to_vec(),
                cwd.to_path_buf(),
                input.to_string(),
                timeout.as_millis(),
            ));
            match &self.output {
                MockOutput::Text(text) => Ok(text.clone()),
                MockOutput::Fail(message) => Err(PluginError::Execution {
                    plugin_id: "mock".to_string(),
                    message: message.clone(),
                }),
            }
        }
    }

    fn manifest_json(id: &str) -> Value {
        json!({
            "id": id,
            "name": "测试插件",
            "stages": ["pre_request"],
            "command": ["node", "index.js"],
            "timeout_ms": 5000,
            "enabled": true
        })
    }

    fn base_manifest() -> PluginManifest {
        serde_json::from_value(manifest_json("my-plugin")).unwrap()
    }

    fn ctx(stage: PluginStage) -> PluginRequestContext {
        PluginRequestContext {
            app_type: "claude".to_string(),
            session_id: "sess-1".to_string(),
            request_model: "claude-x".to_string(),
            stage,
            provider: Some(PluginProviderInfo {
                id: "p1".to_string(),
                name: "Provider 1".to_string(),
                is_bedrock: false,
            }),
        }
    }

    // -----------------------------------------------------------------------
    // 清单解析 / 校验
    // -----------------------------------------------------------------------

    #[test]
    fn test_manifest_parse_defaults_and_unknown_fields() {
        let manifest: PluginManifest = serde_json::from_value(json!({
            "id": "my-plugin",
            "name": "我的插件",
            "stages": ["pre_send"],
            "command": ["python", "run.py"],
            "future_field": 123
        }))
        .unwrap();

        assert_eq!(manifest.priority, 500);
        assert_eq!(manifest.timeout_ms, 10_000);
        assert!(manifest.enabled);
        manifest.validate().unwrap();
    }

    #[test]
    fn test_manifest_missing_required_field_fails() {
        let value = manifest_json("my-plugin");

        let mut broken = value.clone();
        broken["stages"] = json!(null);
        assert!(serde_json::from_value::<PluginManifest>(broken).is_err());

        let mut broken = value;
        broken["command"] = json!(null);
        assert!(serde_json::from_value::<PluginManifest>(broken).is_err());
    }

    #[test]
    fn test_manifest_validate_rejects_sse_chunk_in_oneshot_mode() {
        // oneshot（缺省）模式不允许 sse_chunk（每事件拉进程不可行）
        let mut manifest = base_manifest();
        manifest.stages = vec![PluginStage::SseChunk];
        assert!(manifest.validate().is_err());

        // persistent 模式下 sse_chunk 合法
        let mut manifest = base_manifest();
        manifest.mode = PluginMode::Persistent;
        manifest.stages = vec![PluginStage::SseChunk];
        manifest.validate().unwrap();
    }

    #[test]
    fn test_manifest_mode_parsing() {
        // 缺省 oneshot
        let manifest: PluginManifest = serde_json::from_value(manifest_json("my-plugin")).unwrap();
        assert_eq!(manifest.mode, PluginMode::Oneshot);

        // 显式 persistent
        let mut value = manifest_json("my-plugin");
        value["mode"] = json!("persistent");
        let manifest: PluginManifest = serde_json::from_value(value).unwrap();
        assert_eq!(manifest.mode, PluginMode::Persistent);

        // 未知值 → 反序列化失败（清单加载即失败，面板展示失败条目）
        let mut value = manifest_json("my-plugin");
        value["mode"] = json!("daemon");
        assert!(serde_json::from_value::<PluginManifest>(value).is_err());
    }

    // -----------------------------------------------------------------------
    // 常驻模式（persistent）：mock 传输
    // -----------------------------------------------------------------------

    /// mock 常驻传输：记录 (input, timeout_ms)，按预置序列逐次应答
    struct MockPersistentTransport {
        responses: Mutex<Vec<Result<String, PluginError>>>,
        calls: Mutex<Vec<(String, u64)>>,
        shutdown_calls: Mutex<usize>,
    }

    impl MockPersistentTransport {
        fn ok(response: &str) -> Arc<Self> {
            Arc::new(Self {
                responses: Mutex::new(vec![Ok(response.to_string())]),
                calls: Mutex::new(Vec::new()),
                shutdown_calls: Mutex::new(0),
            })
        }

        fn shutdown_count(&self) -> usize {
            *self.shutdown_calls.lock().unwrap()
        }
    }

    impl PersistentTransport for MockPersistentTransport {
        fn call(&self, input: &str, timeout: Duration) -> Result<String, PluginError> {
            self.calls
                .lock()
                .unwrap()
                .push((input.to_string(), timeout.as_millis() as u64));
            self.responses
                .lock()
                .unwrap()
                .pop()
                .unwrap_or_else(|| Ok("{}".to_string()))
        }

        fn shutdown(&self) {
            *self.shutdown_calls.lock().unwrap() += 1;
        }
    }

    fn persistent_manifest() -> PluginManifest {
        let mut manifest = base_manifest();
        manifest.mode = PluginMode::Persistent;
        manifest
    }

    #[test]
    fn test_persistent_plugin_request_response_roundtrip() {
        let transport = MockPersistentTransport::ok(r#"{"body": {"model": "rewritten"}}"#);
        let plugin = ExternalPlugin::with_persistent_transport(
            persistent_manifest(),
            Path::new("/tmp/plugins/demo"),
            Arc::new(MockRunner::err("oneshot runner 不应被调用")),
            transport.clone(),
        )
        .unwrap();

        let mut body = json!({"model": "original"});
        let changed = plugin
            .transform_request(&ctx(PluginStage::PreRequest), &mut body)
            .unwrap();
        assert!(changed);
        assert_eq!(body, json!({"model": "rewritten"}));

        // 协议输入与 oneshot 形状一致（stage/settings/body），走常驻通道
        let calls = transport.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let input_value: Value = serde_json::from_str(&calls[0].0).unwrap();
        assert_eq!(input_value["stage"], json!("pre_request"));
        assert_eq!(input_value["body"], json!({"model": "original"}));
        assert_eq!(calls[0].1, 5000);
    }

    #[test]
    fn test_persistent_plugin_sse_event_protocol() {
        let transport = MockPersistentTransport::ok(r#"{"body": {"data": "changed payload"}}"#);
        let mut manifest = persistent_manifest();
        manifest.stages = vec![PluginStage::SseChunk];
        let plugin = ExternalPlugin::with_persistent_transport(
            manifest,
            Path::new("/tmp/plugins/demo"),
            Arc::new(MockRunner::err("oneshot runner 不应被调用")),
            transport.clone(),
        )
        .unwrap();

        let mut data = "original payload".to_string();
        let changed = plugin
            .transform_sse_event(
                &ctx(PluginStage::SseChunk),
                Some("content_block_delta"),
                &mut data,
                &mut (),
            )
            .unwrap();
        assert!(changed);
        assert_eq!(data, "changed payload");

        // 请求携带 event / data 字段
        let input_value: Value =
            serde_json::from_str(&transport.calls.lock().unwrap()[0].0).unwrap();
        assert_eq!(input_value["stage"], json!("sse_chunk"));
        assert_eq!(input_value["event"], json!("content_block_delta"));
        assert_eq!(input_value["data"], json!("original payload"));

        // `{}` → 无修改
        let transport = MockPersistentTransport::ok("{}");
        let mut manifest = persistent_manifest();
        manifest.stages = vec![PluginStage::SseChunk];
        let plugin = ExternalPlugin::with_persistent_transport(
            manifest,
            Path::new("/tmp/plugins/demo"),
            Arc::new(MockRunner::err("oneshot runner 不应被调用")),
            transport,
        )
        .unwrap();
        let mut data = "untouched".to_string();
        let changed = plugin
            .transform_sse_event(&ctx(PluginStage::SseChunk), None, &mut data, &mut ())
            .unwrap();
        assert!(!changed);
        assert_eq!(data, "untouched");
    }

    #[test]
    fn test_persistent_plugin_transport_failure_returns_error() {
        let transport = Arc::new(MockPersistentTransport {
            responses: Mutex::new(vec![Err(PluginError::Execution {
                plugin_id: "persistent".to_string(),
                message: "进程崩溃".to_string(),
            })]),
            calls: Mutex::new(Vec::new()),
            shutdown_calls: Mutex::new(0),
        });
        let plugin = ExternalPlugin::with_persistent_transport(
            persistent_manifest(),
            Path::new("/tmp/plugins/demo"),
            Arc::new(MockRunner::err("oneshot runner 不应被调用")),
            transport,
        )
        .unwrap();

        let mut body = json!({"model": "original"});
        let result = plugin.transform_request(&ctx(PluginStage::PreRequest), &mut body);
        assert!(
            result.is_err(),
            "传输失败应返回 Err（fail-open 由管线处理）"
        );
        assert_eq!(body, json!({"model": "original"}), "失败不得改写 body");
    }

    #[test]
    fn test_persistent_plugin_disabled_shuts_down_transport() {
        // Codex 审查 P2：运行时禁用必须终止常驻子进程（否则第三方进程存活到重载）
        let transport = MockPersistentTransport::ok("{}");
        let plugin = ExternalPlugin::with_persistent_transport(
            persistent_manifest(),
            Path::new("/tmp/plugins/demo"),
            Arc::new(MockRunner::err("oneshot runner 不应被调用")),
            transport.clone(),
        )
        .unwrap();
        assert_eq!(transport.shutdown_count(), 0, "启用状态下不应关停");

        plugin.on_disabled();
        assert_eq!(transport.shutdown_count(), 1);

        // 重复禁用幂等无害
        plugin.on_disabled();
        assert_eq!(transport.shutdown_count(), 2);

        // oneshot 插件没有常驻传输：on_disabled 不应 panic
        let oneshot = ExternalPlugin::new(
            base_manifest(),
            Path::new("/tmp/plugins/demo"),
            Arc::new(MockRunner::ok("{}")),
        )
        .unwrap();
        oneshot.on_disabled();
    }

    // -----------------------------------------------------------------------
    // 命令解析
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_command() {
        let dir = Path::new("/tmp/plugins/demo");

        // 纯命令名：交给 PATH
        assert_eq!(
            resolve_command(&["node".to_string(), "index.js".to_string()], dir),
            vec!["node".to_string(), "index.js".to_string()]
        );

        // 相对路径 + 分隔符：相对插件目录
        assert_eq!(
            resolve_command(&["./scripts/run.sh".to_string()], dir),
            vec![dir.join("./scripts/run.sh").display().to_string()]
        );

        // 绝对路径：保持原样
        assert_eq!(
            resolve_command(&["/usr/bin/node".to_string()], dir),
            vec!["/usr/bin/node".to_string()]
        );
    }

    // -----------------------------------------------------------------------
    // 协议输出解析
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_output_with_body() {
        let mut body = json!({"model": "old"});
        let changed =
            parse_plugin_output("user:x", r#"{"body": {"model": "new"}}"#, &mut body).unwrap();
        assert!(changed);
        assert_eq!(body, json!({"model": "new"}));
    }

    #[test]
    fn test_parse_output_empty_object_no_change() {
        let mut body = json!({"model": "old"});
        let changed = parse_plugin_output("user:x", "  {}  ", &mut body).unwrap();
        assert!(!changed);
        assert_eq!(body, json!({"model": "old"}));
    }

    #[test]
    fn test_parse_output_invalid() {
        let mut body = json!({"model": "old"});

        // 空 stdout
        assert!(parse_plugin_output("user:x", "", &mut body).is_err());
        // 非法 JSON（如混入了日志输出）
        assert!(parse_plugin_output("user:x", "[LOG] ok\n{\"body\":1}", &mut body).is_err());
        // 缺 body 字段
        assert!(parse_plugin_output("user:x", r#"{"other": 1}"#, &mut body).is_err());
        // 非 JSON 对象
        assert!(parse_plugin_output("user:x", "[1,2,3]", &mut body).is_err());

        // 失败不改写原 body
        assert_eq!(body, json!({"model": "old"}));
    }

    // -----------------------------------------------------------------------
    // ExternalPlugin 调用（mock runner）
    // -----------------------------------------------------------------------

    #[test]
    fn test_external_plugin_transform_request_protocol() {
        let runner = Arc::new(MockRunner::ok(r#"{"body": {"model": "rewritten"}}"#));
        let plugin = ExternalPlugin::new(
            base_manifest(),
            Path::new("/tmp/plugins/demo"),
            runner.clone(),
        )
        .unwrap();

        let mut body = json!({"model": "original"});
        let changed = plugin
            .transform_request(&ctx(PluginStage::PreRequest), &mut body)
            .unwrap();

        assert!(changed);
        assert_eq!(body, json!({"model": "rewritten"}));

        // 协议输入包含 stage/settings/body 等字段；超时来自 manifest；
        // 工作目录为插件目录
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let (cmd, cwd, input, timeout_ms) = &calls[0];
        assert_eq!(cmd, &vec!["node".to_string(), "index.js".to_string()]);
        assert_eq!(cwd, Path::new("/tmp/plugins/demo"));
        let input_value: Value = serde_json::from_str(input).unwrap();
        assert_eq!(input_value["stage"], json!("pre_request"));
        assert_eq!(input_value["app_type"], json!("claude"));
        assert_eq!(input_value["session_id"], json!("sess-1"));
        assert_eq!(input_value["settings"], json!(null));
        assert_eq!(input_value["body"], json!({"model": "original"}));
        assert_eq!(*timeout_ms, 5000);
    }

    #[test]
    fn test_external_plugin_runner_error_wrapped_and_fail_open() {
        let runner = Arc::new(MockRunner::err("进程崩溃"));
        let plugin =
            ExternalPlugin::new(base_manifest(), Path::new("/tmp/plugins/demo"), runner).unwrap();

        let mut body = json!({"model": "original"});
        let result = plugin.transform_request(&ctx(PluginStage::PreRequest), &mut body);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("user:my-plugin"),
            "错误应携带插件 id: {err}"
        );
        // fail-open：原 body 未被破坏
        assert_eq!(body, json!({"model": "original"}));
    }

    #[test]
    fn test_external_plugin_skips_undeclared_stage() {
        let runner = Arc::new(MockRunner::ok(r#"{"body": {}}"#));
        let plugin =
            ExternalPlugin::new(base_manifest(), Path::new("/tmp/plugins/demo"), runner).unwrap();

        // manifest 只声明 pre_request，PreSend 不应触发进程调用
        let mut body = json!({"model": "original"});
        let changed = plugin
            .transform_request(&ctx(PluginStage::PreSend), &mut body)
            .unwrap();
        assert!(!changed);
        assert_eq!(body, json!({"model": "original"}));
    }

    #[test]
    fn test_external_plugin_metadata() {
        let mut manifest = base_manifest();
        manifest.id = "demo".to_string();
        manifest.version = Some("1.2.3".to_string());
        manifest.description = Some("示例描述".to_string());
        let plugin = ExternalPlugin::new(
            manifest,
            Path::new("/tmp/plugins/demo"),
            Arc::new(MockRunner::ok("{}")),
        )
        .unwrap();

        assert_eq!(plugin.id(), "user:demo");
        assert!(!plugin.is_builtin());
        assert_eq!(plugin.version().as_deref(), Some("1.2.3"));
        assert_eq!(plugin.description(), "示例描述");
        // source 为插件目录下的 plugin.json（用 PathBuf 比较，避免路径分隔符差异）
        assert_eq!(
            plugin.source().map(PathBuf::from),
            Some(Path::new("/tmp/plugins/demo").join(MANIFEST_FILE))
        );
    }

    // -----------------------------------------------------------------------
    // load_user_plugins（临时目录）
    // -----------------------------------------------------------------------

    #[test]
    fn test_load_user_plugins_missing_dir_silent() {
        let (plugins, errors) = load_user_plugins(Path::new("/nonexistent/cc-switch-plugins"));
        assert!(plugins.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn test_load_user_plugins_success_and_failures() {
        let tmp = tempfile::tempdir().unwrap();

        // 合法插件
        let good_dir = tmp.path().join("good");
        fs::create_dir_all(&good_dir).unwrap();
        fs::write(
            good_dir.join(MANIFEST_FILE),
            serde_json::to_string(&manifest_json("good-plugin")).unwrap(),
        )
        .unwrap();

        // 坏清单：缺必填字段
        let bad_field_dir = tmp.path().join("bad-field");
        fs::create_dir_all(&bad_field_dir).unwrap();
        fs::write(
            bad_field_dir.join(MANIFEST_FILE),
            json!({"id": "bad-field"}).to_string(),
        )
        .unwrap();

        // 非 JSON
        let not_json_dir = tmp.path().join("not-json");
        fs::create_dir_all(&not_json_dir).unwrap();
        fs::write(not_json_dir.join(MANIFEST_FILE), "not json at all").unwrap();

        // 缺 plugin.json
        fs::create_dir_all(tmp.path().join("no-manifest")).unwrap();

        // 清单非法（校验失败：非法 id）
        let bad_id_dir = tmp.path().join("bad id!");
        fs::create_dir_all(&bad_id_dir).unwrap();
        fs::write(
            bad_id_dir.join(MANIFEST_FILE),
            serde_json::to_string(&manifest_json("bad id!")).unwrap(),
        )
        .unwrap();

        let (plugins, errors) = load_user_plugins(tmp.path());

        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].id(), "user:good-plugin");

        let err_dirs: Vec<&str> = errors.iter().map(|e| e.dir_name.as_str()).collect();
        assert_eq!(errors.len(), 4);
        assert!(err_dirs.contains(&"bad-field"), "err_dirs = {err_dirs:?}");
        assert!(err_dirs.contains(&"not-json"));
        assert!(err_dirs.contains(&"no-manifest"));
        assert!(err_dirs.contains(&"bad id!"));
    }

    #[test]
    fn test_load_user_plugins_ignores_regular_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("readme.txt"), "hello").unwrap();

        let (plugins, errors) = load_user_plugins(tmp.path());
        assert!(plugins.is_empty());
        assert!(errors.is_empty());
    }

    // -----------------------------------------------------------------------
    // 声明式配置界面（config_schema + stage=config 协议）
    // -----------------------------------------------------------------------

    fn config_schema_json() -> Value {
        json!([{
            "type": "toggle", "key": "enable_regex", "file": "config.json",
            "label": "启用正则引擎"
        }, {
            "type": "select", "key": "hash.algorithm", "file": "config.json",
            "label": "散列算法", "options": ["blake2b", "sha256"]
        }, {
            "type": "table", "key": "rules", "file": "rules.json", "path": "rules",
            "label": "正则规则",
            "columns": [
                {"key": "enabled", "type": "toggle", "label": "启用"},
                {"key": "name", "type": "text", "label": "名称"}
            ]
        }])
    }

    fn config_manifest() -> PluginManifest {
        let mut value = manifest_json("my-plugin");
        value["mode"] = json!("persistent");
        value["config_schema"] = config_schema_json();
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn test_manifest_config_schema_validation() {
        let manifest = config_manifest();
        assert_eq!(manifest.config_schema.len(), 3);
        manifest.validate().unwrap();

        // select 缺 options → 拒绝
        let mut value = config_schema_json();
        value[1]["options"] = json!([]);
        let mut broken = config_manifest();
        broken.config_schema = serde_json::from_value(value).unwrap();
        assert!(broken.validate().is_err(), "select 缺 options 应拒绝");

        // table 缺 columns → 拒绝
        let mut value = config_schema_json();
        value[2]["columns"] = json!([]);
        let mut broken = config_manifest();
        broken.config_schema = serde_json::from_value(value).unwrap();
        assert!(broken.validate().is_err(), "table 缺 columns 应拒绝");

        // file 路径逃逸 → 拒绝
        let mut value = config_schema_json();
        value[0]["file"] = json!("../escape.json");
        let mut broken = config_manifest();
        broken.config_schema = serde_json::from_value(value).unwrap();
        assert!(broken.validate().is_err(), "file 路径逃逸应拒绝");

        // 非 .json 文件 → 拒绝
        let mut value = config_schema_json();
        value[0]["file"] = json!("config.toml");
        let mut broken = config_manifest();
        broken.config_schema = serde_json::from_value(value).unwrap();
        assert!(broken.validate().is_err(), "非 json 文件应拒绝");
    }

    #[test]
    fn test_persistent_plugin_config_get_set_roundtrip() {
        let transport = Arc::new(MockPersistentTransport {
            // 注意：mock 按尾部弹出，故按调用逆序排列（get → set ok → set err）
            responses: Mutex::new(vec![
                // set 被插件校验拒绝
                Err(PluginError::Execution {
                    plugin_id: "persistent".to_string(),
                    message: "rules[0].pattern 正则无效".to_string(),
                }),
                // set 成功
                Ok("{}".to_string()),
                // get
                Ok(r#"{"config": {"config.json": {"enable_regex": true},
                     "rules.json": {"enabled": true, "rules": []}}}"#.to_string()),
            ]),
            calls: Mutex::new(Vec::new()),
            shutdown_calls: Mutex::new(0),
        });
        let plugin = ExternalPlugin::with_persistent_transport(
            config_manifest(),
            Path::new("/tmp/plugins/demo"),
            Arc::new(MockRunner::err("oneshot runner 不应被调用")),
            transport.clone(),
        )
        .unwrap();

        // schema 透出
        assert_eq!(plugin.config_schema().len(), 3);

        // get：返回插件给的文档 map
        let docs = plugin.config_read().unwrap();
        assert_eq!(docs["config.json"]["enable_regex"], json!(true));
        assert_eq!(docs["rules.json"]["enabled"], json!(true));

        // get 请求协议
        {
            // 锁守卫限定作用域（let 引用绑定会延长临时守卫生命周期，导致死锁）
            let calls = transport.calls.lock().unwrap();
            let parsed: Value = serde_json::from_str(&calls[0].0).unwrap();
            assert_eq!(parsed["stage"], json!("config"));
            assert_eq!(parsed["op"], json!("get"));
        }

        // set 成功
        plugin
            .config_write(&json!({"config.json": {"enable_regex": false}}))
            .unwrap();
        {
            let calls = transport.calls.lock().unwrap();
            let parsed: Value = serde_json::from_str(&calls[1].0).unwrap();
            assert_eq!(parsed["stage"], json!("config"));
            assert_eq!(parsed["op"], json!("set"));
            assert_eq!(parsed["config"]["config.json"]["enable_regex"], json!(false));
        }

        // set 被插件拒绝：错误透传
        let err = plugin
            .config_write(&json!({"rules.json": {"enabled": true}}))
            .unwrap_err();
        assert!(err.to_string().contains("正则无效"), "{err}");
    }

    #[test]
    fn test_oneshot_plugin_config_via_runner() {
        // oneshot 清单（mode 缺省）：config 读写同样走 runner（每次拉起进程）
        let mut value = manifest_json("my-plugin");
        value["config_schema"] = config_schema_json();
        let oneshot_manifest: PluginManifest = serde_json::from_value(value).unwrap();
        let runner = Arc::new(MockRunner::ok(r#"{"config": {"config.json": {}}}"#));
        let plugin =
            ExternalPlugin::new(oneshot_manifest, Path::new("/tmp/plugins/demo"), runner).unwrap();
        let docs = plugin.config_read().unwrap();
        assert!(docs.get("config.json").is_some());
    }
}

#[cfg(test)]
mod real_process_tests {
    //! 真实进程冒烟（#[ignore]，需 node 在 PATH；手动：
    //! `cargo test --lib -- --ignored real_node_plugin`）
    use super::*;

    #[tokio::test]
    #[ignore = "spawn 真实 Node 进程，需 node 在 PATH；仅作手动冒烟"]
    async fn real_node_plugin_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("echo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.json"),
            r#"{"id":"echo","name":"Echo","stages":["pre_request"],"command":["node","echo.js"]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("echo.js"),
            r#"
                let raw = "";
                process.stdin.on("data", (c) => { raw += c; });
                process.stdin.on("end", () => {
                  const input = JSON.parse(raw);
                  input.body.metadata = { touched: true };
                  process.stdout.write(JSON.stringify({ body: input.body }));
                });
            "#,
        )
        .unwrap();

        let (plugins, errors) = load_user_plugins(tmp.path());
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(plugins.len(), 1);
        let plugin = &plugins[0];

        let ctx = PluginRequestContext {
            app_type: "codex".to_string(),
            session_id: "s".to_string(),
            request_model: "gpt-4o".to_string(),
            stage: PluginStage::PreRequest,
            provider: None,
        };
        let mut body = serde_json::json!({"model": "gpt-4o"});
        let changed = plugin
            .transform_request(&ctx, &mut body)
            .expect("plugin transform should succeed");
        assert!(changed);
        assert_eq!(body["metadata"]["touched"], serde_json::json!(true));
    }
}
