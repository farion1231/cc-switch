//! 共享层：让每个账号目录用起来和默认的 `~/.claude` 一样（MCP、Skills、设置、会话历史）。
//!
//! | 内容 | 方式 |
//! |---|---|
//! | `skills/` `commands/` `agents/` `output-styles/` `plugins/` `rules/` `workflows/` | 目录链接到默认目录 |
//! | `projects/` `file-history/` `todos/` `plans/` | 目录链接（换账号后会话历史照样看得到） |
//! | `settings.json` | 镜像：去掉关键字段、独有字段和 `CLAUDE_CONFIG_DIR` 一类的键；顶层按键三方合并，保留账号自己改过的 |
//! | `.claude.json` | 只镜像 `mcpServers`（只删自己镜像过的条目），补上 `hasCompletedOnboarding`、`theme` |
//! | `CLAUDE.md` | 一行 `@<默认 CLAUDE.md>` |
//!
//! 已经是真实目录的不动（只有空目录会换成链接），绝不复制、不删用户的东西。
//! 上一次镜像的内容记在账号外层目录的 `.mirror-state.json`，三方合并靠它判断谁改了什么。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::AppError;
use crate::live::floor;

/// 链接到默认目录的子目录。
const LINKED_DIRS: &[&str] = &[
    "skills",
    "commands",
    "agents",
    "output-styles",
    "plugins",
    "rules",
    "workflows",
    "projects",
    "file-history",
    "todos",
    "plans",
];

/// 镜像 `settings.json` 时一律去掉的 env 键（会把账号指到别的目录或别的登录）。
const STRIPPED_ENV: &[&str] = &["CLAUDE_CONFIG_DIR", "CLAUDE_SECURESTORAGE_CONFIG_DIR"];

/// 这些键一律跟随默认值，账号里改了也不保留（共享的会话历史不能被某个账号按更短的周期清掉）。
const FORCED_TOP: &[&str] = &["cleanupPeriodDays"];

/// `.claude.json` 里缺失时从默认补上的键（免得新账号第一次启动又走一遍引导）。
const SEEDED_PROFILE_KEYS: &[&str] = &["hasCompletedOnboarding", "theme"];

const STATE_FILENAME: &str = ".mirror-state.json";

/// 默认登录那一侧的路径。
pub struct Defaults {
    /// 默认的 Claude 配置目录（`~/.claude` 或设置里的覆盖目录）
    pub config_dir: PathBuf,
    /// 默认的 `.claude.json`
    pub profile: PathBuf,
}

impl Defaults {
    pub fn current() -> Self {
        Self {
            config_dir: crate::config::get_claude_config_dir(),
            profile: crate::config::get_claude_mcp_path(),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MirrorState {
    /// 上一次镜像进去的 `settings.json`（已去掉关键字段）
    #[serde(default)]
    settings: Option<Value>,
    /// 上一次镜像进去的 MCP 名字
    #[serde(default)]
    mcp: BTreeSet<String>,
}

/// 同步一个账号目录。`home` 是账号外层目录（放镜像状态）。
pub fn sync_account(defaults: &Defaults, config_dir: &Path, home: &Path) -> Result<(), AppError> {
    if !config_dir.is_dir() {
        return Ok(());
    }
    let state_path = home.join(STATE_FILENAME);
    let mut state: MirrorState = fs::read(&state_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();

    link_dirs(&defaults.config_dir, config_dir);
    import_claude_md(&defaults.config_dir, config_dir)?;
    mirror_settings(&defaults.config_dir, config_dir, &mut state)?;
    mirror_profile(&defaults.profile, config_dir, &mut state)?;

    let bytes =
        serde_json::to_vec_pretty(&state).map_err(|source| AppError::JsonSerialize { source })?;
    crate::config::atomic_write_private(&state_path, &bytes)
}

// ---------- 目录链接 ----------

fn link_dirs(default_dir: &Path, config_dir: &Path) {
    for name in LINKED_DIRS {
        let target = default_dir.join(name);
        let link = config_dir.join(name);
        if let Err(error) = link_dir(&target, &link) {
            log::warn!("[ClaudeAccount] 共享 {name} 失败: {error}");
        }
    }
}

fn link_dir(target: &Path, link: &Path) -> Result<(), AppError> {
    fs::create_dir_all(target).map_err(|e| AppError::io(target, e))?;
    match fs::symlink_metadata(link) {
        Ok(meta) if is_link(&meta) => {
            if fs::read_link(link).ok().as_deref() == Some(target) {
                return Ok(());
            }
            crate::agy_config::remove_link(link)?;
        }
        Ok(meta) if meta.is_dir() => {
            // Claude Code 抢先建的空目录换成链接；有内容的不动
            let empty = fs::read_dir(link)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(false);
            if !empty {
                log::warn!(
                    "[ClaudeAccount] {} 已是有内容的目录，不共享",
                    link.display()
                );
                return Ok(());
            }
            fs::remove_dir(link).map_err(|e| AppError::io(link, e))?;
        }
        Ok(_) => {
            log::warn!("[ClaudeAccount] {} 是文件，不共享", link.display());
            return Ok(());
        }
        Err(_) => {}
    }
    crate::agy_config::create_dir_link(target, link)
}

fn is_link(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        return meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    }
    #[allow(unreachable_code)]
    false
}

// ---------- CLAUDE.md ----------

fn import_line(default_dir: &Path) -> String {
    format!("@{}\n", default_dir.join("CLAUDE.md").to_string_lossy())
}

/// 账号目录的 `CLAUDE.md` 引用默认的那份。账号里已有别的内容（用户自己写的）就不动。
fn import_claude_md(default_dir: &Path, config_dir: &Path) -> Result<(), AppError> {
    let path = config_dir.join("CLAUDE.md");
    let line = import_line(default_dir);
    match fs::read_to_string(&path) {
        Ok(current) if current == line => Ok(()),
        Ok(_) => Ok(()),
        Err(_) => fs::write(&path, line).map_err(|e| AppError::io(&path, e)),
    }
}

// ---------- settings.json ----------

/// 去掉不能带进账号目录的键：关键字段（第三方地址、Key、模型名……）、独有字段、指向别处的目录。
fn strip_settings(doc: &Value) -> Value {
    let Some(root) = doc.as_object() else {
        return Value::Object(Map::new());
    };
    let mut out = Map::new();
    for (key, value) in root {
        if floor::claude_floor_top(key) {
            continue;
        }
        if key == "env" {
            let env: Map<String, Value> = value
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(name, _)| {
                    !floor::claude_floor_env(name)
                        && !floor::claude_exclusive_env(name)
                        && !STRIPPED_ENV.contains(&name.as_str())
                })
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();
            if !env.is_empty() {
                out.insert(key.clone(), Value::Object(env));
            }
            continue;
        }
        out.insert(key.clone(), value.clone());
    }
    Value::Object(out)
}

/// 顶层按键三方合并：默认这边的值为准，账号自己改过、默认又没动的键保留账号的。
fn merge_settings(stripped: &Value, account: &Value, last: Option<&Value>) -> Value {
    let empty = Map::new();
    let base = stripped.as_object().unwrap_or(&empty);
    let mine = account.as_object().unwrap_or(&empty);
    let last = last.and_then(Value::as_object);
    let mut out = base.clone();
    for (key, value) in mine {
        if FORCED_TOP.contains(&key.as_str()) {
            continue;
        }
        let before = last.and_then(|last| last.get(key));
        let account_changed = match last {
            Some(_) => before != Some(value),
            // 第一次镜像：账号里原有的键都当作账号自己的
            None => true,
        };
        let default_unchanged = base.get(key) == before;
        if account_changed && (default_unchanged || last.is_none() && !base.contains_key(key)) {
            out.insert(key.clone(), value.clone());
        }
    }
    // 账号自己的 env 里也不能有指向别处的目录（会链式跳转）
    if let Some(env) = out.get_mut("env").and_then(Value::as_object_mut) {
        for name in STRIPPED_ENV {
            env.shift_remove(*name);
        }
    }
    Value::Object(out)
}

fn read_json(path: &Path) -> Option<Value> {
    let bytes = fs::read(path).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    let text = text.trim_start_matches('\u{feff}');
    if text.trim().is_empty() {
        return Some(Value::Object(Map::new()));
    }
    serde_json::from_str(text).ok()
}

fn mirror_settings(
    default_dir: &Path,
    config_dir: &Path,
    state: &mut MirrorState,
) -> Result<(), AppError> {
    let default_path = default_dir.join("settings.json");
    let default = match fs::metadata(&default_path) {
        Ok(_) => match read_json(&default_path) {
            Some(doc) => doc,
            // 默认文件读不懂：这次不镜像，免得用空对象盖掉账号里的设置
            None => return Ok(()),
        },
        Err(_) => Value::Object(Map::new()),
    };
    let stripped = strip_settings(&default);
    let account_path = config_dir.join("settings.json");
    let account = match fs::metadata(&account_path) {
        Ok(_) => match read_json(&account_path) {
            Some(doc) => doc,
            None => return Ok(()),
        },
        Err(_) => Value::Object(Map::new()),
    };
    let merged = merge_settings(&stripped, &account, state.settings.as_ref());
    if merged != account || !account_path.exists() {
        write_json(&account_path, &merged)?;
    }
    state.settings = Some(stripped);
    Ok(())
}

fn write_json(path: &Path, doc: &Value) -> Result<(), AppError> {
    let mut bytes =
        serde_json::to_vec_pretty(doc).map_err(|source| AppError::JsonSerialize { source })?;
    bytes.push(b'\n');
    crate::config::atomic_write_private(path, &bytes)
}

// ---------- .claude.json ----------

/// 只在账号已经登录过（`.claude.json` 已存在）时同步：登录前不替 Claude Code 建这个文件。
fn mirror_profile(
    default_profile: &Path,
    config_dir: &Path,
    state: &mut MirrorState,
) -> Result<(), AppError> {
    let path = config_dir.join(".claude.json");
    if !path.exists() {
        return Ok(());
    }
    let Some(mut account) = read_json(&path) else {
        return Ok(());
    };
    let Some(account_root) = account.as_object_mut() else {
        return Ok(());
    };
    let default = read_json(default_profile).unwrap_or_else(|| Value::Object(Map::new()));
    let default_servers = default
        .get("mcpServers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let before = account_root.clone();
    let servers = account_root
        .entry("mcpServers")
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(servers) = servers.as_object_mut() {
        for name in &state.mcp {
            if !default_servers.contains_key(name) {
                servers.shift_remove(name);
            }
        }
        for (name, server) in &default_servers {
            servers.insert(name.clone(), server.clone());
        }
    }
    for key in SEEDED_PROFILE_KEYS {
        if !account_root.contains_key(*key) {
            if let Some(value) = default.get(*key) {
                account_root.insert(key.to_string(), value.clone());
            }
        }
    }
    state.mcp = default_servers.keys().cloned().collect();
    if *account_root != before {
        write_json(&path, &account)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn setup() -> (tempfile::TempDir, Defaults, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let default_dir = tmp.path().join("default");
        fs::create_dir_all(&default_dir).unwrap();
        let home = tmp.path().join("account");
        let config_dir = home.join("config");
        fs::create_dir_all(&config_dir).unwrap();
        let defaults = Defaults {
            config_dir: default_dir.clone(),
            profile: tmp.path().join(".claude.json"),
        };
        (tmp, defaults, config_dir, home)
    }

    #[test]
    fn mirrors_settings_without_third_party_keys() {
        let (_tmp, defaults, config_dir, home) = setup();
        write_json(
            &defaults.config_dir.join("settings.json"),
            &json!({
                "model": "opus",
                "hooks": {"Stop": []},
                "env": {
                    "ANTHROPIC_BASE_URL": "https://relay.example",
                    "ANTHROPIC_AUTH_TOKEN": "sk-x",
                    "CLAUDE_CONFIG_DIR": "/elsewhere",
                    "DISABLE_TELEMETRY": "1"
                }
            }),
        )
        .unwrap();
        sync_account(&defaults, &config_dir, &home).unwrap();
        let account = read_json(&config_dir.join("settings.json")).unwrap();
        assert_eq!(
            account,
            json!({"hooks": {"Stop": []}, "env": {"DISABLE_TELEMETRY": "1"}})
        );
    }

    #[test]
    fn keeps_account_edits_and_follows_default_edits() {
        let (_tmp, defaults, config_dir, home) = setup();
        let default_settings = defaults.config_dir.join("settings.json");
        write_json(
            &default_settings,
            &json!({"theme": "dark", "statusLine": "a", "cleanupPeriodDays": 30}),
        )
        .unwrap();
        sync_account(&defaults, &config_dir, &home).unwrap();

        // 账号里改了 theme、加了自己的键；默认这边改了 statusLine
        write_json(
            &config_dir.join("settings.json"),
            &json!({"theme": "light", "statusLine": "a", "mine": true, "cleanupPeriodDays": 1}),
        )
        .unwrap();
        write_json(
            &default_settings,
            &json!({"theme": "dark", "statusLine": "b", "cleanupPeriodDays": 30}),
        )
        .unwrap();
        sync_account(&defaults, &config_dir, &home).unwrap();
        let account = read_json(&config_dir.join("settings.json")).unwrap();
        assert_eq!(account["theme"], json!("light"));
        assert_eq!(account["statusLine"], json!("b"));
        assert_eq!(account["mine"], json!(true));
        assert_eq!(account["cleanupPeriodDays"], json!(30));
    }

    #[test]
    fn default_deletion_propagates() {
        let (_tmp, defaults, config_dir, home) = setup();
        let default_settings = defaults.config_dir.join("settings.json");
        write_json(&default_settings, &json!({"hooks": {}, "statusLine": "a"})).unwrap();
        sync_account(&defaults, &config_dir, &home).unwrap();
        write_json(&default_settings, &json!({"hooks": {}})).unwrap();
        sync_account(&defaults, &config_dir, &home).unwrap();
        let account = read_json(&config_dir.join("settings.json")).unwrap();
        assert_eq!(account, json!({"hooks": {}}));
    }

    #[test]
    fn mirrors_mcp_only_after_sign_in_and_removes_only_own_entries() {
        let (_tmp, defaults, config_dir, home) = setup();
        write_json(
            &defaults.profile,
            &json!({"mcpServers": {"a": {"command": "a"}, "b": {"command": "b"}}, "hasCompletedOnboarding": true, "oauthAccount": {"accountUuid": "default"}}),
        )
        .unwrap();
        // 还没登录：不建 .claude.json
        sync_account(&defaults, &config_dir, &home).unwrap();
        assert!(!config_dir.join(".claude.json").exists());

        write_json(
            &config_dir.join(".claude.json"),
            &json!({"oauthAccount": {"accountUuid": "mine"}, "mcpServers": {"own": {"command": "o"}}}),
        )
        .unwrap();
        sync_account(&defaults, &config_dir, &home).unwrap();
        let profile = read_json(&config_dir.join(".claude.json")).unwrap();
        assert_eq!(profile["oauthAccount"]["accountUuid"], json!("mine"));
        assert_eq!(profile["hasCompletedOnboarding"], json!(true));
        assert_eq!(
            profile["mcpServers"],
            json!({"own": {"command": "o"}, "a": {"command": "a"}, "b": {"command": "b"}})
        );

        // 默认里删了 b：账号里的 b 跟着删，账号自己的 own 不动
        write_json(
            &defaults.profile,
            &json!({"mcpServers": {"a": {"command": "a"}}}),
        )
        .unwrap();
        sync_account(&defaults, &config_dir, &home).unwrap();
        let profile = read_json(&config_dir.join(".claude.json")).unwrap();
        assert_eq!(
            profile["mcpServers"],
            json!({"own": {"command": "o"}, "a": {"command": "a"}})
        );
    }

    #[test]
    fn imports_claude_md_unless_account_has_its_own() {
        let (_tmp, defaults, config_dir, home) = setup();
        sync_account(&defaults, &config_dir, &home).unwrap();
        let path = config_dir.join("CLAUDE.md");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            import_line(&defaults.config_dir)
        );
        fs::write(&path, "my own notes").unwrap();
        sync_account(&defaults, &config_dir, &home).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "my own notes");
    }

    #[cfg(unix)]
    #[test]
    fn links_shared_dirs_and_leaves_real_ones() {
        let (_tmp, defaults, config_dir, home) = setup();
        fs::create_dir_all(config_dir.join("plugins")).unwrap();
        fs::write(config_dir.join("plugins").join("x"), "x").unwrap();
        fs::create_dir_all(config_dir.join("todos")).unwrap();
        sync_account(&defaults, &config_dir, &home).unwrap();

        let skills = fs::symlink_metadata(config_dir.join("skills")).unwrap();
        assert!(skills.file_type().is_symlink());
        assert_eq!(
            fs::read_link(config_dir.join("skills")).unwrap(),
            defaults.config_dir.join("skills")
        );
        // 空目录换成了链接，有内容的保留
        assert!(fs::symlink_metadata(config_dir.join("todos"))
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(!fs::symlink_metadata(config_dir.join("plugins"))
            .unwrap()
            .file_type()
            .is_symlink());
        // 再同步一次不出错
        sync_account(&defaults, &config_dir, &home).unwrap();
    }
}
