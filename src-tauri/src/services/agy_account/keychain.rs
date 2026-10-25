//! agy 官方登录在系统钥匙串里的那一条：service `gemini`、account `antigravity`。
//!
//! agy 用 zalando/go-keyring 读写它；macOS 上 go-keyring 自己就是调 `/usr/bin/security`，
//! 条目的访问控制里信任的也是这个程序，所以这里同样经它读写，不会弹授权框（10-21 真机实测）。
//! 值是 `go-keyring-base64:` 加 base64 的 JSON：
//! `{token{access_token,token_type,refresh_token,expiry,expires_in},auth_method,id_token}`。
//!
//! 只做切换要用的两件事：读出整条、整条写入。不删除条目，也不碰 `~/.gemini` 下的文件
//! （`oauth_creds.json` 那些是 Gemini CLI 的登录，agy 1.4.3 切号只看钥匙串）。

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::Value;

use crate::error::AppError;

const GO_KEYRING_PREFIX: &str = "go-keyring-base64:";

/// 钥匙串里那一条的读写。测试换成内存实现，别碰真实钥匙串。
pub trait AgyLoginStore: Send + Sync {
    /// 当前登录的原始值；没登录（条目不存在）时为 `None`。
    fn read(&self) -> Result<Option<String>, AppError>;
    /// 整条覆盖写入（条目不存在时新建）。
    fn write(&self, value: &str) -> Result<(), AppError>;
}

/// 按 go-keyring 的格式解出登录 JSON；没有前缀的当成裸 JSON（Windows / Linux 上就是这样存的）。
pub fn decode(raw: &str) -> Result<Value, AppError> {
    let raw = raw.trim();
    let json = match raw.strip_prefix(GO_KEYRING_PREFIX) {
        Some(encoded) => {
            let bytes = STANDARD
                .decode(encoded)
                .map_err(|_| AppError::Config("Antigravity 登录数据不是有效的 base64".into()))?;
            String::from_utf8(bytes)
                .map_err(|_| AppError::Config("Antigravity 登录数据不是 UTF-8".into()))?
        }
        None => raw.to_string(),
    };
    let value: Value = serde_json::from_str(&json)
        .map_err(|_| AppError::Config("Antigravity 登录数据不是 JSON".into()))?;
    if !value.is_object() {
        return Err(AppError::Config(
            "Antigravity 登录数据不是 JSON 对象".into(),
        ));
    }
    Ok(value)
}

/// 编码成 macOS 上 go-keyring 写入的格式。
pub fn encode(credential: &Value) -> Result<String, AppError> {
    let json =
        serde_json::to_string(credential).map_err(|source| AppError::JsonSerialize { source })?;
    Ok(format!("{GO_KEYRING_PREFIX}{}", STANDARD.encode(json)))
}

/// 本机钥匙串。目前只实现了 macOS；其他平台读写都报不支持。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct SystemKeychain {
    service: &'static str,
    account: &'static str,
}

impl SystemKeychain {
    /// agy 自己的那一条。
    pub fn agy() -> Self {
        Self {
            service: "gemini",
            account: "antigravity",
        }
    }

    /// 真机测试用的另一条，别动 agy 那条。
    #[cfg(test)]
    pub fn named(service: &'static str, account: &'static str) -> Self {
        Self { service, account }
    }
}

pub fn supported() -> bool {
    cfg!(target_os = "macos")
}

#[cfg(target_os = "macos")]
mod macos {
    use std::io::Write;
    use std::process::{Command, Stdio};

    use crate::error::AppError;

    const SECURITY: &str = "/usr/bin/security";
    /// `security find-generic-password` 找不到条目时的退出码（errSecItemNotFound）。
    const NOT_FOUND_EXIT: i32 = 44;
    /// `security -i` 的命令行缓冲是 4096 字节，留点余量。
    const INTERACTIVE_LINE_LIMIT: usize = 4000;

    pub fn read(service: &str, account: &str) -> Result<Option<String>, AppError> {
        let output = Command::new(SECURITY)
            .args(["find-generic-password", "-s", service, "-a", account, "-w"])
            .stdin(Stdio::null())
            .output()
            .map_err(|source| AppError::IoContext {
                context: "读取 Antigravity 登录失败".into(),
                source,
            })?;
        if output.status.code() == Some(NOT_FOUND_EXIT) {
            return Ok(None);
        }
        if !output.status.success() {
            return Err(AppError::Config(format!(
                "读取 Antigravity 登录失败（security 退出码 {:?}）",
                output.status.code()
            )));
        }
        let value = String::from_utf8(output.stdout)
            .map_err(|_| AppError::Config("Antigravity 登录数据不是 UTF-8".into()))?;
        let value = value.trim_end_matches(['\r', '\n']).to_string();
        Ok((!value.is_empty()).then_some(value))
    }

    /// `-U` 原地更新，保留条目原有的访问控制（不删再建、不加 `-A`）。值经 `security -i` 的标准
    /// 输入传，不出现在进程参数里；只有超出交互缓冲时才退回命令行参数。
    pub fn write(service: &str, account: &str, value: &str) -> Result<(), AppError> {
        // go-keyring 格式只含 base64 字符，引号里放得下；别的值不写，免得拼坏命令。
        if value.is_empty()
            || !value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | ':' | '-'))
        {
            return Err(AppError::InvalidInput(
                "Antigravity 登录数据含有不能写入钥匙串的字符".into(),
            ));
        }
        let line = format!("add-generic-password -U -s {service} -a {account} -w \"{value}\"\n");
        let output = if line.len() <= INTERACTIVE_LINE_LIMIT {
            let mut child = Command::new(SECURITY)
                .arg("-i")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|source| AppError::IoContext {
                    context: "写入 Antigravity 登录失败".into(),
                    source,
                })?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(line.as_bytes())
                    .map_err(|source| AppError::IoContext {
                        context: "写入 Antigravity 登录失败".into(),
                        source,
                    })?;
            }
            child.wait_with_output()
        } else {
            log::warn!("[AgyAccount] 登录数据超出 security -i 缓冲，改经命令行参数写入");
            Command::new(SECURITY)
                .args([
                    "add-generic-password",
                    "-U",
                    "-s",
                    service,
                    "-a",
                    account,
                    "-w",
                    value,
                ])
                .stdin(Stdio::null())
                .output()
        }
        .map_err(|source| AppError::IoContext {
            context: "写入 Antigravity 登录失败".into(),
            source,
        })?;
        // `security -i` 遇到错误也可能退出 0，错误只写在 stderr
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() || !stderr.trim().is_empty() {
            return Err(AppError::Config(format!(
                "写入 Antigravity 登录失败（security 退出码 {:?}）",
                output.status.code()
            )));
        }
        Ok(())
    }
}

impl AgyLoginStore for SystemKeychain {
    fn read(&self) -> Result<Option<String>, AppError> {
        #[cfg(target_os = "macos")]
        {
            macos::read(self.service, self.account)
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(unsupported_error())
        }
    }

    fn write(&self, value: &str) -> Result<(), AppError> {
        #[cfg(target_os = "macos")]
        {
            macos::write(self.service, self.account, value)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = value;
            Err(unsupported_error())
        }
    }
}

pub fn unsupported_error() -> AppError {
    AppError::localized(
        "agy.account.unsupported_platform",
        "切换 Antigravity 账号目前只支持 macOS",
        "Switching Antigravity accounts is only supported on macOS for now",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trips_the_go_keyring_format() {
        let credential = json!({"token": {"access_token": "a", "refresh_token": "r"}, "auth_method": "consumer", "id_token": "i"});
        let raw = encode(&credential).unwrap();
        assert!(raw.starts_with(GO_KEYRING_PREFIX));
        assert_eq!(decode(&raw).unwrap(), credential);
    }

    #[test]
    fn accepts_bare_json_and_rejects_garbage() {
        assert_eq!(decode(r#"{"token":{}}"#).unwrap(), json!({"token": {}}));
        assert!(decode("go-keyring-base64:!!!").is_err());
        assert!(decode("[1,2]").is_err());
        assert!(decode("not json").is_err());
    }
}
