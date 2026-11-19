//! macOS 钥匙串：照 Claude Code 自己的读写方式用 `/usr/bin/security`。
//!
//! - 读：`find-generic-password -a <account> -s <service> -w`，输出就是凭据 JSON；
//! - 写：`security -i` 从 stdin 读一行 `add-generic-password -U -a … -s … -X <hex>`，令牌不出现在
//!   命令行参数里（`ps` 看不到）；一行超出 `security -i` 的缓冲时（连了 OAuth 类 MCP 服务器后
//!   `mcpOAuth` 很容易撑大），和 Claude Code 一样改经命令行参数写；
//! - 删：`delete-generic-password`。
//!
//! Claude Code 写钥匙串失败（非暂时性）时会改写槽位里的 `.credentials.json` 并删掉条目，
//! 所以读的时候钥匙串没有就看那个文件；写进钥匙串后删掉它，和 Claude Code 一致。

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::{CredentialStore, FileStore, Secret, Slot};
use crate::error::AppError;
use crate::services::claude_account::layout;

const SECURITY: &str = "/usr/bin/security";
const TIMEOUT: Duration = Duration::from_secs(5);
/// `security -i` 一行的上限（它按 4096 字节的缓冲读行，超出的部分会被当成下一条命令）。
/// 超过时和 Claude Code 一样改走命令行参数：令牌会短暂出现在进程列表里，但这一条 Claude Code
/// 自己每次刷新令牌也是这么写的，拒绝写入只会让切换失败（#9002）。
const MAX_LINE: usize = 4032;
/// 没有这个条目
const EXIT_NOT_FOUND: i32 = 44;
/// 钥匙串锁着
const EXIT_LOCKED: i32 = 36;

pub struct KeychainStore;

impl CredentialStore for KeychainStore {
    fn read(&self, slot: &Slot) -> Result<Option<Secret>, AppError> {
        let output = run(
            Command::new(SECURITY).args([
                "find-generic-password",
                "-a",
                &layout::keychain_account(),
                "-s",
                &slot.keychain_service(),
                "-w",
            ]),
            None,
        )?;
        match output.code {
            0 => {}
            EXIT_NOT_FOUND => return FileStore.read(slot),
            EXIT_LOCKED => return Err(locked()),
            _ => return Err(failed("read")),
        }
        let text = String::from_utf8(output.stdout).map_err(|_| failed("read"))?;
        let text = text.trim_end_matches(['\n', '\r']);
        if text.trim().is_empty() {
            return Ok(None);
        }
        // `-w` 遇到不可打印的内容会改输出 hex
        match Secret::parse(text.as_bytes()) {
            Ok(secret) => Ok(Some(secret)),
            Err(error) => match hex_decode(text) {
                Some(bytes) => Secret::parse(&bytes).map(Some),
                None => Err(error),
            },
        }
    }

    fn write(&self, slot: &Slot, secret: &Secret) -> Result<(), AppError> {
        let hex = hex_encode(&secret.to_bytes());
        let output = match add_invocation(
            &layout::keychain_account(),
            &slot.keychain_service(),
            &hex,
        ) {
            AddInvocation::Stdin(line) => {
                run(Command::new(SECURITY).arg("-i"), Some(line.as_bytes()))?
            }
            AddInvocation::Args(args) => {
                log::warn!(
                    "[ClaudeAccount] 登录信息超出 security -i 缓冲（{} 字节），改经命令行参数写入钥匙串",
                    hex.len() / 2
                );
                run(Command::new(SECURITY).args(&args), None)?
            }
        };
        match output.code {
            0 => FileStore.delete(slot),
            EXIT_LOCKED => Err(locked()),
            _ => Err(failed("write")),
        }
    }

    fn delete(&self, slot: &Slot) -> Result<(), AppError> {
        let output = run(
            Command::new(SECURITY).args([
                "delete-generic-password",
                "-a",
                &layout::keychain_account(),
                "-s",
                &slot.keychain_service(),
            ]),
            None,
        )?;
        match output.code {
            0 | EXIT_NOT_FOUND => FileStore.delete(slot),
            EXIT_LOCKED => Err(locked()),
            _ => Err(failed("delete")),
        }
    }
}

/// 写一条 `add-generic-password` 的方式：放得进 `security -i` 一行就经 stdin，否则经命令行参数。
/// 里面有凭据的 hex，只在测试里能打印。
#[cfg_attr(test, derive(Debug, PartialEq))]
enum AddInvocation {
    Stdin(String),
    Args(Vec<String>),
}

fn add_invocation(account: &str, service: &str, hex: &str) -> AddInvocation {
    let line = format!("add-generic-password -U -a \"{account}\" -s \"{service}\" -X \"{hex}\"\n");
    if line.len() <= MAX_LINE {
        return AddInvocation::Stdin(line);
    }
    AddInvocation::Args(
        [
            "add-generic-password",
            "-U",
            "-a",
            account,
            "-s",
            service,
            "-X",
            hex,
        ]
        .map(String::from)
        .to_vec(),
    )
}

struct Output {
    code: i32,
    stdout: Vec<u8>,
}

/// 跑一次 `security`，最多等 [`TIMEOUT`]。stderr 丢掉（可能回显内容）。
fn run(command: &mut Command, stdin: Option<&[u8]>) -> Result<Output, AppError> {
    let mut child = command
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| failed("spawn"))?;
    if let Some(input) = stdin {
        let mut pipe = child.stdin.take().ok_or_else(|| failed("stdin"))?;
        pipe.write_all(input).map_err(|_| failed("stdin"))?;
        // 关掉 stdin，`security -i` 读到 EOF 才退出
        drop(pipe);
    }
    let mut stdout = child.stdout.take().ok_or_else(|| failed("stdout"))?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < TIMEOUT => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AppError::localized(
                    "claude.snapshot.keychain_timeout",
                    "访问钥匙串超时（可能在等你确认弹窗），没有切换",
                    "Keychain access timed out (it may be waiting for you to confirm a prompt), so nothing was switched.",
                ));
            }
        }
    };
    let stdout = reader.join().unwrap_or_default();
    Ok(Output {
        code: status.code().unwrap_or(-1),
        stdout,
    })
}

fn locked() -> AppError {
    AppError::localized(
        "claude.snapshot.keychain_locked",
        "钥匙串已锁定，请解锁后再切换",
        "The keychain is locked. Unlock it, then switch again.",
    )
}

/// 不带输出内容：`security` 的输出里可能有凭据。
fn failed(stage: &str) -> AppError {
    AppError::Message(format!("访问钥匙串失败（{stage}）"))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex_decode(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.len() % 2 != 0 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let bytes = br#"{"a":"b"}"#;
        assert_eq!(hex_decode(&hex_encode(bytes)).unwrap(), bytes);
        assert!(hex_decode("xyz").is_none());
        assert!(hex_decode("abc").is_none());
    }

    #[test]
    fn small_payload_goes_through_stdin() {
        let hex = hex_encode(br#"{"claudeAiOauth":{}}"#);
        match add_invocation("me", "Claude Code-credentials", &hex) {
            AddInvocation::Stdin(line) => {
                assert!(line.len() <= MAX_LINE);
                assert!(line.ends_with(&format!("-X \"{hex}\"\n")));
            }
            other => panic!("expected stdin, got {other:?}"),
        }
    }

    /// #9002：连了 OAuth 类 MCP 服务器后凭据 JSON 有 3 KB 左右，hex 后超出 `security -i` 一行，
    /// 要和 Claude Code 一样改经命令行参数写，而不是拒绝切换。
    #[test]
    fn large_payload_falls_back_to_args() {
        let json = format!(r#"{{"mcpOAuth":{{"t":"{}"}}}}"#, "x".repeat(3100));
        let hex = hex_encode(json.as_bytes());
        assert_eq!(
            add_invocation("me", "Claude Code-credentials", &hex),
            AddInvocation::Args(
                [
                    "add-generic-password",
                    "-U",
                    "-a",
                    "me",
                    "-s",
                    "Claude Code-credentials",
                    "-X",
                    &hex,
                ]
                .map(String::from)
                .to_vec()
            )
        );
    }
}
