//! agy 官方登录在系统钥匙串里的那一条：service `gemini`、account `antigravity`。
//!
//! agy 用 zalando/go-keyring 读写它；macOS 上 go-keyring 自己就是调 `/usr/bin/security`，
//! 条目的访问控制里信任的也是这个程序，所以这里同样经它读写，不会弹授权框（10-21 真机实测）。
//! macOS 上的值是 `go-keyring-base64:` 加 base64 的 JSON：
//! `{token{access_token,token_type,refresh_token,expiry,expires_in},auth_method,id_token}`。
//! Windows 上 go-keyring 存在「凭据管理器」里：普通凭据，目标名 `gemini:antigravity`，
//! 用户名 `antigravity`，值是裸 JSON 的 UTF-8 字节，最长 2560 字节（系统上限，go-keyring 也这么拦）。
//! Linux 用 D-Bus Secret Service，条目按 `service` 和 `username` 匹配，值是裸 JSON。
//! agy 1.5 起自己写入时不再带 `id_token`，改成 `id_token_email`（可能带 `writer_id`，1.5.2 有、1.5.3 已去掉），
//! 认账号的办法见 `AgyAccountManager::read_live`。旧格式 agy 1.5 照样能读（切过去的号 agy
//! 续期后会改写成新格式），这里不转换，原样读写。
//!
//! 只做切换要用的两件事：读出整条、整条写入。不删除条目，也不碰 `~/.gemini` 下的文件
//! （`oauth_creds.json` 那些是 Gemini CLI 的登录，agy 1.4.3 切号只看钥匙串）。

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::Value;

use crate::error::AppError;

#[cfg(target_os = "linux")]
mod linux;

const GO_KEYRING_PREFIX: &str = "go-keyring-base64:";

/// 钥匙串里那一条的读写。测试换成内存实现，别碰真实钥匙串。
pub trait AgyLoginStore: Send + Sync {
    /// 当前登录的原始值；没登录（条目不存在）时为 `None`。
    fn read(&self) -> Result<Option<String>, AppError>;
    /// 用户主动切号时，允许系统提示解锁钥匙串。
    fn read_for_switch(&self) -> Result<Option<String>, AppError> {
        self.read()
    }
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

/// 编码成本平台上 go-keyring 写入的格式。
pub fn encode(credential: &Value) -> Result<String, AppError> {
    if cfg!(target_os = "linux") {
        serde_json::to_string(credential).map_err(|source| AppError::JsonSerialize { source })
    } else if cfg!(windows) {
        encode_bare(credential)
    } else {
        encode_base64(credential)
    }
}

/// macOS 上的格式：`go-keyring-base64:` 加 base64 的 JSON。
fn encode_base64(credential: &Value) -> Result<String, AppError> {
    let json =
        serde_json::to_string(credential).map_err(|source| AppError::JsonSerialize { source })?;
    Ok(format!("{GO_KEYRING_PREFIX}{}", STANDARD.encode(json)))
}

/// Windows 凭据管理器一条凭据的值最长 2560 字节。
const WINCRED_BLOB_LIMIT: usize = 2560;

/// Windows 上的格式：裸 JSON。超出凭据上限时去掉 `id_token`、改记 `id_token_email`，
/// 也就是 agy 1.5 起自己写入的样子；认账号靠 refresh token 和邮箱，不靠 id_token。
fn encode_bare(credential: &Value) -> Result<String, AppError> {
    let to_json = |value: &Value| {
        serde_json::to_string(value).map_err(|source| AppError::JsonSerialize { source })
    };
    let json = to_json(credential)?;
    if json.len() <= WINCRED_BLOB_LIMIT {
        return Ok(json);
    }
    let mut slim = credential.clone();
    if let Some(object) = slim.as_object_mut() {
        if let Some(id_token) = object.remove("id_token") {
            let email = id_token
                .as_str()
                .and_then(super::identity::identity)
                .and_then(|claims| claims.email);
            if let (Some(email), false) = (email, object.contains_key("id_token_email")) {
                object.insert("id_token_email".into(), Value::String(email));
            }
        }
    }
    let json = to_json(&slim)?;
    if json.len() <= WINCRED_BLOB_LIMIT {
        return Ok(json);
    }
    Err(AppError::localized(
        "agy.account.credential_too_large",
        "这个账号的登录数据超过了 Windows 凭据管理器的长度上限，没有切换",
        "This account's sign-in data exceeds the Windows Credential Manager size limit; agy was not switched.",
    ))
}

/// 本机钥匙串：macOS Keychain、Windows Credential Manager 或 Linux Secret Service。
#[cfg_attr(
    not(any(target_os = "macos", windows, target_os = "linux")),
    allow(dead_code)
)]
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
    #[cfg(all(test, any(target_os = "macos", windows)))]
    pub fn named(service: &'static str, account: &'static str) -> Self {
        Self { service, account }
    }
}

pub fn supported() -> bool {
    cfg!(any(target_os = "macos", windows, target_os = "linux"))
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

#[cfg(windows)]
mod wincred {
    use std::ptr;

    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND};
    use windows_sys::Win32::Security::Credentials::{
        CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    use crate::error::AppError;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// go-keyring 的目标名：`service:account`。
    fn target_name(service: &str, account: &str) -> Vec<u16> {
        wide(&format!("{service}:{account}"))
    }

    pub fn read(service: &str, account: &str) -> Result<Option<String>, AppError> {
        let target = target_name(service, account);
        let mut credential: *mut CREDENTIALW = ptr::null_mut();
        // SAFETY: target 以 0 结尾、在调用期间存活；成功时 credential 指向系统分配的结构，下面用完即 CredFree。
        let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) };
        if ok == 0 {
            // SAFETY: 紧跟在失败的系统调用之后取错误码。
            let code = unsafe { GetLastError() };
            if code == ERROR_NOT_FOUND {
                return Ok(None);
            }
            return Err(AppError::Config(format!(
                "读取 Antigravity 登录失败（Windows 错误码 {code}）"
            )));
        }
        // SAFETY: CredReadW 成功，credential 非空；blob 长度由系统给出，复制出来后再释放。
        let bytes = unsafe {
            let blob = &*credential;
            let bytes = if blob.CredentialBlob.is_null() || blob.CredentialBlobSize == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(blob.CredentialBlob, blob.CredentialBlobSize as usize)
                    .to_vec()
            };
            CredFree(credential as *const core::ffi::c_void);
            bytes
        };
        let value = String::from_utf8(bytes)
            .map_err(|_| AppError::Config("Antigravity 登录数据不是 UTF-8".into()))?;
        Ok((!value.is_empty()).then_some(value))
    }

    /// 和 go-keyring 一样整条覆盖：普通凭据、本机持久、用户名就是 account。
    pub fn write(service: &str, account: &str, value: &str) -> Result<(), AppError> {
        let blob = value.as_bytes();
        if blob.is_empty() || blob.len() > super::WINCRED_BLOB_LIMIT {
            return Err(AppError::InvalidInput(
                "Antigravity 登录数据的长度超出 Windows 凭据管理器的上限".into(),
            ));
        }
        let mut target = target_name(service, account);
        let mut user = wide(account);
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_ptr() as *mut u8,
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: user.as_mut_ptr(),
            ..Default::default()
        };
        // SAFETY: 结构里的指针都指向本函数里存活的缓冲区；CredWriteW 只读不写它们。
        let ok = unsafe { CredWriteW(&credential, 0) };
        if ok == 0 {
            // SAFETY: 紧跟在失败的系统调用之后取错误码。
            let code = unsafe { GetLastError() };
            return Err(AppError::Config(format!(
                "写入 Antigravity 登录失败（Windows 错误码 {code}）"
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
        #[cfg(windows)]
        {
            wincred::read(self.service, self.account)
        }
        #[cfg(target_os = "linux")]
        {
            linux::read(self.service, self.account, false)
        }
        #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
        {
            Err(unsupported_error())
        }
    }

    #[cfg(target_os = "linux")]
    fn read_for_switch(&self) -> Result<Option<String>, AppError> {
        linux::read(self.service, self.account, true)
    }

    fn write(&self, value: &str) -> Result<(), AppError> {
        #[cfg(target_os = "macos")]
        {
            macos::write(self.service, self.account, value)
        }
        #[cfg(windows)]
        {
            wincred::write(self.service, self.account, value)
        }
        #[cfg(target_os = "linux")]
        {
            linux::write(self.service, self.account, value)
        }
        #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
        {
            let _ = value;
            Err(unsupported_error())
        }
    }
}

pub fn unsupported_error() -> AppError {
    AppError::localized(
        "agy.account.unsupported_platform",
        "切换 Antigravity 账号目前只支持 macOS、Windows 和 Linux",
        "Switching Antigravity accounts is only supported on macOS, Windows, and Linux",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trips_the_go_keyring_format() {
        let credential = json!({"token": {"access_token": "a", "refresh_token": "r"}, "auth_method": "consumer", "id_token": "i"});
        let raw = encode_base64(&credential).unwrap();
        assert!(raw.starts_with(GO_KEYRING_PREFIX));
        assert_eq!(decode(&raw).unwrap(), credential);
        let bare = encode_bare(&credential).unwrap();
        assert!(!bare.starts_with(GO_KEYRING_PREFIX));
        assert_eq!(decode(&bare).unwrap(), credential);
    }

    fn fake_id_token(email: &str, padding: usize) -> String {
        let claims = json!({"sub": "1", "email": email, "pad": "x".repeat(padding)});
        format!(
            "h.{}.s",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string())
        )
    }

    #[test]
    fn bare_format_drops_an_oversized_id_token_for_its_email() {
        let small = json!({"token": {"refresh_token": "r"}, "id_token": fake_id_token("a@example.com", 10)});
        assert_eq!(decode(&encode_bare(&small).unwrap()).unwrap(), small);

        let big = json!({"token": {"refresh_token": "r"}, "id_token": fake_id_token("a@example.com", 3000)});
        let slim = decode(&encode_bare(&big).unwrap()).unwrap();
        assert!(slim.get("id_token").is_none());
        assert_eq!(slim["id_token_email"], "a@example.com");
        assert_eq!(slim["token"]["refresh_token"], "r");

        let hopeless = json!({"token": {"refresh_token": "x".repeat(3000)}});
        assert!(encode_bare(&hopeless).is_err());
    }

    #[test]
    fn accepts_bare_json_and_rejects_garbage() {
        assert_eq!(decode(r#"{"token":{}}"#).unwrap(), json!({"token": {}}));
        assert!(decode("go-keyring-base64:!!!").is_err());
        assert!(decode("[1,2]").is_err());
        assert!(decode("not json").is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_writes_bare_json_without_the_windows_size_limit() {
        let credential = json!({"token": {"refresh_token": "x".repeat(3000)}});
        let encoded = encode(&credential).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&encoded).unwrap(), credential);
    }
}
