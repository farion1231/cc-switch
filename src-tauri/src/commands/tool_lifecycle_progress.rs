//! 安装 / 升级 / 卸载过程的进度上报。
//!
//! 所有写入按全局锁串行（见 `ToolLifecycleCoordinator`），一个慢的升级会让其余的
//! 排队等待；安装脚本又是静默执行的。没有这些事件时界面上只有转圈，用户分不清
//! 「在下载一个 100 MB 的包」「排在别人后面」和「真卡住了」。

use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub(super) const TOOL_LIFECYCLE_PROGRESS_EVENT: &str = "tool-lifecycle-progress";

/// 一行输出最多保留的字符数；界面上只显示一行，再长也看不全。
const MAX_LINE_CHARS: usize = 200;
/// 两次推送输出之间的最短间隔：带 `\r` 的进度条一秒能刷几十行。
const OUTPUT_THROTTLE: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum ToolLifecycleStage {
    /// 前面还有别的工具在安装或升级
    Queued,
    /// 轮到这次操作，命令已开始执行
    Running,
    /// 命令的最新一行输出
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ToolLifecycleProgressPayload {
    pub tools: Vec<String>,
    pub stage: ToolLifecycleStage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
}

type Sink = Arc<dyn Fn(ToolLifecycleProgressPayload) + Send + Sync>;

/// 给一次操作涉及的工具发进度事件。克隆开销很小，可以分别交给排队回调和执行闭包。
#[derive(Clone)]
pub(super) struct ToolLifecycleProgress {
    tools: Vec<String>,
    sink: Sink,
}

impl ToolLifecycleProgress {
    pub fn from_app<R: tauri::Runtime>(app: tauri::AppHandle<R>, tools: &[&str]) -> Self {
        use tauri::Emitter;
        Self::with_sink(
            tools,
            Arc::new(move |payload| {
                match payload.stage {
                    ToolLifecycleStage::Queued => {
                        log::info!(
                            "[tool-lifecycle] {:?} 排队等待前一个写入结束",
                            payload.tools
                        )
                    }
                    ToolLifecycleStage::Running => {
                        log::info!("[tool-lifecycle] {:?} 开始执行", payload.tools)
                    }
                    ToolLifecycleStage::Output => {}
                }
                if let Err(e) = app.emit(TOOL_LIFECYCLE_PROGRESS_EVENT, payload) {
                    log::debug!("推送工具安装进度失败: {e}");
                }
            }),
        )
    }

    pub fn with_sink(tools: &[&str], sink: Sink) -> Self {
        Self {
            tools: tools.iter().map(|tool| tool.to_string()).collect(),
            sink,
        }
    }

    /// 测试用：不上报任何事件。
    #[cfg(all(test, target_os = "windows"))]
    pub fn noop() -> Self {
        Self::with_sink(&[], Arc::new(|_| {}))
    }

    fn send(&self, stage: ToolLifecycleStage, line: Option<String>) {
        (self.sink)(ToolLifecycleProgressPayload {
            tools: self.tools.clone(),
            stage,
            line,
        });
    }

    pub fn queued(&self) {
        self.send(ToolLifecycleStage::Queued, None);
    }

    pub fn running(&self) {
        self.send(ToolLifecycleStage::Running, None);
    }

    pub fn output(&self, line: String) {
        self.send(ToolLifecycleStage::Output, Some(line));
    }
}

/// 和 `Command::output()` 一样执行到结束并收齐 stdout / stderr，
/// 同时把清洗后的每一行（按 `\n` 或 `\r` 断行）节流后交给 `progress`。
pub(super) fn run_with_progress(
    mut command: Command,
    progress: &ToolLifecycleProgress,
) -> std::io::Result<Output> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let throttle = Arc::new(LineThrottle::new(OUTPUT_THROTTLE));
    let readers = [
        child
            .stdout
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
    ]
    .map(|pipe| {
        pipe.map(|pipe| {
            let progress = progress.clone();
            let throttle = throttle.clone();
            std::thread::spawn(move || read_lines(pipe, &progress, &throttle))
        })
    });
    let status = child.wait()?;
    let [stdout, stderr] = readers.map(|reader| {
        reader
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default()
    });
    // 最后一行可能因节流还没推出去；命令结束前补发，界面停在真正的最后一句。
    throttle.flush(progress);
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

/// 读完整个管道并返回全部字节（失败时拼错误详情要用），边读边按行上报。
fn read_lines(
    mut pipe: Box<dyn Read + Send>,
    progress: &ToolLifecycleProgress,
    throttle: &LineThrottle,
) -> Vec<u8> {
    let mut all = Vec::new();
    let mut pending = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = match pipe.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        all.extend_from_slice(&chunk[..read]);
        for &byte in &chunk[..read] {
            if byte == b'\n' || byte == b'\r' {
                offer_line(&pending, progress, throttle);
                pending.clear();
            } else {
                pending.push(byte);
            }
        }
    }
    offer_line(&pending, progress, throttle);
    all
}

fn offer_line(bytes: &[u8], progress: &ToolLifecycleProgress, throttle: &LineThrottle) {
    if bytes.is_empty() {
        return;
    }
    if let Some(line) = clean_progress_line(&super::decode_command_output(bytes)) {
        throttle.offer(line, progress);
    }
}

/// 去掉终端控制序列和不可见字符，过长截断；没有字母或数字的行（空行、纯框线）返回 `None`。
///
/// 转圈动画不一定用 `\r`：有的安装器（如 OpenCode）用「光标回行首 / 清行」序列在同一行
/// 重画，所有帧落在同一「行」里。这类序列之后的内容才是屏幕上最终看到的，取最后一段。
pub(super) fn clean_progress_line(raw: &str) -> Option<String> {
    static REDRAW: Lazy<Regex> = Lazy::new(|| {
        // 光标上移 / 左移 / 换行 / 定位，以及清屏、清行
        Regex::new(r"\x1b\[[0-?]*[ -/]*[ADEFGHJKf]").expect("valid regex")
    });
    REDRAW
        .split(raw)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .find_map(clean_segment)
}

fn clean_segment(raw: &str) -> Option<String> {
    static ESCAPES: Lazy<Regex> = Lazy::new(|| {
        // CSI（颜色、光标移动）与 OSC（窗口标题、超链接）序列
        Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-_]")
            .expect("valid regex")
    });
    let stripped = ESCAPES.replace_all(raw, "");
    let visible: String = stripped
        .chars()
        .map(|c| if c == '\t' { ' ' } else { c })
        .filter(|c| !c.is_control())
        .collect();
    let line = visible.trim();
    if !line.chars().any(char::is_alphanumeric) {
        return None;
    }
    if line.chars().count() <= MAX_LINE_CHARS {
        return Some(line.to_string());
    }
    let mut truncated: String = line.chars().take(MAX_LINE_CHARS).collect();
    truncated.push('…');
    Some(truncated)
}

/// 间隔内来的行先记下最新一行，下一次放行或命令结束时再推。
struct LineThrottle {
    interval: Duration,
    state: Mutex<ThrottleState>,
}

struct ThrottleState {
    last_sent: Option<Instant>,
    held: Option<String>,
}

impl LineThrottle {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            state: Mutex::new(ThrottleState {
                last_sent: None,
                held: None,
            }),
        }
    }

    fn offer(&self, line: String, progress: &ToolLifecycleProgress) {
        let send_now = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let due = state
                .last_sent
                .is_none_or(|sent| sent.elapsed() >= self.interval);
            if due {
                state.last_sent = Some(Instant::now());
                state.held = None;
            } else {
                state.held = Some(line.clone());
            }
            due
        };
        if send_now {
            progress.output(line);
        }
    }

    fn flush(&self, progress: &ToolLifecycleProgress) {
        let held = self
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .held
            .take();
        if let Some(line) = held {
            progress.output(line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recording(
        tools: &[&str],
    ) -> (
        ToolLifecycleProgress,
        Arc<Mutex<Vec<ToolLifecycleProgressPayload>>>,
    ) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink_events = events.clone();
        let progress = ToolLifecycleProgress::with_sink(
            tools,
            Arc::new(move |payload| sink_events.lock().unwrap().push(payload)),
        );
        (progress, events)
    }

    #[test]
    fn from_app_emits_events_listeners_receive() {
        use tauri::Listener;
        let app = tauri::test::mock_app();
        let received = Arc::new(Mutex::new(Vec::new()));
        let sink = received.clone();
        app.listen_any(TOOL_LIFECYCLE_PROGRESS_EVENT, move |event| {
            sink.lock().unwrap().push(event.payload().to_string());
        });
        let progress = ToolLifecycleProgress::from_app(app.handle().clone(), &["gemini"]);
        progress.queued();
        progress.running();
        progress.output("added 1 package".into());
        let received = received.lock().unwrap();
        assert_eq!(
            *received,
            [
                r#"{"tools":["gemini"],"stage":"queued"}"#,
                r#"{"tools":["gemini"],"stage":"running"}"#,
                r#"{"tools":["gemini"],"stage":"output","line":"added 1 package"}"#,
            ]
        );
    }

    #[test]
    fn clean_progress_line_strips_escapes_and_truncates() {
        assert_eq!(
            clean_progress_line("\x1b[1;32m==>\x1b[0m [4/5] Installing\t pkg  "),
            Some("==> [4/5] Installing  pkg".to_string())
        );
        assert_eq!(
            clean_progress_line("\x1b]0;title\x07\x1b[2K   "),
            None,
            "只有控制序列的行不算输出"
        );
        // 用光标序列原地重画的转圈动画只留最后一帧；末尾的清行不吞掉前面的内容
        assert_eq!(
            clean_progress_line(
                "\x1b[?25l◒  Upgrading\x1b[999D\x1b[J◐  Upgrading\x1b[1G\x1b[2K◓  Upgrading\x1b[2K"
            ),
            Some("◓  Upgrading".to_string())
        );
        assert_eq!(clean_progress_line("│"), None, "纯框线不算输出");
        assert_eq!(clean_progress_line("└  Done"), Some("└  Done".to_string()));
        let long = "下".repeat(MAX_LINE_CHARS + 10);
        let cleaned = clean_progress_line(&long).unwrap();
        assert_eq!(cleaned.chars().count(), MAX_LINE_CHARS + 1);
        assert!(cleaned.ends_with('…'));
    }

    #[test]
    fn progress_payload_serializes_for_frontend() {
        let payload = ToolLifecycleProgressPayload {
            tools: vec!["mcode".into()],
            stage: ToolLifecycleStage::Queued,
            line: None,
        };
        assert_eq!(
            serde_json::to_value(payload).unwrap(),
            serde_json::json!({ "tools": ["mcode"], "stage": "queued" })
        );
    }

    #[test]
    fn throttle_holds_fast_lines_and_flushes_the_last_one() {
        let (progress, events) = recording(&["codex"]);
        let throttle = LineThrottle::new(Duration::from_secs(60));
        throttle.offer("first".into(), &progress);
        throttle.offer("second".into(), &progress);
        throttle.offer("third".into(), &progress);
        throttle.flush(&progress);
        throttle.flush(&progress);
        let lines: Vec<_> = events
            .lock()
            .unwrap()
            .iter()
            .map(|event| event.line.clone().unwrap())
            .collect();
        assert_eq!(lines, ["first", "third"]);
    }

    #[cfg(unix)]
    #[test]
    fn run_with_progress_collects_output_and_reports_lines() {
        let (progress, events) = recording(&["claude"]);
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("printf 'downloading 10%%\\rdownloading 90%%\\n'; printf 'oops\\n' >&2; exit 3");
        let output = run_with_progress(command, &progress).unwrap();
        assert_eq!(output.status.code(), Some(3));
        assert_eq!(output.stdout, b"downloading 10%\rdownloading 90%\n");
        assert_eq!(output.stderr, b"oops\n");
        let events = events.lock().unwrap();
        assert!(events
            .iter()
            .all(|event| event.stage == ToolLifecycleStage::Output && event.tools == ["claude"]));
        // 节流下中间行可能被跳过，但第一行与最后一行一定送达
        assert!(!events.is_empty());
        let last = events.last().unwrap().line.as_deref().unwrap();
        assert!(last == "oops" || last == "downloading 90%", "{last}");
    }
}
