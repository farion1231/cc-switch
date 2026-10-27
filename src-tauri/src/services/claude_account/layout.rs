//! 账号目录的布局：`<app_config_dir>/claude-accounts/<local_id>/`。
//!
//! - `config/` 是这个账号的 `CLAUDE_CONFIG_DIR`，里面的东西全归 Claude Code（凭据、
//!   `.claude.json`、会话缓存……），CC Switch 只建目录、接共享层、删目录。
//! - `.open-url`（macOS / Linux）是登录时交给 Claude Code 的 `BROWSER`：只把要打开的地址
//!   写进同目录的 `.opened-url`，由 CC Switch 的窗口显示、打开。
//!
//! macOS 上 Claude Code 把登录存在钥匙串 `Claude Code-credentials-<后缀>`，后缀是
//! `CLAUDE_CONFIG_DIR` 原字符串（NFC）的 sha256 前 8 位。删账号时按同样的算法找到它。

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::AppError;

const ROOT_DIRNAME: &str = "claude-accounts";
const CONFIG_DIRNAME: &str = "config";
#[cfg(unix)]
pub const OPENER_FILENAME: &str = ".open-url";
pub const OPENED_URL_FILENAME: &str = ".opened-url";

/// 所有账号目录的根。
pub fn accounts_root(data_dir: &Path) -> PathBuf {
    data_dir.join(ROOT_DIRNAME)
}

/// 一个账号的外层目录（放 `config/` 和登录用的小文件）。
pub fn account_home(data_dir: &Path, local_id: &str) -> PathBuf {
    accounts_root(data_dir).join(local_id)
}

/// 一个账号的 `CLAUDE_CONFIG_DIR`。
pub fn config_dir(data_dir: &Path, local_id: &str) -> PathBuf {
    account_home(data_dir, local_id).join(CONFIG_DIRNAME)
}

/// 新账号的本机 id：32 位小写 hex，只用作目录名。
pub fn new_local_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// `local_id` 只能是 [`new_local_id`] 生成的样子，防止账号文件被改坏后拼出 `..` 之类的路径。
pub fn valid_local_id(local_id: &str) -> bool {
    local_id.len() == 32
        && local_id
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// 交给 Claude Code 之前的最后一道检查：必须是账号根下的绝对路径，且不是用户自己的
/// `~/.claude`（那是「默认登录」，CC Switch 绝不把它当账号目录）。
pub fn assert_config_dir(data_dir: &Path, dir: &Path) -> Result<(), AppError> {
    let text = dir.to_string_lossy();
    if text.trim().is_empty() || !dir.is_absolute() {
        return Err(AppError::Message(format!(
            "账号目录必须是绝对路径: {}",
            dir.display()
        )));
    }
    if dir
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(AppError::Message(format!(
            "账号目录不能包含 `..`: {}",
            dir.display()
        )));
    }
    if !dir.starts_with(accounts_root(data_dir)) {
        return Err(AppError::Message(format!(
            "账号目录不在 CC Switch 的账号根下: {}",
            dir.display()
        )));
    }
    let default_dir = crate::config::get_home_dir().join(".claude");
    if same_path(dir, &default_dir) || same_path(dir, &crate::config::get_claude_config_dir()) {
        return Err(AppError::Message(
            "不能把默认的 Claude 配置目录当作账号目录".to_string(),
        ));
    }
    Ok(())
}

/// 规范化后比较（目录不存在时按原样比较）；大小写不敏感的文件系统上也按不敏感比。
fn same_path(a: &Path, b: &Path) -> bool {
    let a = std::fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf());
    let b = std::fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf());
    if cfg!(any(target_os = "macos", windows)) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// `value` 是不是某个账号目录（CC Switch 写进 `settings.json` 的 `CLAUDE_CONFIG_DIR`）。
/// 用来判断 live 里的这个值归不归 CC Switch：不是的话一律不碰。
pub fn is_account_config_dir(data_dir: &Path, value: &str) -> bool {
    let path = Path::new(value.trim());
    let Ok(rest) = path.strip_prefix(accounts_root(data_dir)) else {
        return false;
    };
    let mut parts = rest.components();
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(id), Some(config), None)
            if valid_local_id(&id.as_os_str().to_string_lossy())
                && config.as_os_str() == CONFIG_DIRNAME
    )
}

/// Claude Code 在 macOS 钥匙串里存这个目录登录的 service 名。
///
/// 和 Claude Code 一致：对传给它的 `CLAUDE_CONFIG_DIR` 原字符串做 NFC，再取 sha256 的前
/// 8 位 hex。这里的目录都由 CC Switch 生成（`<数据目录>/claude-accounts/<hex>/config`），
/// 数据目录是纯 ASCII 时 NFC 不改变任何字节；含非 ASCII 字符时按原样计算，可能对不上，
/// 只影响删账号时能不能顺手删掉钥匙串里的条目。
pub fn keychain_service(config_dir: &Path) -> String {
    let digest = Sha256::digest(config_dir.to_string_lossy().as_bytes());
    let suffix: String = digest
        .iter()
        .take(4)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("Claude Code-credentials-{suffix}")
}

/// Claude Code 存钥匙串时用的 account：`$USER`（取不到用系统用户名），不合规则时回落
/// `claude-code-user`。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn keychain_account() -> String {
    let user = std::env::var("USER")
        .ok()
        .filter(|user| !user.is_empty())
        .or_else(|| {
            crate::config::get_home_dir()
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    let valid = !user.is_empty()
        && user
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if valid {
        user
    } else {
        "claude-code-user".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keychain_service_matches_claude_code() {
        // Claude Code 2.1.309：sha256("/tmp/cc") 的前 8 位
        assert_eq!(
            keychain_service(Path::new("/tmp/cc")),
            format!(
                "Claude Code-credentials-{}",
                &format!("{:x}", Sha256::digest(b"/tmp/cc"))[..8]
            )
        );
        // 原字符串参与计算：尾斜杠不同，条目不同
        assert_ne!(
            keychain_service(Path::new("/tmp/cc")),
            keychain_service(Path::new("/tmp/cc/"))
        );
    }

    #[test]
    fn local_id_shape() {
        let id = new_local_id();
        assert!(valid_local_id(&id));
        assert!(!valid_local_id("../../etc"));
        assert!(!valid_local_id("ABCDEF0123456789abcdef0123456789"));
    }

    #[test]
    fn recognizes_only_account_config_dirs() {
        let data = Path::new("/data/.cc-switch");
        let id = "0123456789abcdef0123456789abcdef";
        let dir = config_dir(data, id);
        assert!(is_account_config_dir(data, &dir.to_string_lossy()));
        assert!(!is_account_config_dir(data, "/home/me/.claude"));
        assert!(!is_account_config_dir(
            data,
            &account_home(data, id).to_string_lossy()
        ));
        assert!(!is_account_config_dir(
            data,
            &dir.join("nested").to_string_lossy()
        ));
        assert!(!is_account_config_dir(
            data,
            &accounts_root(data)
                .join("not-hex")
                .join("config")
                .to_string_lossy()
        ));
    }

    #[test]
    fn rejects_unsafe_config_dirs() {
        let data = std::env::temp_dir().join("cc-switch-layout-test");
        let data = data.as_path();
        let good = config_dir(data, "0123456789abcdef0123456789abcdef");
        assert!(assert_config_dir(data, &good).is_ok());
        assert!(assert_config_dir(data, Path::new("relative/config")).is_err());
        assert!(assert_config_dir(data, Path::new("")).is_err());
        assert!(assert_config_dir(data, &accounts_root(data).join("..").join("x")).is_err());
        assert!(assert_config_dir(data, &std::env::temp_dir().join("elsewhere")).is_err());
    }
}
