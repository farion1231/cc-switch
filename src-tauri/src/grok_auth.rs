//! Grok CLI 登录文件 `~/.grok/auth.json` 里 xAI OIDC 那一条（`https://auth.x.ai::<client>`）。
//!
//! Grok 官方卡绑了授权中心的 xAI 账号时，切到这张卡就把账号的登录写进这一条，Grok CLI
//! 下次读到的就是这个号。授权中心的 xAI 登录和 Grok CLI 是同一个 OAuth client，令牌通用；
//! 两边可能共用同一个 refresh token，所以：
//! - 切走前先看 CLI 有没有把它轮换过（[`read_live_login`]），换过就先收进授权中心；
//! - cc-switch 自己刷新换了 refresh token 时，CLI 手里的还是旧的，要写回去
//!   （[`sync_after_refresh`]），否则 CLI 下次刷新会被拒、掉登录。
//!
//! 只动这一条，文件里的其他条目（旧版 session 登录等）原样保留。
//!
//! **登录暂存。**第一次用托管账号覆盖 Grok CLI 自己的登录时，记下原来是谁
//! （`~/.cc-switch/grok-login-stash.json`，0600，不同步；不存令牌，那个号已经收进授权
//! 中心）。切回「跟随 Grok CLI 登录」的官方卡时按它还原，原来没登录就删掉这一条；还原后
//! 删掉暂存。授权中心删掉一个号、而 live 里是 CC Switch 替它写的登录时，也按暂存还原。

use std::path::{Path, PathBuf};

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Map, Value};

use crate::live::patch::json::{self, JsonStyle};
use crate::live::patch::{LivePatch, LiveWriteError};

/// 登录暂存文件名（在 CC Switch 的设备目录 `~/.cc-switch` 下）
pub(crate) const STASH_FILENAME: &str = "grok-login-stash.json";

/// 写进暂存的条目里不留令牌：还原时从授权中心取新的
const SECRET_FIELDS: [&str; 2] = ["key", "refresh_token"];

pub(crate) const OIDC_ISSUER: &str = "https://auth.x.ai";

/// `auth.json` 里 OIDC 登录那一条的 key
pub(crate) fn oidc_scope() -> String {
    format!(
        "{OIDC_ISSUER}::{}",
        crate::proxy::providers::xai_oauth_auth::XAI_CLIENT_ID
    )
}

pub(crate) fn auth_path() -> PathBuf {
    crate::grok_config::get_grok_config_dir().join("auth.json")
}

/// 要写进 Grok CLI 的一个账号的登录
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GrokCliLogin {
    /// xAI 用户 id（令牌的 `sub`），即授权中心的账号 id
    pub account_id: String,
    pub email: Option<String>,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at_ms: i64,
    pub team_id: Option<String>,
    pub principal_type: Option<String>,
    pub principal_id: Option<String>,
}

/// live 里 OIDC 那一条现在是谁、拿着哪个 refresh token
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveLogin {
    pub user_id: String,
    pub email: Option<String>,
    pub refresh_token: Option<String>,
}

/// 解析 `auth.json`（不存在或空白时为空对象），保留原文件的缩进、换行等格式
fn parse_root(
    path: &Path,
    pre: Option<&[u8]>,
) -> Result<(Map<String, Value>, JsonStyle), LiveWriteError> {
    let (doc, style) = json::parse(path, pre)?;
    let Value::Object(root) = doc else {
        unreachable!("json::parse 只返回对象")
    };
    Ok((root, style))
}

fn to_bytes(
    path: &Path,
    root: Map<String, Value>,
    style: &JsonStyle,
) -> Result<Vec<u8>, LiveWriteError> {
    json::serialize(path, &Value::Object(root), style)
}

fn non_empty(entry: &Map<String, Value>, key: &str) -> Option<String> {
    entry
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// 读出 OIDC 那一条的用户和 refresh token；文件不存在、没有这一条或这一条没有 `user_id`
/// 时为 `None`。
pub(crate) fn read_live_login(
    path: &Path,
    content: Option<&[u8]>,
) -> Result<Option<LiveLogin>, LiveWriteError> {
    Ok(read_live_entry(path, content)?
        .as_ref()
        .and_then(live_login_of))
}

/// OIDC 那一条里的登录；没有 `user_id` 时为 `None`
pub(crate) fn live_login_of(entry: &Map<String, Value>) -> Option<LiveLogin> {
    Some(LiveLogin {
        user_id: non_empty(entry, "user_id")?,
        email: non_empty(entry, "email"),
        refresh_token: non_empty(entry, "refresh_token"),
    })
}

/// 读盘上的 OIDC 登录（读不了、解析不了都当没有）
pub(crate) fn read_live_login_from_disk() -> Option<LiveLogin> {
    let path = auth_path();
    let content = crate::live::engine::read_current(&path).ok()??;
    read_live_login(&path, Some(&content)).ok()?
}

fn iso_micros(ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(ms)
        .unwrap_or_else(Utc::now)
        .to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// 以 `pre` 为底，把 OIDC 那一条换成 `login`。同一个用户时保留 CLI 自己写的其他字段
/// （名字、头像、团队设置等）；live 里不是这个用户时以 `base`（还原时暂存的原条目，不含
/// 令牌）为底，没有就整条重建，免得留下上一个号的资料。
pub(crate) fn merge_login(
    path: &Path,
    pre: Option<&[u8]>,
    login: &GrokCliLogin,
    base: Option<&Map<String, Value>>,
) -> Result<Vec<u8>, LiveWriteError> {
    let (mut root, style) = parse_root(path, pre)?;
    let scope = oidc_scope();
    let same_user = |entry: &&Map<String, Value>| {
        entry.get("user_id").and_then(Value::as_str) == Some(login.account_id.as_str())
    };
    let mut entry = root
        .get(&scope)
        .and_then(Value::as_object)
        .filter(same_user)
        .or(base.filter(same_user))
        .cloned()
        .unwrap_or_default();

    let mut set = |key: &str, value: String| {
        entry.insert(key.to_string(), Value::String(value));
    };
    set("key", login.access_token.clone());
    set("auth_mode", "oidc".to_string());
    set(
        "create_time",
        Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true),
    );
    set("user_id", login.account_id.clone());
    if let Some(email) = login.email.as_deref().filter(|e| e.contains('@')) {
        set("email", email.to_string());
    }
    set(
        "principal_type",
        login
            .principal_type
            .clone()
            .unwrap_or_else(|| "User".to_string()),
    );
    set(
        "principal_id",
        login
            .principal_id
            .clone()
            .unwrap_or_else(|| login.account_id.clone()),
    );
    if let Some(team_id) = &login.team_id {
        set("team_id", team_id.clone());
    }
    set("refresh_token", login.refresh_token.clone());
    set("expires_at", iso_micros(login.expires_at_ms));
    set("oidc_issuer", OIDC_ISSUER.to_string());
    set(
        "oidc_client_id",
        crate::proxy::providers::xai_oauth_auth::XAI_CLIENT_ID.to_string(),
    );

    root.insert(scope, Value::Object(entry));
    to_bytes(path, root, &style)
}

/// 删掉 OIDC 那一条（还原到「原来没登录」）。删完文件空了、且原来就没有这个文件时删文件。
pub(crate) fn remove_login(
    path: &Path,
    pre: Option<&[u8]>,
    delete_if_empty: bool,
) -> Result<Option<Vec<u8>>, LiveWriteError> {
    if pre.is_none() {
        return Ok(None);
    }
    let (mut root, style) = parse_root(path, pre)?;
    root.shift_remove(&oidc_scope());
    if root.is_empty() && delete_if_empty {
        return Ok(None);
    }
    to_bytes(path, root, &style).map(Some)
}

/// OIDC 那一条的完整内容（暂存时用）
pub(crate) fn read_live_entry(
    path: &Path,
    content: Option<&[u8]>,
) -> Result<Option<Map<String, Value>>, LiveWriteError> {
    Ok(parse_root(path, content)?
        .0
        .get(&oidc_scope())
        .and_then(Value::as_object)
        .cloned())
}

/// 第一次被 CC Switch 覆盖前，Grok CLI 自己的登录是谁
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct GrokLoginStash {
    #[serde(default)]
    pub version: u32,
    /// 原来登录的用户；`None` 表示原来没登录
    #[serde(default)]
    pub user_id: Option<String>,
    /// 原来那一条（去掉了令牌），还原时把名字、头像等资料带回去
    #[serde(default)]
    pub entry: Option<Map<String, Value>>,
    /// 原来有没有 `auth.json`：没有的话还原成「没登录」时连文件一起删
    #[serde(default)]
    pub file_existed: bool,
}

impl GrokLoginStash {
    /// 按 live 现在的内容建暂存
    pub(crate) fn capture(entry: Option<Map<String, Value>>, file_existed: bool) -> Self {
        let user_id = entry.as_ref().and_then(|entry| non_empty(entry, "user_id"));
        let entry = entry.filter(|_| user_id.is_some()).map(|mut entry| {
            for field in SECRET_FIELDS {
                entry.shift_remove(field);
            }
            entry
        });
        Self {
            version: 1,
            user_id,
            entry,
            file_existed,
        }
    }

    /// 还原成暂存里原来的登录：`login` 是原来那个号的新登录，`None` 表示还原到没登录
    pub(crate) fn restore_write(&self, login: Option<GrokCliLogin>) -> LoginWrite {
        match login {
            Some(login) => LoginWrite::Set {
                login: Box::new(login),
                base: self.entry.clone(),
            },
            None => LoginWrite::Remove {
                delete_if_empty: !self.file_existed,
            },
        }
    }

    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec_pretty(self).unwrap_or_default();
        bytes.push(b'\n');
        bytes
    }
}

pub(crate) fn stash_path() -> PathBuf {
    crate::live::engine::DeviceStore::for_device().file(STASH_FILENAME)
}

/// 读登录暂存。`Ok(None)`：没有暂存；`Err`：文件在但读不了（不能当成没有，免得覆盖掉）。
pub(crate) fn read_stash() -> Result<Option<GrokLoginStash>, String> {
    let path = stash_path();
    let Some(bytes) = crate::live::engine::read_current(&path).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| format!("Grok 登录暂存 {} 无法解析: {e}", path.display()))
}

/// `auth.json` 里 OIDC 那一条要换成什么
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LoginWrite {
    /// 换成这个账号的登录；`base` 是还原时暂存的原条目
    Set {
        login: Box<GrokCliLogin>,
        base: Option<Map<String, Value>>,
    },
    /// 删掉这一条（还原到原来没登录）
    Remove { delete_if_empty: bool },
}

/// 写 `auth.json` 的补丁。`expected` 是准备阶段看到的 OIDC 登录：发布前 Grok CLI 又刷新过
/// （refresh token 变了），就拒绝写，免得把刚轮换出来的令牌覆盖掉。
pub(crate) struct GrokAuthPatch {
    pub write: LoginWrite,
    pub expected: Option<LiveLogin>,
}

impl LivePatch for GrokAuthPatch {
    fn apply(&self, path: &Path, pre: Option<&[u8]>) -> Result<Vec<u8>, LiveWriteError> {
        Ok(self.apply_file(path, pre)?.unwrap_or_default())
    }

    fn apply_file(
        &self,
        path: &Path,
        pre: Option<&[u8]>,
    ) -> Result<Option<Vec<u8>>, LiveWriteError> {
        if read_live_login(path, pre)? != self.expected {
            return Err(LiveWriteError::Conflict {
                path: path.to_path_buf(),
            });
        }
        match &self.write {
            LoginWrite::Set { login, base } => {
                merge_login(path, pre, login, base.as_ref()).map(Some)
            }
            LoginWrite::Remove { delete_if_empty } => remove_login(path, pre, *delete_if_empty),
        }
    }
}

/// 直接写 `auth.json`（不经切换事务：授权中心删号时用），写前按 `expected` 比对。
pub(crate) fn write_now(patch: &GrokAuthPatch) -> Result<(), String> {
    let path = auth_path();
    let pre = crate::live::engine::read_current(&path).map_err(|e| e.to_string())?;
    match patch
        .apply_file(&path, pre.as_deref())
        .map_err(|e| e.to_string())?
    {
        Some(bytes) => {
            crate::config::atomic_write_private(&path, &bytes).map_err(|e| e.to_string())
        }
        None => crate::config::delete_file(&path).map_err(|e| e.to_string()),
    }
}

/// cc-switch 刷新账号后，把新令牌同步给共用同一个 refresh token 的 Grok CLI。
///
/// 只有 live 那一条就是这个账号、且拿着这次被换掉的 `consumed_refresh` 时才写；CLI 自己
/// 登录的另一套会话（refresh token 不同）不碰。返回是否写了。
pub(crate) fn sync_after_refresh(
    consumed_refresh: &str,
    login: &GrokCliLogin,
) -> Result<bool, String> {
    let path = auth_path();
    let pre = crate::live::engine::read_current(&path).map_err(|e| e.to_string())?;
    let live = read_live_login(&path, pre.as_deref()).map_err(|e| e.to_string())?;
    let shares_session = live.is_some_and(|live| {
        live.user_id == login.account_id && live.refresh_token.as_deref() == Some(consumed_refresh)
    });
    if !shares_session {
        return Ok(false);
    }
    let bytes = merge_login(&path, pre.as_deref(), login, None).map_err(|e| e.to_string())?;
    crate::config::atomic_write_private(&path, &bytes).map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn login(account: &str, refresh: &str) -> GrokCliLogin {
        GrokCliLogin {
            account_id: account.to_string(),
            email: Some(format!("{account}@example.com")),
            access_token: format!("at-{account}"),
            refresh_token: refresh.to_string(),
            expires_at_ms: 1_800_000_000_000,
            team_id: Some("team-1".to_string()),
            principal_type: Some("User".to_string()),
            principal_id: Some(account.to_string()),
        }
    }

    fn path() -> PathBuf {
        PathBuf::from("/tmp/grok/auth.json")
    }

    fn entry(bytes: &[u8]) -> Map<String, Value> {
        let root: Value = serde_json::from_slice(bytes).unwrap();
        root[oidc_scope()].as_object().unwrap().clone()
    }

    #[test]
    fn writes_fresh_file_with_cli_shape() {
        let out = merge_login(&path(), None, &login("u1", "rt1"), None).unwrap();
        let e = entry(&out);
        assert_eq!(e["key"], "at-u1");
        assert_eq!(e["user_id"], "u1");
        assert_eq!(e["refresh_token"], "rt1");
        assert_eq!(e["auth_mode"], "oidc");
        assert_eq!(e["oidc_issuer"], OIDC_ISSUER);
        assert_eq!(e["email"], "u1@example.com");
        assert!(e["expires_at"].as_str().unwrap().ends_with('Z'));
    }

    #[test]
    fn same_user_keeps_cli_fields_and_other_entries() {
        let pre = serde_json::json!({
            "https://accounts.x.ai/sign-in": {"key": "legacy"},
            (oidc_scope()): {"user_id": "u1", "first_name": "Ann", "key": "old", "refresh_token": "rt0"}
        })
        .to_string();
        let out = merge_login(&path(), Some(pre.as_bytes()), &login("u1", "rt1"), None).unwrap();
        let e = entry(&out);
        assert_eq!(e["first_name"], "Ann");
        assert_eq!(e["key"], "at-u1");
        let root: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(root["https://accounts.x.ai/sign-in"]["key"], "legacy");
    }

    #[test]
    fn other_user_entry_is_rebuilt() {
        let pre = serde_json::json!({
            (oidc_scope()): {"user_id": "u2", "first_name": "Bob", "key": "old"}
        })
        .to_string();
        let out = merge_login(&path(), Some(pre.as_bytes()), &login("u1", "rt1"), None).unwrap();
        let e = entry(&out);
        assert!(!e.contains_key("first_name"));
        assert_eq!(e["user_id"], "u1");
    }

    #[test]
    fn reads_live_login() {
        assert_eq!(read_live_login(&path(), None).unwrap(), None);
        assert_eq!(read_live_login(&path(), Some(b"  ")).unwrap(), None);
        let pre = serde_json::json!({
            (oidc_scope()): {"user_id": "u1", "refresh_token": "rt1"}
        })
        .to_string();
        assert_eq!(
            read_live_login(&path(), Some(pre.as_bytes())).unwrap(),
            Some(LiveLogin {
                user_id: "u1".to_string(),
                email: None,
                refresh_token: Some("rt1".to_string())
            })
        );
        assert!(read_live_login(&path(), Some(b"{oops")).is_err());
    }

    #[test]
    fn patch_refuses_when_cli_rotated_since_prepare() {
        let pre = serde_json::json!({
            (oidc_scope()): {"user_id": "u2", "refresh_token": "rotated"}
        })
        .to_string();
        let patch = |seen: &str| GrokAuthPatch {
            write: LoginWrite::Set {
                login: Box::new(login("u1", "rt1")),
                base: None,
            },
            expected: Some(LiveLogin {
                user_id: "u2".to_string(),
                email: None,
                refresh_token: Some(seen.to_string()),
            }),
        };
        assert!(matches!(
            patch("before").apply(&path(), Some(pre.as_bytes())),
            Err(LiveWriteError::Conflict { .. })
        ));
        assert!(patch("rotated")
            .apply(&path(), Some(pre.as_bytes()))
            .is_ok());
    }

    #[test]
    fn stash_keeps_identity_and_profile_but_no_tokens() {
        let entry = serde_json::json!({
            "user_id": "u2", "first_name": "Bob", "key": "at", "refresh_token": "rt"
        });
        let stash = GrokLoginStash::capture(entry.as_object().cloned(), true);
        assert_eq!(stash.user_id.as_deref(), Some("u2"));
        let kept = stash.entry.as_ref().unwrap();
        assert_eq!(kept["first_name"], "Bob");
        assert!(!kept.contains_key("key") && !kept.contains_key("refresh_token"));
        assert!(stash.file_existed);

        let none = GrokLoginStash::capture(None, false);
        assert_eq!(none.user_id, None);
        assert_eq!(none.entry, None);
    }

    #[test]
    fn restore_brings_back_profile_fields_from_the_stash() {
        let pre = serde_json::json!({
            (oidc_scope()): {"user_id": "u1", "key": "at-u1", "refresh_token": "rt-u1"}
        })
        .to_string();
        let base = serde_json::json!({"user_id": "u2", "first_name": "Bob"});
        let patch = GrokAuthPatch {
            write: LoginWrite::Set {
                login: Box::new(login("u2", "rt2")),
                base: base.as_object().cloned(),
            },
            expected: read_live_login(&path(), Some(pre.as_bytes())).unwrap(),
        };
        let out = patch
            .apply_file(&path(), Some(pre.as_bytes()))
            .unwrap()
            .unwrap();
        let e = entry(&out);
        assert_eq!(e["first_name"], "Bob");
        assert_eq!(e["user_id"], "u2");
        assert_eq!(e["refresh_token"], "rt2");
    }

    #[test]
    fn restore_to_signed_out_removes_the_entry_or_the_file() {
        let only = serde_json::json!({(oidc_scope()): {"user_id": "u1"}}).to_string();
        assert_eq!(
            remove_login(&path(), Some(only.as_bytes()), true).unwrap(),
            None
        );
        let kept = remove_login(&path(), Some(only.as_bytes()), false)
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&kept).unwrap(),
            serde_json::json!({})
        );

        let mixed = serde_json::json!({
            "https://accounts.x.ai/sign-in": {"key": "legacy"},
            (oidc_scope()): {"user_id": "u1"}
        })
        .to_string();
        let out = remove_login(&path(), Some(mixed.as_bytes()), true)
            .unwrap()
            .unwrap();
        let root: Value = serde_json::from_slice(&out).unwrap();
        assert!(root.get(oidc_scope()).is_none());
        assert_eq!(root["https://accounts.x.ai/sign-in"]["key"], "legacy");
    }
}
