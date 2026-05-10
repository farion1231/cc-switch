//! 流式命令执行工具
//!
//! 把外部进程的 stdout/stderr 逐行实时推送到前端，用于一键安装/卸载这类
//! 长任务的进度可视化。同时维护一个会话级 ProcessRegistry，让前端可以
//! 通过 cancel_install 请求中止正在执行的命令。

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::Utc;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{Mutex, Notify};

/// 推送给前端的事件名（统一一个事件名 + payload 内带 channelId 由前端过滤）
pub const EVENT_LOG: &str = "install-log";
pub const EVENT_DONE: &str = "install-log-done";

/// 日志行的来源
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogStream {
    /// 子进程标准输出
    Stdout,
    /// 子进程标准错误
    Stderr,
    /// 业务侧自定义进度（如「Step 2/5: 备份 ~/.claude/」）
    Info,
    /// 业务侧报告的错误
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub channel_id: String,
    pub stream: LogStream,
    pub line: String,
    pub ts: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoneEvent {
    pub channel_id: String,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub cancelled: bool,
}

/// 会话级取消状态
///
/// 在一次 install/uninstall 会话内多个 stream_command 调用之间共享，
/// 业务侧也可读取 `cancel_token` 在步骤之间检查是否需要中止。
#[derive(Clone)]
pub struct SessionState {
    pub cancel_token: Arc<AtomicBool>,
    pub cancel_notify: Arc<Notify>,
}

/// 全局进程注册表（通过 Tauri State 注入）
#[derive(Clone, Default)]
pub struct ProcessRegistry {
    inner: Arc<Mutex<RegistryInner>>,
}

#[derive(Default)]
struct RegistryInner {
    sessions: HashMap<String, SessionState>,
}

impl ProcessRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 顶层命令开始时调用，初始化一个会话状态
    pub async fn begin_session(&self, channel_id: &str) -> SessionState {
        let mut inner = self.inner.lock().await;
        let state = SessionState {
            cancel_token: Arc::new(AtomicBool::new(false)),
            cancel_notify: Arc::new(Notify::new()),
        };
        inner.sessions.insert(channel_id.to_string(), state.clone());
        state
    }

    /// 顶层命令结束时调用，清理会话状态
    pub async fn end_session(&self, channel_id: &str) {
        let mut inner = self.inner.lock().await;
        inner.sessions.remove(channel_id);
    }

    /// 由前端 cancel_install 触发：标记取消并唤醒所有等待的 stream_command
    pub async fn cancel(&self, channel_id: &str) -> bool {
        let inner = self.inner.lock().await;
        if let Some(s) = inner.sessions.get(channel_id) {
            s.cancel_token.store(true, Ordering::SeqCst);
            s.cancel_notify.notify_waiters();
            true
        } else {
            false
        }
    }

    /// 业务侧在步骤之间查询当前会话是否已被取消
    pub async fn is_cancelled(&self, channel_id: &str) -> bool {
        let inner = self.inner.lock().await;
        inner
            .sessions
            .get(channel_id)
            .map(|s| s.cancel_token.load(Ordering::SeqCst))
            .unwrap_or(false)
    }
}

/// 推一行日志到前端
pub fn emit_line(app: &AppHandle, channel_id: &str, stream: LogStream, line: impl Into<String>) {
    let line_text = line.into();
    let payload = LogLine {
        channel_id: channel_id.to_string(),
        stream,
        line: line_text.clone(),
        ts: Utc::now().timestamp_millis(),
    };
    if let Err(e) = app.emit(EVENT_LOG, payload) {
        log::warn!("emit {} failed: {}", EVENT_LOG, e);
    }
    match stream {
        LogStream::Stderr | LogStream::Error => log::warn!("[{}] {}", channel_id, line_text),
        _ => log::info!("[{}] {}", channel_id, line_text),
    }
}

/// 推业务侧自定义进度（INFO 类）
pub fn emit_progress(app: &AppHandle, channel_id: &str, msg: impl Into<String>) {
    emit_line(app, channel_id, LogStream::Info, msg);
}

/// 推业务侧错误行（前端高亮）
pub fn emit_error_line(app: &AppHandle, channel_id: &str, msg: impl Into<String>) {
    emit_line(app, channel_id, LogStream::Error, msg);
}

/// 推 done 事件
pub fn emit_done(
    app: &AppHandle,
    channel_id: &str,
    success: bool,
    exit_code: Option<i32>,
    cancelled: bool,
) {
    let payload = DoneEvent {
        channel_id: channel_id.to_string(),
        success,
        exit_code,
        cancelled,
    };
    if let Err(e) = app.emit(EVENT_DONE, payload) {
        log::warn!("emit {} failed: {}", EVENT_DONE, e);
    }
}

/// 命令执行结果摘要
#[derive(Debug, Clone, Copy)]
pub struct StreamOutcome {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub cancelled: bool,
}

/// 流式执行命令：spawn + 异步逐行读 stdout/stderr 并 emit 到前端
///
/// `state` 由调用方从 ProcessRegistry::begin_session 拿到，整段会话共用同一份。
///
/// 取消机制：
/// - 进入 select! 前先 pin 一个 `notified()` future 防止 notify 丢失
/// - 已取消时直接 start_kill 并 wait 收尸
/// - 进程被 kill 后 stdout/stderr 管道 EOF，读任务自然结束
///
/// 限制：Windows 下 kill 仅作用于直接子进程，由 cmd /C 启动的 npm/node 等
/// 孙进程可能成为孤儿，依赖 OS 自行回收。
pub async fn stream_command(
    app: &AppHandle,
    state: &SessionState,
    channel_id: &str,
    program: &str,
    args: &[&str],
) -> Result<StreamOutcome, String> {
    // 已取消则直接返回，不浪费 spawn
    if state.cancel_token.load(Ordering::SeqCst) {
        emit_progress(app, channel_id, "[已取消，跳过命令]");
        return Ok(StreamOutcome {
            success: false,
            exit_code: None,
            cancelled: true,
        });
    }

    let display = if args.is_empty() {
        format!("$ {}", program)
    } else {
        format!("$ {} {}", program, args.join(" "))
    };
    emit_progress(app, channel_id, display);

    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| {
            let msg = format!("spawn 失败 ({}): {}", program, e);
            emit_error_line(app, channel_id, msg.clone());
            msg
        })?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法获取 stdout 句柄".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "无法获取 stderr 句柄".to_string())?;

    let app_out = app.clone();
    let app_err = app.clone();
    let cid_out = channel_id.to_string();
    let cid_err = channel_id.to_string();

    let stdout_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        loop {
            match reader.next_line().await {
                Ok(Some(line)) => emit_line(&app_out, &cid_out, LogStream::Stdout, line),
                Ok(None) => break,
                Err(e) => {
                    emit_line(
                        &app_out,
                        &cid_out,
                        LogStream::Stderr,
                        format!("[stdout 读取错误: {}]", e),
                    );
                    break;
                }
            }
        }
    });

    let stderr_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        loop {
            match reader.next_line().await {
                Ok(Some(line)) => emit_line(&app_err, &cid_err, LogStream::Stderr, line),
                Ok(None) => break,
                Err(e) => {
                    emit_line(
                        &app_err,
                        &cid_err,
                        LogStream::Stderr,
                        format!("[stderr 读取错误: {}]", e),
                    );
                    break;
                }
            }
        }
    });

    // 必须先 pin 一个 notified() future，再进 select!，
    // 这样即便 cancel 紧接其后调用 notify_waiters() 也不会丢信号。
    let notified = state.cancel_notify.notified();
    tokio::pin!(notified);

    let wait_result = if state.cancel_token.load(Ordering::SeqCst) {
        let _ = child.start_kill();
        child.wait().await
    } else {
        tokio::select! {
            res = child.wait() => res,
            _ = &mut notified => {
                emit_progress(app, channel_id, "[收到取消信号，正在终止子进程...]");
                let _ = child.start_kill();
                child.wait().await
            }
        }
    };

    let _ = stdout_task.await;
    let _ = stderr_task.await;

    let cancelled = state.cancel_token.load(Ordering::SeqCst);

    match wait_result {
        Ok(status) => Ok(StreamOutcome {
            success: status.success() && !cancelled,
            exit_code: status.code(),
            cancelled,
        }),
        Err(e) => {
            if cancelled {
                Ok(StreamOutcome {
                    success: false,
                    exit_code: None,
                    cancelled: true,
                })
            } else {
                let msg = format!("等待子进程结束失败: {}", e);
                emit_error_line(app, channel_id, msg.clone());
                Err(msg)
            }
        }
    }
}

/// 仅捕获输出但不流式（用于安装后 verify --version 这类短命令）
///
/// 不向前端 emit，但仍受会话取消标记影响。
pub async fn capture_command(
    state: &SessionState,
    program: &str,
    args: &[&str],
) -> Result<(bool, String, String), String> {
    if state.cancel_token.load(Ordering::SeqCst) {
        return Err("已取消".to_string());
    }
    let output = Command::new(program)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("执行 {} 失败: {}", program, e))?;
    Ok((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    ))
}
