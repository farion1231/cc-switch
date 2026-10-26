//! 加账号：在账号目录里运行官方的 `claude auth login --claudeai`。
//!
//! 登录页、令牌、存到哪里全由 Claude Code 负责，CC Switch 不经手任何凭据，只做三件事：
//! - 拿到登录链接给窗口显示、打开。macOS / Linux 把 `BROWSER` 设成账号目录旁的小脚本，
//!   Claude Code 要打开的那个链接（登录后回到本机端口）会被写进文件；Windows 上让 Claude
//!   Code 自己打开浏览器。两种情况下它打印出来的链接都作后备（那个页面显示一段代码让人粘贴）。
//! - 浏览器回不到本机时，替用户把回调地址转交给本机端口，或把页面上的代码写进 stdin。
//! - 等它退出，退出码 0 就是登录成功。

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use regex::Regex;
use tokio::sync::oneshot;

use crate::error::AppError;

use super::layout;

/// 等登录链接最多这么久（Claude Code 首次启动可能要初始化一会儿）。
const LINK_WAIT: Duration = Duration::from_secs(30);
/// 已经打印出链接后，再等它去「打开浏览器」的这么久：截获到的链接更好用。
const OPENED_GRACE: Duration = Duration::from_secs(3);
/// 定位 `claude` 可执行文件的时限。
const LOCATE_TIMEOUT: Duration = Duration::from_secs(15);

/// 截获 `BROWSER` 的脚本把链接写到这个变量指向的文件。
const OPENED_URL_ENV: &str = "CC_SWITCH_CLAUDE_OPENED_URL";

/// 交给账号目录里 Claude Code 之前要清掉的变量：任何一个都会把它指向别的登录或别的服务。
pub const CLAUDE_ACCOUNT_UNSET: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_CUSTOM_HEADERS",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_REFRESH_TOKEN",
    "CLAUDE_CODE_OAUTH_SCOPES",
    // 协议选择器：和 `live::floor::CLAUDE_PROTOCOL_SELECTORS` 一致（单测核对）
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "CLAUDE_CODE_USE_GATEWAY",
    "CLAUDE_CODE_USE_MANTLE",
    "CLAUDE_CODE_USE_ANTHROPIC_AWS",
    "CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD",
    "CLAUDE_SECURESTORAGE_CONFIG_DIR",
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
];

/// 一次登录的最终结果。
#[derive(Debug)]
pub struct LoginExit {
    pub success: bool,
    /// 失败时 Claude Code 最后说的话（或超时 / 取消的说明）
    pub message: String,
}

/// 正在运行的 `claude auth login`。
pub struct RunningLogin {
    pub auth_url: String,
    /// Claude Code 自己已经打开了浏览器（Windows 上不截获 `BROWSER`），窗口不用再开一次
    pub browser_opened: bool,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    /// 登录链接里 `redirect_uri` 的本机端口；只有截获到的链接才有
    callback_port: Option<u16>,
    cancel: Arc<AtomicBool>,
    pub done: oneshot::Receiver<LoginExit>,
}

impl RunningLogin {
    /// 结束这次登录（杀掉子进程）。
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// 浏览器回不到本机时，用户粘贴回来的东西：落地页的完整地址（转交给本机端口），
    /// 或登录页显示的代码（`code#state`，写进 stdin）。
    pub async fn submit_paste(&self, raw: &str) -> Result<(), AppError> {
        let raw = raw.trim();
        if let Ok(url) = url::Url::parse(raw) {
            if matches!(url.scheme(), "http" | "https") {
                return self.forward_callback(&url).await;
            }
        }
        if looks_like_code(raw) {
            let stdin = Arc::clone(&self.stdin);
            let line = format!("{raw}\n");
            return tokio::task::spawn_blocking(move || {
                let mut guard = stdin.lock().unwrap_or_else(|e| e.into_inner());
                let Some(stdin) = guard.as_mut() else {
                    return Err(login_over());
                };
                stdin
                    .write_all(line.as_bytes())
                    .and_then(|_| stdin.flush())
                    .map_err(|_| login_over())
            })
            .await
            .map_err(|error| AppError::Message(error.to_string()))?;
        }
        Err(AppError::localized(
            "claude.account.paste_unrecognized",
            "请粘贴登录页显示的代码，或浏览器最后停留的完整地址",
            "Paste the code the sign-in page shows, or the full address the browser ended on.",
        ))
    }

    async fn forward_callback(&self, url: &url::Url) -> Result<(), AppError> {
        let not_ours = || {
            AppError::localized(
                "claude.account.paste_wrong_address",
                "这个地址不是这次登录的，请粘贴这次登录的浏览器标签最后停留的地址",
                "That address isn't from this sign-in. Paste the one this sign-in's browser tab ended on.",
            )
        };
        let port = self.callback_port.ok_or_else(not_ours)?;
        let host_ok = url
            .host_str()
            .is_some_and(|host| matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1"));
        let has_code = url.query_pairs().any(|(key, _)| key == "code");
        if !host_ok || url.port() != Some(port) || !has_code {
            return Err(not_ours());
        }
        let target = format!(
            "http://localhost:{port}{}{}",
            url.path(),
            url.query().map(|q| format!("?{q}")).unwrap_or_default()
        );
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| AppError::Message(error.to_string()))?;
        client
            .get(&target)
            .send()
            .await
            .map_err(|error| AppError::Message(format!("Claude Code 没有接收这个地址: {error}")))?;
        Ok(())
    }
}

fn login_over() -> AppError {
    AppError::localized(
        "claude.account.login_over",
        "这次登录已结束，请重新开始",
        "This sign-in has ended. Start it again.",
    )
}

/// 登录页显示的代码是 `code#state` 的样子，没有空白。
fn looks_like_code(raw: &str) -> bool {
    match raw.split_once('#') {
        Some((code, state)) => {
            !code.is_empty() && !state.is_empty() && !raw.chars().any(char::is_whitespace)
        }
        None => false,
    }
}

/// 在 `config_dir` 里启动 `claude auth login --claudeai`，等到拿到登录链接再返回。
/// `home` 是账号的外层目录（放截获脚本）。`timeout` 到了还没登完就杀掉子进程。
pub async fn start(
    config_dir: PathBuf,
    home: PathBuf,
    timeout: Duration,
) -> Result<RunningLogin, AppError> {
    let opened_file = home.join(layout::OPENED_URL_FILENAME);
    let _ = std::fs::remove_file(&opened_file);
    let opener = write_opener(&home)?;

    let spawn = {
        let config_dir = config_dir.clone();
        let opened_file = opened_file.clone();
        tokio::task::spawn_blocking(move || {
            spawn_login(&config_dir, opener.as_deref(), &opened_file)
        })
    };
    let mut child = spawn
        .await
        .map_err(|error| AppError::Message(error.to_string()))??;

    let stdin = Arc::new(Mutex::new(child.stdin.take()));
    let cancel = Arc::new(AtomicBool::new(false));
    let (printed_tx, mut printed_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let last_line = Arc::new(Mutex::new(String::new()));

    let readers: Vec<_> = [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    .map(|stream| {
        let printed_tx = printed_tx.clone();
        let last_line = Arc::clone(&last_line);
        std::thread::spawn(move || read_output(stream, &printed_tx, &last_line))
    })
    .collect();
    drop(printed_tx);

    let (done_tx, done_rx) = oneshot::channel();
    let exited = Arc::new(AtomicBool::new(false));
    {
        let cancel = Arc::clone(&cancel);
        let exited = Arc::clone(&exited);
        let stdin = Arc::clone(&stdin);
        let last_line = Arc::clone(&last_line);
        std::thread::spawn(move || {
            let outcome = supervise(&mut child, &cancel, timeout);
            exited.store(true, Ordering::SeqCst);
            // 关掉 stdin，读输出的线程会在管道关闭后结束
            stdin.lock().unwrap_or_else(|e| e.into_inner()).take();
            for reader in readers {
                let _ = reader.join();
            }
            let said = last_line.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let exit = match outcome {
                Supervised::Exited(true) => LoginExit {
                    success: true,
                    message: String::new(),
                },
                Supervised::Exited(false) => LoginExit {
                    success: false,
                    message: if said.is_empty() {
                        "claude auth login 没有完成".to_string()
                    } else {
                        said
                    },
                },
                Supervised::TimedOut => LoginExit {
                    success: false,
                    message: "登录超时".to_string(),
                },
                Supervised::Cancelled => LoginExit {
                    success: false,
                    message: "登录已取消".to_string(),
                },
            };
            let _ = done_tx.send(exit);
        });
    }

    let mut done_rx = done_rx;
    let auth_url = wait_for_link(&opened_file, &mut printed_rx, &exited, &mut done_rx).await;
    let (auth_url, intercepted) = match auth_url {
        Ok(found) => found,
        Err(error) => {
            cancel.store(true, Ordering::SeqCst);
            return Err(error);
        }
    };
    let callback_port = callback_port(&auth_url);
    Ok(RunningLogin {
        auth_url,
        // 截获到的链接是 CC Switch 拦下来的，浏览器还没开；只打印出来的链接，Windows 上
        // Claude Code 已经自己去开浏览器了，macOS / Linux 上则是它没打算开（没有图形界面）
        browser_opened: !intercepted && cfg!(windows),
        stdin,
        callback_port,
        cancel,
        done: done_rx,
    })
}

enum Supervised {
    Exited(bool),
    TimedOut,
    Cancelled,
}

fn supervise(child: &mut Child, cancel: &AtomicBool, timeout: Duration) -> Supervised {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Supervised::Exited(status.success()),
            Ok(None) => {}
            Err(_) => return Supervised::Exited(false),
        }
        let reason = if cancel.load(Ordering::SeqCst) {
            Some(Supervised::Cancelled)
        } else if started.elapsed() >= timeout {
            Some(Supervised::TimedOut)
        } else {
            None
        };
        if let Some(reason) = reason {
            // 整树终止：Windows 上 npm 装的 claude.cmd 直接子进程是 cmd.exe，真正等授权的
            // node 是它的后代，只杀 cmd.exe 的话 node 还占着输出管道，读输出的线程等不到 EOF
            crate::commands::terminate_child_tree(child);
            let _ = child.wait();
            return reason;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn read_output(
    stream: Box<dyn Read + Send>,
    printed: &tokio::sync::mpsc::UnboundedSender<String>,
    last_line: &Mutex<String>,
) {
    let url_pattern = Regex::new(r"https://\S+").expect("valid regex");
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { break };
        let line = strip_ansi(&line);
        if let Some(found) = url_pattern.find(&line) {
            // 只认授权页：更新提示、文档之类的链接也可能先打印出来
            if is_authorize_url(found.as_str()) {
                let _ = printed.send(found.as_str().to_string());
            }
            continue;
        }
        let line = line
            .trim()
            .trim_start_matches("Paste code here if prompted >")
            .trim();
        if !line.is_empty() {
            *last_line.lock().unwrap_or_else(|e| e.into_inner()) = line.to_string();
        }
    }
}

fn strip_ansi(line: &str) -> String {
    let pattern = Regex::new(r"\x1b\[[0-9;?]*[A-Za-z]").expect("valid regex");
    pattern.replace_all(line, "").into_owned()
}

/// 截获到的链接优先；只有打印出来的链接时，再等一小会儿看 Claude Code 会不会去打开浏览器。
async fn wait_for_link(
    opened_file: &Path,
    printed: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
    exited: &AtomicBool,
    done: &mut oneshot::Receiver<LoginExit>,
) -> Result<(String, bool), AppError> {
    let started = Instant::now();
    let mut printed_url: Option<(String, Instant)> = None;
    loop {
        if let Ok(text) = std::fs::read_to_string(opened_file) {
            let url = text.trim();
            if url.starts_with("https://") {
                return Ok((url.to_string(), true));
            }
        }
        while let Ok(url) = printed.try_recv() {
            if printed_url.is_none() {
                printed_url = Some((url, Instant::now()));
            }
        }
        if let Some((url, at)) = &printed_url {
            if at.elapsed() >= OPENED_GRACE {
                return Ok((url.clone(), false));
            }
        }
        if exited.load(Ordering::SeqCst) {
            if let Some((url, _)) = printed_url {
                return Ok((url, false));
            }
            let message = match done.try_recv() {
                Ok(exit) => exit.message,
                Err(_) => "claude auth login 已退出".to_string(),
            };
            return Err(AppError::Message(format!(
                "claude auth login 没有给出登录链接: {message}"
            )));
        }
        if started.elapsed() >= LINK_WAIT {
            if let Some((url, _)) = printed_url {
                return Ok((url, false));
            }
            return Err(AppError::Message(
                "claude auth login 没有给出登录链接，请在终端里运行 `claude auth login` 看看它输出了什么"
                    .to_string(),
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Claude 的授权页（`claude.com/cai/oauth/authorize`、`platform.claude.com/oauth/authorize`）。
fn is_authorize_url(text: &str) -> bool {
    url::Url::parse(text).is_ok_and(|url| {
        url.path().ends_with("/oauth/authorize")
            && url.host_str().is_some_and(|host| {
                host == "claude.com" || host.ends_with(".claude.com") || host == "claude.ai"
            })
    })
}

/// 登录链接里 `redirect_uri` 指向本机时的端口。
fn callback_port(auth_url: &str) -> Option<u16> {
    let url = url::Url::parse(auth_url).ok()?;
    let redirect = url
        .query_pairs()
        .find(|(key, _)| key == "redirect_uri")
        .map(|(_, value)| value.into_owned())?;
    let redirect = url::Url::parse(&redirect).ok()?;
    match redirect.host_str()? {
        "localhost" | "127.0.0.1" | "[::1]" | "::1" => redirect.port(),
        _ => None,
    }
}

/// macOS / Linux：写截获 `BROWSER` 的脚本。Windows 返回 `None`，让 Claude Code 自己打开浏览器。
fn write_opener(home: &Path) -> Result<Option<PathBuf>, AppError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = home.join(layout::OPENER_FILENAME);
        let script = format!(
            "#!/bin/sh\n[ -n \"${OPENED_URL_ENV}\" ] && printf '%s' \"$1\" > \"${OPENED_URL_ENV}\"\nexit 0\n"
        );
        std::fs::write(&path, script).map_err(|source| AppError::io(&path, source))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .map_err(|source| AppError::io(&path, source))?;
        Ok(Some(path))
    }
    #[cfg(not(unix))]
    {
        let _ = home;
        Ok(None)
    }
}

fn spawn_login(
    config_dir: &Path,
    opener: Option<&Path>,
    opened_file: &Path,
) -> Result<Child, AppError> {
    let mut cmd = crate::commands::detected_tool_command(
        "claude",
        &["auth", "login", "--claudeai"],
        LOCATE_TIMEOUT,
    )
    .map_err(AppError::Message)?;
    apply_account_env(&mut cmd, config_dir);
    cmd.env_remove("BROWSER");
    if let Some(opener) = opener {
        cmd.env("BROWSER", opener).env(OPENED_URL_ENV, opened_file);
    }
    cmd.current_dir(config_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.spawn()
        .map_err(|error| AppError::Message(format!("无法启动 claude auth login: {error}")))
}

/// 让命令只用这个账号目录：清掉会指向别处的变量，设 `CLAUDE_CONFIG_DIR`。
pub fn apply_account_env(cmd: &mut std::process::Command, config_dir: &Path) {
    clear_account_env(cmd);
    cmd.env("CLAUDE_CONFIG_DIR", config_dir);
}

/// 清掉会把 Claude Code 指到别的登录、别的接口的变量（`CLAUDE_CONFIG_DIR` 由调用方决定）。
pub fn clear_account_env(cmd: &mut std::process::Command) {
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy();
        if name.starts_with("ANTHROPIC_") || name.starts_with("CLAUDE_CODE_USE_") {
            cmd.env_remove(&key);
        }
    }
    for key in CLAUDE_ACCOUNT_UNSET {
        cmd.env_remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 取消登录要终止整棵进程树：后代进程还占着输出管道的话，读输出的线程永远等不到
    /// EOF（Windows 上 claude.cmd → node 就是这样）。这里用 sh → sleep 模拟。
    #[cfg(unix)]
    #[test]
    fn cancel_ends_descendants_holding_the_output_pipe() {
        use std::os::unix::process::CommandExt;

        let mut cmd = std::process::Command::new("sh");
        cmd.args(["-c", "sleep 30 & echo ready; wait"])
            .stdout(Stdio::piped())
            // 和 detected_tool_command 一样放进单独的进程组
            .process_group(0);
        let mut child = cmd.spawn().expect("spawn sh");
        let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));
        // 等后代进程起来再取消
        let mut ready = String::new();
        stdout.read_line(&mut ready).expect("read ready");
        assert_eq!(ready.trim(), "ready");
        let (eof_tx, eof_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut stdout, &mut std::io::sink());
            let _ = eof_tx.send(());
        });

        let cancel = AtomicBool::new(true);
        assert!(matches!(
            supervise(&mut child, &cancel, Duration::from_secs(60)),
            Supervised::Cancelled
        ));
        assert!(
            eof_rx.recv_timeout(Duration::from_secs(5)).is_ok(),
            "the grandchild still holds the pipe"
        );
    }

    #[test]
    fn unset_list_covers_every_protocol_selector() {
        // 从卡片打开终端时只能按名字清，漏一个选择器 claude 就可能走别的服务
        for selector in crate::live::floor::CLAUDE_PROTOCOL_SELECTORS {
            assert!(
                CLAUDE_ACCOUNT_UNSET.contains(selector),
                "{selector} 不在 CLAUDE_ACCOUNT_UNSET 里"
            );
        }
    }

    #[test]
    fn only_authorize_pages_count_as_login_links() {
        assert!(is_authorize_url(
            "https://claude.com/cai/oauth/authorize?code=true&client_id=x"
        ));
        assert!(is_authorize_url(
            "https://platform.claude.com/oauth/authorize?client_id=x"
        ));
        assert!(!is_authorize_url(
            "https://docs.claude.com/en/docs/claude-code"
        ));
        assert!(!is_authorize_url("https://evil.example/oauth/authorize"));
    }

    #[test]
    fn recognizes_pasted_codes() {
        assert!(looks_like_code("abc123#state456"));
        assert!(!looks_like_code("abc123"));
        assert!(!looks_like_code("#state"));
        assert!(!looks_like_code("abc #state"));
    }

    #[test]
    fn callback_port_only_for_loopback() {
        let local = "https://claude.ai/oauth/authorize?client_id=x&redirect_uri=http%3A%2F%2Flocalhost%3A54545%2Fcallback&state=s";
        assert_eq!(callback_port(local), Some(54545));
        let remote = "https://claude.ai/oauth/authorize?redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback";
        assert_eq!(callback_port(remote), None);
        assert_eq!(callback_port("not a url"), None);
    }

    #[test]
    fn strips_ansi_sequences() {
        assert_eq!(strip_ansi("\x1b[1mhello\x1b[0m"), "hello");
    }
}
