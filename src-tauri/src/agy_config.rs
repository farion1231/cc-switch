//! Antigravity CLI (`agy`) 的 live 配置。
//!
//! `agy` 只从进程环境读取 `GEMINI_API_KEY` 和 `GOOGLE_GEMINI_BASE_URL`，不读
//! `.env`。`settings.json` 只负责 `modelProvider` 和 `model`。切换供应商时：
//!
//! 1. 按键补丁写入 `~/.gemini/antigravity-cli/settings.json`，其余键不动。
//! 2. 把 Key 和地址写进同目录的 `env`（权限 0600）。
//! 3. 安装 `bin/agy` 包装脚本，启动时注入这些变量再执行真正的 `agy`。
//! 4. 把包装目录放到 `~/.zshenv` 的 `PATH` 最前面，这样终端里的 `agy` 会走到包装。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

#[cfg(test)]
use crate::config::atomic_write_private;
use crate::config::{get_home_dir, write_text_file};
use crate::error::AppError;
use crate::live::patch::json::JsonPatch;
use crate::live::patch::{KeyPath, LivePatch};

const WRAPPER_MARK: &str = "cc-switch-agy-wrapper";
const PATH_MARK: &str = "cc-switch agy wrapper";

/// `~/.gemini/antigravity-cli`
pub fn agy_dir() -> PathBuf {
    get_home_dir().join(".gemini").join("antigravity-cli")
}

pub fn customizations_dir() -> PathBuf {
    get_home_dir().join(".gemini").join("config")
}

pub fn settings_path() -> PathBuf {
    agy_dir().join("settings.json")
}

pub fn env_path() -> PathBuf {
    agy_dir().join("env")
}

#[cfg(test)]
fn apply_in(root: &Path, settings: &Value, install_shell: bool) -> Result<(), AppError> {
    let projection = AgyProjection::of(settings);
    write_settings(&root.join("settings.json"), &projection)?;
    write_env(&root.join("env"), &projection)?;
    install_wrapper(&root.join("bin").join("agy"), &projection)?;
    if install_shell {
        ensure_wrapper_on_path(&root.join("bin"))?;
    }
    Ok(())
}

/// 从 live 读回编辑器用的 `{ env, config }`。
pub fn read_live_settings() -> Result<Value, AppError> {
    let settings_file = settings_path();
    if !settings_file.exists() && !env_path().exists() {
        return Err(AppError::localized(
            "agy.config.missing",
            "Antigravity 配置文件不存在",
            "Antigravity configuration files are missing",
        ));
    }
    let settings: Value = if settings_file.exists() {
        let raw = fs::read(&settings_file).map_err(|e| AppError::io(&settings_file, e))?;
        serde_json::from_slice(&raw)
            .map_err(|e| AppError::Config(format!("Invalid agy settings.json: {e}")))?
    } else {
        json!({})
    };
    let env_map = read_env_file(&env_path())?;
    let mut env = serde_json::Map::new();
    if let Some(key) = env_map.get("GEMINI_API_KEY") {
        env.insert("GEMINI_API_KEY".into(), json!(key));
    }
    if let Some(url) = env_map.get("GOOGLE_GEMINI_BASE_URL") {
        env.insert("GOOGLE_GEMINI_BASE_URL".into(), json!(url));
    }
    let model = settings.get("model").cloned().filter(|v| !v.is_null());
    Ok(json!({
        "env": env,
        "config": {
            "model": { "name": model.unwrap_or(Value::Null) }
        }
    }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgyProjection {
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
}

impl AgyProjection {
    fn of(settings: &Value) -> Self {
        let env = settings.get("env");
        let api_key = non_empty(env_string(env, "GEMINI_API_KEY"));
        let base_url = non_empty(env_string(env, "GOOGLE_GEMINI_BASE_URL"));
        let model = non_empty(
            settings
                .pointer("/config/model/name")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| env_string(env, "GEMINI_MODEL")),
        );
        Self {
            api_key,
            base_url,
            model,
        }
    }
}

fn env_string(env: Option<&Value>, key: &str) -> Option<String> {
    env?.get(key)?.as_str().map(str::to_string)
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

fn settings_patch(projection: &AgyProjection) -> JsonPatch {
    let mut patch = JsonPatch::default();
    let provider = KeyPath::new(&["modelProvider"]);
    let model = KeyPath::new(&["model"]);
    if projection.api_key.is_some() {
        patch.set.push((provider, json!("gemini")));
    } else {
        patch.remove.push(provider);
    }
    match &projection.model {
        Some(name) => patch.set.push((model, json!(name))),
        None => patch.remove.push(model),
    }
    patch
}

#[cfg(test)]
fn write_settings(path: &Path, projection: &AgyProjection) -> Result<(), AppError> {
    let patch = settings_patch(projection);
    let pre = if path.exists() {
        Some(fs::read(path).map_err(|e| AppError::io(path, e))?)
    } else {
        None
    };
    let bytes = patch
        .apply(path, pre.as_deref())
        .map_err(|e| AppError::Message(format!("写入 agy settings.json 失败: {e}")))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    atomic_write_private(path, &bytes)
}

fn env_bytes(projection: &AgyProjection) -> Vec<u8> {
    let mut lines = vec![format!("# {PATH_MARK}")];
    if let Some(key) = &projection.api_key {
        lines.push(format!("GEMINI_API_KEY={}", shell_quote(key)));
    }
    if let Some(url) = &projection.base_url {
        lines.push(format!("GOOGLE_GEMINI_BASE_URL={}", shell_quote(url)));
    }
    if let Some(model) = &projection.model {
        lines.push(format!("AGY_MODEL={}", shell_quote(model)));
    }
    lines.push(String::new());
    lines.join("\n").into_bytes()
}

#[cfg(test)]
fn write_env(path: &Path, projection: &AgyProjection) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    atomic_write_private(path, &env_bytes(projection))
}

/// Key-bearing files and the current provider pointer share the crash-recoverable operation.
pub(crate) fn apply_managed(
    db: &crate::database::Database,
    settings: &Value,
    operation: &str,
    target: crate::mode::state::PendingTarget,
) -> Result<crate::mode::operation::OperationReport, AppError> {
    use crate::live::{engine::LiveFile, patch::WholeFile};
    use crate::mode::operation::{AppWrite, FileChange};
    if !cfg!(unix) {
        return Err(AppError::Config(
            "Antigravity provider switching currently requires a POSIX shell (macOS/Linux)".into(),
        ));
    }
    let write = AppWrite::begin(db, "agy")?;
    let projection = AgyProjection::of(settings);
    let patch = settings_patch(&projection);
    // Validate before installing the launcher or publishing any changes.
    let path = settings_path();
    let pre = if path.exists() {
        Some(fs::read(&path).map_err(|e| AppError::io(&path, e))?)
    } else {
        None
    };
    patch.apply(&path, pre.as_deref()).map_err(AppError::from)?;
    install_wrapper(&agy_dir().join("bin/agy"), &projection)?;
    ensure_wrapper_on_path(&agy_dir().join("bin"))?;
    let env = WholeFile::Write(env_bytes(&projection));
    write.run(
        operation,
        &[
            FileChange {
                file: LiveFile::private(env_path()),
                patch: &env,
            },
            FileChange {
                file: LiveFile::private(path),
                patch: &patch,
            },
        ],
        target,
    )
}

fn install_wrapper(path: &Path, _projection: &AgyProjection) -> Result<(), AppError> {
    if path.exists() && !is_wrapper(path) {
        return Err(AppError::Config(
            "Refusing to overwrite an unmanaged agy launcher".into(),
        ));
    }
    let real = real_agy_bin(path);
    let model_flag = {
        r#"
# Subcommands (MCP, plugins, models, help) must not receive a session model flag.
case "${1-}" in
  mcp|plugin|plugins|models|help|install|update|agent|agents|changelog|remote-control|mic-serve) exec "$REAL" "$@" ;;
esac
has_model=0
for arg in "$@"; do
  case "$arg" in
    --model|--model=*) has_model=1 ;;
  esac
done
if [ -n "$MODEL" ] && [ "$has_model" -eq 0 ]; then
  exec "$REAL" --model "$MODEL" "$@"
fi
"#
    };
    let script = format!(
        r#"#!/bin/sh
# {WRAPPER_MARK}
DIR="$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)"
if [ -f "$DIR/env" ]; then
  unset GEMINI_API_KEY GOOGLE_GEMINI_BASE_URL AGY_MODEL
  set -a
  # shellcheck disable=SC1091
  . "$DIR/env"
  set +a
fi
MODEL="${{AGY_MODEL-}}"
unset AGY_MODEL
DEFAULT_REAL={real}
REAL="${{AGY_REAL_BIN:-$DEFAULT_REAL}}"
if [ ! -x "$REAL" ]; then
  echo "agy: real binary not found at $REAL" >&2
  exit 127
fi
{model_flag}exec "$REAL" "$@"
"#,
        real = shell_quote(&real.to_string_lossy()),
    );
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    write_text_file(path, &script)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)
            .map_err(|e| AppError::io(path, e))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).map_err(|e| AppError::io(path, e))?;
    }
    Ok(())
}

pub(crate) fn real_agy_bin(wrapper: &Path) -> PathBuf {
    let home_bin = get_home_dir().join(".local").join("bin").join("agy");
    if home_bin.is_file() && !is_wrapper(&home_bin) {
        return home_bin;
    }
    if let Some(found) = command_v_agy(wrapper) {
        if !is_wrapper(&found) {
            return found;
        }
    }
    home_bin
}

fn is_wrapper(path: &Path) -> bool {
    use std::io::Read;
    let mut prefix = [0; 256];
    fs::File::open(path)
        .and_then(|mut file| file.read(&mut prefix))
        .map(|len| String::from_utf8_lossy(&prefix[..len]).contains(WRAPPER_MARK))
        .unwrap_or(false)
}

fn command_v_agy(wrapper: &Path) -> Option<PathBuf> {
    let output = Command::new("sh")
        .arg("-c")
        .arg("command -v agy")
        .env("PATH", stripped_path(wrapper.parent().unwrap_or(wrapper)))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let path = PathBuf::from(text.trim());
    path.is_file().then_some(path)
}

fn stripped_path(wrapper_dir: &Path) -> String {
    let current = std::env::var("PATH").unwrap_or_default();
    current
        .split(':')
        .filter(|entry| Path::new(entry) != wrapper_dir)
        .collect::<Vec<_>>()
        .join(":")
}

fn ensure_wrapper_on_path(bin_dir: &Path) -> Result<(), AppError> {
    let home = get_home_dir();
    let shell = std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(target_os = "macos") {
            "/bin/zsh".into()
        } else {
            "/bin/sh".into()
        }
    });
    match Path::new(&shell).file_name().and_then(|name| name.to_str()) {
        Some("zsh") => append_path_entry(&home.join(".zshenv"), bin_dir),
        Some("bash") => {
            append_path_entry(&home.join(".bashrc"), bin_dir)?;
            let login = [".bash_profile", ".bash_login", ".profile"]
                .into_iter()
                .map(|name| home.join(name))
                .find(|path| path.exists())
                .unwrap_or_else(|| home.join(".profile"));
            append_path_entry(&login, bin_dir)
        }
        Some("sh" | "dash" | "ksh") => append_path_entry(&home.join(".profile"), bin_dir),
        _ => Err(AppError::Config(
            "Automatic agy launcher setup supports zsh, bash and POSIX sh".into(),
        )),
    }
}

fn append_path_entry(zshenv: &Path, bin_dir: &Path) -> Result<(), AppError> {
    let existing = if zshenv.exists() {
        fs::read_to_string(zshenv).map_err(|e| AppError::io(zshenv, e))?
    } else {
        String::new()
    };
    if existing.contains(PATH_MARK) {
        return Ok(());
    }
    let line = format!(
        "\n# {PATH_MARK}\nexport PATH={}:\"$PATH\"\n",
        shell_quote(&bin_dir.to_string_lossy())
    );
    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(&line);
    write_text_file(zshenv, &next)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

fn read_env_file(path: &Path) -> Result<std::collections::HashMap<String, String>, AppError> {
    let mut map = std::collections::HashMap::new();
    if !path.exists() {
        return Ok(map);
    }
    let text = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        map.insert(key.to_string(), unquote(value));
    }
    Ok(map)
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    if let Some(inner) = value.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        return inner.replace(r"'\''", "'");
    }
    value.trim_matches('"').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_sets_provider_and_keeps_other_settings() {
        let dir = std::env::temp_dir().join(format!("cc-switch-agy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("settings.json"),
            "{\n  \"trustedWorkspaces\": [\n    \"/Users/me\"\n  ]\n}\n",
        )
        .unwrap();

        apply_in(
            &dir,
            &json!({
                "env": {
                    "GEMINI_API_KEY": "test-key",
                    "GOOGLE_GEMINI_BASE_URL": "https://example.test"
                },
                "config": { "model": { "name": "gemini-3.5-flash" } }
            }),
            false,
        )
        .unwrap();

        let settings: Value =
            serde_json::from_str(&fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        assert_eq!(settings["modelProvider"], "gemini");
        assert_eq!(settings["model"], "gemini-3.5-flash");
        assert_eq!(settings["trustedWorkspaces"][0], "/Users/me");

        let env = fs::read_to_string(dir.join("env")).unwrap();
        assert!(env.contains("GEMINI_API_KEY='test-key'"));
        assert!(env.contains("GOOGLE_GEMINI_BASE_URL='https://example.test'"));
        assert!(env.contains("AGY_MODEL='gemini-3.5-flash'"));

        let wrapper = fs::read_to_string(dir.join("bin").join("agy")).unwrap();
        assert!(wrapper.contains(WRAPPER_MARK));
        assert!(wrapper.contains("GEMINI_API_KEY") || wrapper.contains("$DIR/env"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_key_removes_model_provider() {
        let dir = std::env::temp_dir().join(format!("cc-switch-agy-login-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("settings.json"),
            r#"{"modelProvider":"gemini","model":"old","trustedWorkspaces":["/Users/me"]}"#,
        )
        .unwrap();
        apply_in(&dir, &json!({ "env": {}, "config": {} }), false).unwrap();
        let settings: Value =
            serde_json::from_str(&fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        assert!(settings.get("modelProvider").is_none());
        assert!(settings.get("model").is_none());
        assert_eq!(settings["trustedWorkspaces"][0], "/Users/me");
        let env = fs::read_to_string(dir.join("env")).unwrap();
        assert!(!env.contains("GEMINI_API_KEY="));
        let _ = fs::remove_dir_all(&dir);
    }
}

#[cfg(all(test, unix))]
mod integration_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct HomeGuard(Option<std::ffi::OsString>);
    impl HomeGuard {
        fn set(path: &Path) -> Self {
            let guard = Self(std::env::var_os("CC_SWITCH_TEST_HOME"));
            std::env::set_var("CC_SWITCH_TEST_HOME", path);
            crate::settings::reload_settings().unwrap();
            guard
        }
    }
    impl Drop for HomeGuard {
        fn drop(&mut self) {
            crate::mode::operation::failpoint::crash_at(None);
            match self.0.take() {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            let _ = crate::settings::reload_settings();
        }
    }

    #[test]
    fn agy_wrapper_quotes_keys_keeps_explicit_models_and_clears_stale_auth() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("space ' $dir");
        let key = "fixture'$(echo not-a-command)`echo no`";
        apply_in(&root,&json!({"env":{"GEMINI_API_KEY":key,"GOOGLE_GEMINI_BASE_URL":"https://example.test"},"config":{"model":{"name":"chosen"}}}),false).unwrap();
        let real = dir.path().join("fake agy");
        fs::write(&real,"#!/bin/sh\nprintf '%s\\n' \"${GEMINI_API_KEY-unset}\" \"${GOOGLE_GEMINI_BASE_URL-unset}\" \"$@\"\n").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o755)).unwrap();
        let run = |args: &[&str]| {
            let output = Command::new(root.join("bin/agy"))
                .args(args)
                .env("AGY_REAL_BIN", &real)
                .env("GEMINI_API_KEY", "stale")
                .env("GOOGLE_GEMINI_BASE_URL", "stale-url")
                .output()
                .unwrap();
            assert!(output.status.success());
            String::from_utf8(output.stdout).unwrap()
        };
        assert_eq!(
            run(&["--print", "hello"]),
            format!("{key}\nhttps://example.test\n--model\nchosen\n--print\nhello\n")
        );
        assert_eq!(
            run(&["--model=explicit"]),
            format!("{key}\nhttps://example.test\n--model=explicit\n")
        );
        assert_eq!(
            run(&["mcp", "list"]),
            format!("{key}\nhttps://example.test\nmcp\nlist\n")
        );
        assert_eq!(
            read_env_file(&root.join("env")).unwrap()["GEMINI_API_KEY"],
            key
        );
        assert_eq!(
            fs::metadata(root.join("env")).unwrap().permissions().mode() & 0o777,
            0o600
        );
        apply_in(&root, &json!({"env":{},"config":{}}), false).unwrap();
        assert_eq!(run(&["--print", "hi"]), "unset\nunset\n--print\nhi\n");
    }

    #[test]
    fn agy_shell_setup_is_idempotent_and_preserves_existing_lines() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join(".zshenv");
        fs::write(&profile, "# user settings\nexport KEEP=yes\n").unwrap();
        append_path_entry(&profile, &dir.path().join("quoted ' bin")).unwrap();
        let first = fs::read_to_string(&profile).unwrap();
        append_path_entry(&profile, &dir.path().join("quoted ' bin")).unwrap();
        assert_eq!(first, fs::read_to_string(&profile).unwrap());
        assert!(first.starts_with("# user settings\nexport KEEP=yes\n"));
        assert!(Command::new("sh")
            .arg("-n")
            .arg(&profile)
            .status()
            .unwrap()
            .success());
    }

    #[test]
    #[serial_test::serial]
    fn agy_switch_recovers_interrupted_files_and_pointer() {
        use crate::app_config::AppType;
        use crate::mode::{
            current::{provider_for, Purpose},
            operation::{failpoint, AppWrite},
        };
        use crate::{database::Database, provider::Provider, services::provider::agy_direct};
        let dir = tempfile::tempdir().unwrap();
        let _home = HomeGuard::set(dir.path());
        let db = Database::memory().unwrap();
        fs::create_dir_all(agy_dir()).unwrap();
        fs::write(
            settings_path(),
            r#"{"trustedWorkspaces":["keep"],"theme":"dark"}"#,
        )
        .unwrap();
        // Sibling Gemini files are never targets of an Antigravity switch.
        let sibling = dir.path().join(".gemini/settings.json");
        fs::write(&sibling, "{\"keep\":true}").unwrap();
        let row = Provider::with_id(
            "ag-test".into(),
            "Relay".into(),
            json!({"env":{"GEMINI_API_KEY":"fixture","GOOGLE_GEMINI_BASE_URL":"https://example.test"},"config":{"model":{"name":"model-one"}}}),
            None,
        );
        db.save_provider("agy", &row).unwrap();
        failpoint::crash_at(Some("published:0"));
        let result = agy_direct::switch_to(&db, &row);
        failpoint::crash_at(None);
        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(settings_path()).unwrap(),
            r#"{"trustedWorkspaces":["keep"],"theme":"dark"}"#
        );
        // begin completes the partially published operation and requests a retry.
        assert!(AppWrite::begin(&db, "agy").is_err());
        agy_direct::switch_to(&db, &row).unwrap();
        assert_eq!(
            provider_for(&db, &AppType::Agy, Purpose::Direct)
                .unwrap()
                .as_deref(),
            Some("ag-test")
        );
        assert_eq!(
            read_live_settings().unwrap()["config"]["model"]["name"],
            "model-one"
        );
        let doc: Value = serde_json::from_slice(&fs::read(settings_path()).unwrap()).unwrap();
        assert_eq!(doc["theme"], "dark");
        assert_eq!(doc["trustedWorkspaces"][0], "keep");
        assert_eq!(fs::read_to_string(&sibling).unwrap(), "{\"keep\":true}");
        fs::write(settings_path(), "broken").unwrap();
        let old_env = fs::read(env_path()).unwrap();
        assert!(agy_direct::switch_to(&db, &row).is_err());
        assert_eq!(fs::read(env_path()).unwrap(), old_env);
        assert_eq!(fs::read_to_string(settings_path()).unwrap(), "broken");
        assert!(read_live_settings().is_err());
    }
}
