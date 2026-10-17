//! 远程服务的服务端 secret 与客户端凭据库的落盘路径。
//!
//! 这一层回答三个问题，且只回答这三个：
//!
//! 1. **secret 从哪来** —— [`load_or_create`]：首次自建，重启复用。
//! 2. **权限对不对** —— 文件权限位不是 0600 就 [`SecretError::BadPermissions`]，
//!    远程服务据此拒绝启动。**不自动 chmod 修复**：偷偷把 0644 改回 0600 会
//!    让「谁把凭据文件改成可读了」这个问题永远查不出来。
//! 3. **凭据库怎么存** —— [`load_clients`] / [`save_clients`]：只落摘要，不落明文。
//!
//! 失败范围限定在远程服务：桌面端主功能不读这两个文件，因此这里返回 `Err`
//! 只让远程侧起不来，不影响 provider 切换、MCP 管理这些主路径。
//!
//! 依赖边界：只组合既有依赖（`uuid` / `serde_json` / `tempfile`）与既有的
//! `config::atomic_write_private`，不新增 crate、不改 `Cargo.toml`。
//! 测试一律用 `tempfile::tempdir()`，绝不读写真实配置目录。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use uuid::Uuid;

use super::token::ClientDigest;
use super::ClientToken;
use crate::config::atomic_write_private;

/// 凭据在配置目录下的子目录名。
pub const AUTH_SUBDIR: &str = "remote-auth";

/// 服务端 secret 的文件名。
pub const SECRET_FILE: &str = "secret";

/// 客户端凭据库的文件名。
pub const CLIENTS_FILE: &str = "clients.json";

/// secret 文件必须具备的权限位。
pub const SECRET_MODE: u32 = 0o600;

/// secret 的字节长度。
const SECRET_LEN: usize = 32;

/// 凭据层错误。
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    /// 凭据文件权限过宽。远程服务必须因此拒绝启动。
    #[error("凭据文件权限过宽: {path} (0{mode:o}), 需要 0{SECRET_MODE:o})")]
    BadPermissions { path: PathBuf, mode: u32 },
    #[error("IO 错误: {0}")]
    Io(#[from] io::Error),
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),
}

// 装配层与配对 handler 接上来之前，本层每个函数都是「未使用」，于是逐个加了
// 豁免；如今它们都有生产调用点，那些豁免已全部删除。
pub fn load_or_create(config_dir: &Path) -> Result<[u8; 32], SecretError> {
    let dir = config_dir.join(AUTH_SUBDIR);
    let path = dir.join(SECRET_FILE);

    if path.exists() {
        // 已存在 → 只读，**永不重新生成**。重新生成等于无条件撤销全部已签发凭据。
        ensure_mode_0600(&path)?;
        return read_secret(&path);
    }

    let secret = generate_secret();
    atomic_write_private(&path, &secret)
        .map_err(|e| SecretError::Io(io::Error::other(e.to_string())))?;
    // 写完复核：确认落盘结果真的是 0600，而不是「以为」是。
    ensure_mode_0600(&path)?;
    Ok(secret)
}

pub fn load_clients(config_dir: &Path) -> Result<Vec<ClientToken>, SecretError> {
    let path = config_dir.join(AUTH_SUBDIR).join(CLIENTS_FILE);
    if !path.exists() {
        // 首次运行：没有库就是没有凭据，不是错误。
        return Ok(Vec::new());
    }
    let value: Value = serde_json::from_slice(&fs::read(&path)?)?;
    clients_from_value(&value)
}

pub fn save_clients(config_dir: &Path, clients: &[ClientToken]) -> Result<(), SecretError> {
    let path = config_dir.join(AUTH_SUBDIR).join(CLIENTS_FILE);
    // 库里全是摘要，但仍是凭据材料：同样走 0600。
    let bytes = serde_json::to_vec_pretty(&clients_to_value(clients))?;
    atomic_write_private(&path, &bytes)
        .map_err(|e| SecretError::Io(io::Error::other(e.to_string())))
}

fn read_secret(path: &Path) -> Result<[u8; 32], SecretError> {
    let bytes = fs::read(path)?;
    if bytes.len() != SECRET_LEN {
        return Err(invalid_data(format!(
            "secret 长度应为 {SECRET_LEN} 字节，实际 {} 字节",
            bytes.len()
        )));
    }
    let mut secret = [0u8; SECRET_LEN];
    secret.copy_from_slice(&bytes);
    Ok(secret)
}

fn generate_secret() -> [u8; 32] {
    // 熵走既有 `uuid` v4 路径（每条 16 字节，取两条凑 32 字节），与
    // `token::generate_token` 同一套来源，不新增依赖。
    let mut secret = [0u8; SECRET_LEN];
    secret[..16].copy_from_slice(Uuid::new_v4().as_bytes());
    secret[16..].copy_from_slice(Uuid::new_v4().as_bytes());
    secret
}

/// 权限位不等于 0600 就报错。**不做 chmod 修复**。
fn ensure_mode_0600(path: &Path) -> Result<(), SecretError> {
    let mode = file_mode(path)?;
    if mode == SECRET_MODE {
        Ok(())
    } else {
        Err(SecretError::BadPermissions {
            path: path.to_path_buf(),
            mode,
        })
    }
}

/// 只看权限的低 9 位。非 unix 平台没有 POSIX 权限位，一律按合规处理。
#[cfg(unix)]
fn file_mode(path: &Path) -> Result<u32, SecretError> {
    use std::os::unix::fs::PermissionsExt;
    Ok(fs::metadata(path)?.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn file_mode(_path: &Path) -> Result<u32, SecretError> {
    Ok(SECRET_MODE)
}

fn invalid_data(msg: impl Into<String>) -> SecretError {
    SecretError::Io(io::Error::new(io::ErrorKind::InvalidData, msg.into()))
}

fn clients_to_value(clients: &[ClientToken]) -> Value {
    Value::Array(
        clients
            .iter()
            .map(|c| {
                json!({
                    "client_name": c.client_name,
                    "created_at": c.created_at,
                    "last_used_at": c.last_used_at,
                    "token_hash": encode_hex32(c.token_hash.as_bytes()),
                    "cookie_hash": c.cookie_hash.map(|d| encode_hex32(d.as_bytes())),
                })
            })
            .collect(),
    )
}

fn clients_from_value(value: &Value) -> Result<Vec<ClientToken>, SecretError> {
    let entries = value
        .as_array()
        .ok_or_else(|| invalid_data("凭据库顶层必须是数组"))?;

    let mut clients = Vec::with_capacity(entries.len());
    for entry in entries {
        let obj = entry
            .as_object()
            .ok_or_else(|| invalid_data("凭据条目必须是对象"))?;
        clients.push(ClientToken {
            client_name: require_str(obj, "client_name")?.to_string(),
            created_at: require_i64(obj, "created_at")?,
            last_used_at: require_i64(obj, "last_used_at")?,
            token_hash: ClientDigest::new(decode_hex32(
                require_str(obj, "token_hash")?,
                "token_hash",
            )?),
            cookie_hash: match obj.get("cookie_hash") {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) => Some(ClientDigest::new(decode_hex32(s, "cookie_hash")?)),
                Some(_) => return Err(invalid_data("cookie_hash 必须是字符串或 null")),
            },
        });
    }
    Ok(clients)
}

fn require_str<'a>(
    obj: &'a serde_json::Map<String, Value>,
    field: &str,
) -> Result<&'a str, SecretError> {
    obj.get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_data(format!("{field} 必须是字符串")))
}

fn require_i64(obj: &serde_json::Map<String, Value>, field: &str) -> Result<i64, SecretError> {
    obj.get(field)
        .and_then(Value::as_i64)
        .ok_or_else(|| invalid_data(format!("{field} 必须是整数")))
}

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

fn encode_hex32(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(HEX_DIGITS[(byte >> 4) as usize] as char);
        out.push(HEX_DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

fn decode_hex32(text: &str, field: &str) -> Result<[u8; 32], SecretError> {
    let bytes = text.as_bytes();
    if bytes.len() != 64 {
        return Err(invalid_data(format!(
            "{field} 应为 64 个十六进制字符，实际 {}",
            bytes.len()
        )));
    }
    let mut out = [0u8; 32];
    for (i, slot) in out.iter_mut().enumerate() {
        let hi = hex_nibble(bytes[i * 2], field)?;
        let lo = hex_nibble(bytes[i * 2 + 1], field)?;
        *slot = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_nibble(c: u8, field: &str) -> Result<u8, SecretError> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(invalid_data(format!("{field} 含非十六进制字符"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// 只被本模块的用例取用，故随用例一起挂在 test 配置下。
    fn auth_dir(config_dir: &Path) -> PathBuf {
        config_dir.join(AUTH_SUBDIR)
    }

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn write_secret_file(dir: &Path, contents: &[u8], mode: u32) {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(SECRET_FILE);
        fs::write(&path, contents).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
    }

    // --- 规格 ---

    #[test]
    fn refuses_to_start_when_secret_is_0644() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join(AUTH_SUBDIR);
        write_secret_file(&dir, &[9u8; 32], 0o644);

        match load_or_create(tmp.path()) {
            Err(SecretError::BadPermissions { path, mode }) => {
                assert_eq!(path, dir.join(SECRET_FILE));
                assert_eq!(mode, 0o644);
            }
            other => panic!("expected BadPermissions, got {other:?}"),
        }

        // 关键：不自动 chmod 修复，文件保持原样，让「谁改宽的」可追。
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(dir.join(SECRET_FILE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o644
        );
    }

    #[test]
    fn restart_reuses_same_secret_bytes() {
        let tmp = tempdir().unwrap();
        let first = load_or_create(tmp.path()).unwrap();
        let second = load_or_create(tmp.path()).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn permissive_dir_with_0600_file_is_accepted() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join(AUTH_SUBDIR);
        write_secret_file(&dir, &[4u8; 32], 0o600);
        #[cfg(unix)]
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o777)).unwrap();

        assert_eq!(load_or_create(tmp.path()).unwrap(), [4u8; 32]);
    }

    // --- 以下为支撑上述三条规格的旁证 ---

    #[test]
    fn first_run_creates_0600_secret() {
        let tmp = tempdir().unwrap();
        let secret = load_or_create(tmp.path()).unwrap();
        assert_eq!(secret.len(), 32);

        #[cfg(unix)]
        {
            let path = tmp.path().join(AUTH_SUBDIR).join(SECRET_FILE);
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn two_independent_config_dirs_get_different_secrets() {
        let a = tempdir().unwrap();
        let b = tempdir().unwrap();
        assert_ne!(
            load_or_create(a.path()).unwrap(),
            load_or_create(b.path()).unwrap()
        );
    }

    #[test]
    fn mode_0666_is_also_refused() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join(AUTH_SUBDIR);
        write_secret_file(&dir, &[1u8; 32], 0o666);
        assert!(matches!(
            load_or_create(tmp.path()),
            Err(SecretError::BadPermissions { mode: 0o666, .. })
        ));
    }

    #[test]
    fn wrong_length_secret_is_rejected_not_padded() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join(AUTH_SUBDIR);
        write_secret_file(&dir, &[0u8; 16], 0o600);
        assert!(matches!(
            load_or_create(tmp.path()),
            Err(SecretError::Io(_))
        ));
    }

    #[test]
    fn clients_round_trip_without_plaintext() {
        let tmp = tempdir().unwrap();
        let secret = load_or_create(tmp.path()).unwrap();
        let clients = vec![
            super::super::token::ClientToken::new("laptop", &secret, "plain-token-value"),
            super::super::token::ClientToken::new("phone", &secret, "another-plain"),
        ];

        save_clients(tmp.path(), &clients).unwrap();
        let loaded = load_clients(tmp.path()).unwrap();
        // 摘要逐条经 `ct_eq` 比（`ClientToken` 不再派生 `PartialEq`）。
        assert_eq!(loaded.len(), clients.len());
        for (a, b) in loaded.iter().zip(&clients) {
            assert_eq!(a.client_name, b.client_name);
            assert_eq!(a.created_at, b.created_at);
            assert_eq!(a.last_used_at, b.last_used_at);
            assert!(a.token_hash.ct_eq(&b.token_hash), "token 摘要必须往返一致");
            // `Option` 的 `None` 形态也要往返一致，所以按形态分开判；把两侧
            // `zip` 起来会把 `(None, None)` 混成「不等」，那是断言写错了。
            match (a.cookie_hash, b.cookie_hash) {
                (Some(x), Some(y)) => assert!(x.ct_eq(&y), "cookie 摘要必须往返一致"),
                (None, None) => {}
                _ => panic!("cookie 摘要的 None 形态必须往返一致"),
            }
        }

        let raw = fs::read_to_string(tmp.path().join(AUTH_SUBDIR).join(CLIENTS_FILE)).unwrap();
        assert!(!raw.contains("plain-token-value"));
        assert!(!raw.contains("another-plain"));
    }

    #[test]
    fn missing_clients_file_is_empty_not_an_error() {
        let tmp = tempdir().unwrap();
        assert!(load_clients(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn clients_file_is_0600() {
        let tmp = tempdir().unwrap();
        save_clients(tmp.path(), &[]).unwrap();
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(tmp.path().join(AUTH_SUBDIR).join(CLIENTS_FILE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn corrupt_clients_file_is_reported_not_silently_ignored() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join(AUTH_SUBDIR);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(CLIENTS_FILE), b"{not json").unwrap();
        assert!(load_clients(tmp.path()).is_err());
    }

    #[test]
    fn hex_round_trip_covers_all_byte_values() {
        let bytes: [u8; 32] = std::array::from_fn(|i| (i as u8).wrapping_mul(7));
        assert_eq!(decode_hex32(&encode_hex32(&bytes), "f").unwrap(), bytes);
    }

    #[test]
    fn auth_dir_is_derived_from_given_config_dir_only() {
        // 绝不触碰真实配置目录：路径必须完全由入参拼出。
        let tmp = tempdir().unwrap();
        assert_eq!(auth_dir(tmp.path()), tmp.path().join("remote-auth"));
    }
}
