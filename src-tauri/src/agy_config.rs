//! Antigravity CLI (`agy`) 的 live 配置。
//!
//! `agy` 只从进程环境读取 `GEMINI_API_KEY` 和 `GOOGLE_GEMINI_BASE_URL`，不读
//! `.env`。`settings.json` 只负责 `modelProvider` 和 `model`。切换供应商时：
//!
//! 1. 按键补丁写入 `~/.gemini/antigravity-cli/settings.json`，其余键不动。
//! 2. 把 Key 和地址写进同目录的 `env`（权限 0600）。
//! 3. 安装 `bin/agy` 包装脚本，启动时注入这些变量再执行真正的 `agy`。
//! 4. 在 zsh 的各启动阶段把包装目录放到 `PATH` 最前面，避免后续启动文件覆盖。
//!
//! 只有当前供应商带 Key 或自定义端点（第三方）时才接管启动器和 shell 配置；
//! 官方登录模式、删除供应商或空配置都不会安装它们，且会清掉此前由 CC Switch
//! 写入的带标记启动器与 PATH 片段（用户自己的内容原样保留）。

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

impl AgyProjection {
    /// 注入 Key 或自定义端点才需要启动器；仅模型由 settings.json 原生承载。
    fn needs_launcher(&self) -> bool {
        self.api_key.is_some() || self.base_url.is_some()
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

/// 配置里是否带有值得托管的真实内容（Key、端点或模型）。
pub(crate) fn has_managed_content(settings: &Value) -> bool {
    let projection = AgyProjection::of(settings);
    projection.api_key.is_some() || projection.base_url.is_some() || projection.model.is_some()
}

/// 这份配置是否需要 CC Switch 的启动器（带 Key 或自定义端点）。
pub(crate) fn needs_launcher(settings: &Value) -> bool {
    AgyProjection::of(settings).needs_launcher()
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
    let projection = AgyProjection::of(settings);
    let launcher = projection.needs_launcher();
    if launcher && !cfg!(unix) {
        return Err(AppError::Config(
            "Antigravity provider switching currently requires a POSIX shell (macOS/Linux)".into(),
        ));
    }
    let write = AppWrite::begin(db, "agy")?;
    let patch = settings_patch(&projection);
    // Validate before installing the launcher or publishing any changes.
    let path = settings_path();
    let pre = if path.exists() {
        Some(fs::read(&path).map_err(|e| AppError::io(&path, e))?)
    } else {
        None
    };
    patch.apply(&path, pre.as_deref()).map_err(AppError::from)?;
    if launcher {
        install_wrapper(&agy_dir().join("bin/agy"), &projection)?;
        ensure_wrapper_on_path(&agy_dir().join("bin"))?;
    }
    let env = WholeFile::Write(env_bytes(&projection));
    let report = write.run(
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
    )?;
    if !launcher {
        // 官方登录模式不需要注入环境，撤掉之前装的启动器和 PATH 片段。
        remove_launcher()?;
    }
    Ok(report)
}

/// 撤销 CC Switch 安装的启动器和 shell PATH 片段，只动带标记的内容。
pub(crate) fn remove_launcher() -> Result<(), AppError> {
    let bin_dir = agy_dir().join("bin");
    let wrapper = bin_dir.join("agy");
    if wrapper.is_file() && is_wrapper(&wrapper) {
        fs::remove_file(&wrapper).map_err(|e| AppError::io(&wrapper, e))?;
    }
    // 目录为空才会删除；里面有别的东西时保持原样。
    let _ = fs::remove_dir(&bin_dir);
    let home = get_home_dir();
    for profile in shell_profiles(&home) {
        remove_path_entry(&profile)?;
    }
    Ok(())
}

fn install_wrapper(path: &Path, _projection: &AgyProjection) -> Result<(), AppError> {
    if path.exists() && !is_wrapper(path) {
        return Err(AppError::Config(
            "Refusing to overwrite an unmanaged agy launcher".into(),
        ));
    }
    // 安装时找不到真实 agy（例如从 Dock 启动时 PATH 很短）也能装：启动器在运行时
    // 会按终端里的 PATH 重新查找，找不到就像普通的 command not found。
    let real = real_agy_bin(path)
        .map(|found| found.to_string_lossy().into_owned())
        .unwrap_or_default();
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
if [ -z "$REAL" ] || [ ! -x "$REAL" ]; then
  # Resolve at run time, skipping this launcher (and any other CC Switch launcher).
  REAL=""
  SEARCH="$PATH:$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin"
  old_ifs="$IFS"
  IFS=:
  for d in $SEARCH; do
    [ -n "$d" ] || continue
    c="$d/agy"
    [ -f "$c" ] && [ -x "$c" ] || continue
    [ "$(CDPATH= cd -- "$d" 2>/dev/null && pwd)" = "$DIR/bin" ] && continue
    if head -c 256 "$c" 2>/dev/null | grep -q {WRAPPER_MARK}; then continue; fi
    REAL="$c"
    break
  done
  IFS="$old_ifs"
fi
if [ -z "$REAL" ]; then
  echo "agy: command not found (real binary could not be located)" >&2
  exit 127
fi
{model_flag}exec "$REAL" "$@"
"#,
        real = shell_quote(&real),
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

/// 定位真实的 `agy`：先 PATH，再常见安装目录，最后问一次登录 shell。
/// 从 Dock / Finder 启动时 PATH 只有系统目录，所以后两步不可省。
/// 返回 `None` 表示确实找不到；自己的启动器永远不会被返回。
pub(crate) fn real_agy_bin(wrapper: &Path) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    resolve_real_agy(
        wrapper.parent(),
        &path_var,
        &get_home_dir(),
        &[
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ],
        !cfg!(test) && cfg!(unix),
    )
}

fn resolve_real_agy(
    wrapper_dir: Option<&Path>,
    path_var: &std::ffi::OsStr,
    home: &Path,
    system_dirs: &[PathBuf],
    probe_login_shell: bool,
) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::split_paths(path_var)
        .filter(|dir| !dir.as_os_str().is_empty())
        .collect();
    dirs.push(home.join(".local").join("bin"));
    dirs.extend(system_dirs.iter().cloned());
    dirs.push(home.join(".gemini").join("antigravity-cli").join("bin"));
    let names: &[&str] = if cfg!(windows) {
        &["agy.exe", "agy.cmd", "agy"]
    } else {
        &["agy"]
    };
    for dir in dirs {
        if wrapper_dir == Some(dir.as_path()) {
            continue;
        }
        for name in names {
            let candidate = dir.join(name);
            if is_real_binary(&candidate) {
                return Some(candidate);
            }
        }
    }
    if probe_login_shell {
        return login_shell_agy().filter(|found| is_real_binary(found));
    }
    None
}

fn is_real_binary(path: &Path) -> bool {
    if !path.is_file() || is_wrapper(path) {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|meta| meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn is_wrapper(path: &Path) -> bool {
    use std::io::Read;
    let mut prefix = [0; 256];
    fs::File::open(path)
        .and_then(|mut file| file.read(&mut prefix))
        .map(|len| String::from_utf8_lossy(&prefix[..len]).contains(WRAPPER_MARK))
        .unwrap_or(false)
}

/// 在登录 shell 里 `command -v agy`，限时 2 秒，避免用户的 rc 文件卡住界面。
fn login_shell_agy() -> Option<PathBuf> {
    use std::process::Stdio;
    let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty())?;
    let mut child = Command::new(shell)
        .args(["-l", "-c", "command -v agy"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match child.try_wait().ok()? {
            Some(status) if status.success() => break,
            Some(_) => return None,
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(std::time::Duration::from_millis(25)),
        }
    }
    let output = child.wait_with_output().ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let path = PathBuf::from(text.lines().rev().find(|l| !l.trim().is_empty())?.trim());
    path.is_absolute().then_some(path)
}

fn ensure_wrapper_on_path(bin_dir: &Path) -> Result<(), AppError> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(target_os = "macos") {
            "/bin/zsh".into()
        } else {
            "/bin/sh".into()
        }
    });
    ensure_wrapper_on_path_in(&get_home_dir(), &shell, bin_dir)
}

fn ensure_wrapper_on_path_in(home: &Path, shell: &str, bin_dir: &Path) -> Result<(), AppError> {
    match Path::new(shell).file_name().and_then(|name| name.to_str()) {
        Some("zsh") => {
            // macOS path_helper and user startup files can reset PATH after .zshenv.
            for name in [".zshenv", ".zprofile", ".zshrc"] {
                append_path_entry(&home.join(name), bin_dir)?;
            }
            let login = home.join(".zlogin");
            if login.exists() {
                append_path_entry(&login, bin_dir)?;
            }
            Ok(())
        }
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

/// 可能被写入过 PATH 片段的所有 shell 配置文件。
fn shell_profiles(home: &Path) -> Vec<PathBuf> {
    [
        ".zshenv",
        ".zprofile",
        ".zshrc",
        ".zlogin",
        ".bashrc",
        ".bash_profile",
        ".bash_login",
        ".profile",
    ]
    .into_iter()
    .map(|name| home.join(name))
    .collect()
}

fn path_entry_block(bin_dir: &Path) -> String {
    format!(
        "\n# {PATH_MARK}\nexport PATH={}:\"$PATH\"\n",
        shell_quote(&bin_dir.to_string_lossy())
    )
}

/// Only remove the exact block for our wrapper directory, preserving all other bytes.
fn remove_path_entry(profile: &Path) -> Result<(), AppError> {
    if !profile.is_file() {
        return Ok(());
    }
    let existing = fs::read_to_string(profile).map_err(|e| AppError::io(profile, e))?;
    let block = path_entry_block(&agy_dir().join("bin"));
    if !existing.contains(&block) {
        return Ok(());
    }
    let next = existing.replace(&block, "");
    if next.is_empty() {
        fs::remove_file(profile).map_err(|e| AppError::io(profile, e))
    } else {
        write_text_file(profile, &next)
    }
}

fn append_path_entry(zshenv: &Path, bin_dir: &Path) -> Result<(), AppError> {
    let existing = if zshenv.exists() {
        fs::read_to_string(zshenv).map_err(|e| AppError::io(zshenv, e))?
    } else {
        String::new()
    };
    let line = path_entry_block(bin_dir);
    if existing.contains(&line) {
        return Ok(());
    }
    // The block includes its separator; another newline would remain after cleanup.
    let mut next = existing;
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

#[cfg(all(test, unix))]
mod launcher_lifecycle_tests {
    use super::*;
    use crate::app_config::AppType;
    use crate::database::Database;
    use crate::provider::Provider;
    use crate::services::provider::agy_direct;
    use crate::store::AppState;
    use serial_test::serial;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;

    struct EnvGuard(&'static str, Option<std::ffi::OsString>);
    impl EnvGuard {
        fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self(key, previous)
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.1.take() {
                Some(v) => std::env::set_var(self.0, v),
                None => std::env::remove_var(self.0),
            }
        }
    }

    struct Home {
        dir: tempfile::TempDir,
        _home: EnvGuard,
        _shell: EnvGuard,
        _reload: Reload,
    }
    struct Reload;
    impl Drop for Reload {
        fn drop(&mut self) {
            // Runs after the env guards above have restored the variables.
            let _ = crate::settings::reload_settings();
        }
    }
    impl Home {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let home = EnvGuard::set("CC_SWITCH_TEST_HOME", dir.path());
            crate::settings::reload_settings().unwrap();
            let shell = EnvGuard::set("SHELL", "/bin/zsh");
            Self {
                dir,
                _home: home,
                _shell: shell,
                _reload: Reload,
            }
        }
        fn zshenv(&self) -> PathBuf {
            self.dir.path().join(".zshenv")
        }
    }
    impl Drop for Home {
        fn drop(&mut self) {
            crate::mode::operation::failpoint::crash_at(None);
        }
    }

    fn provider(id: &str, settings: Value, official: bool) -> Provider {
        let mut p = Provider::with_id(id.into(), id.into(), settings, None);
        if official {
            p.category = Some("official".into());
        }
        p
    }

    fn managed(id: &str) -> Provider {
        provider(
            id,
            json!({"env":{"GEMINI_API_KEY":"fixture","GOOGLE_GEMINI_BASE_URL":"https://example.test"},"config":{"model":{"name":"m"}}}),
            false,
        )
    }

    fn empty_agy_settings(home: &Home) {
        fs::create_dir_all(agy_dir()).unwrap();
        fs::write(settings_path(), r#"{"theme":"dark"}"#).unwrap();
        fs::write(home.zshenv(), "# user\nexport KEEP=1\n").unwrap();
    }

    #[test]
    #[serial]
    fn existing_empty_agy_settings_are_not_imported_or_synced_into_a_launcher() {
        let home = Home::new();
        empty_agy_settings(&home);
        let db = Arc::new(Database::memory().unwrap());
        let state = AppState::new(db.clone());

        // Startup import skips agy's own first-run settings.json.
        assert!(
            !crate::services::ProviderService::import_default_config(&state, AppType::Agy).unwrap()
        );
        assert!(db.get_all_providers("agy").unwrap().is_empty());

        // A stored provider without managed content is not applied either.
        let blank = provider("blank", json!({"env":{},"config":{}}), false);
        db.save_provider("agy", &blank).unwrap();
        db.set_current_provider("agy", "blank").unwrap();
        agy_direct::reapply(&db, &blank).unwrap();
        // The full sync used by import / cloud sync must report no agy failure.
        let result = crate::services::ProviderService::sync_current_to_live(&state);
        if let Err(error) = &result {
            assert!(!error.to_string().contains("provider/agy"), "{error}");
        }

        assert!(!agy_dir().join("bin/agy").exists());
        assert!(!env_path().exists());
        assert_eq!(
            fs::read_to_string(home.zshenv()).unwrap(),
            "# user\nexport KEEP=1\n"
        );
        for name in [".bashrc", ".profile", ".bash_profile"] {
            assert!(!home.dir.path().join(name).exists());
        }
        assert_eq!(
            fs::read_to_string(settings_path()).unwrap(),
            r#"{"theme":"dark"}"#
        );
    }

    #[test]
    #[serial]
    fn settings_with_real_content_are_still_imported() {
        let home = Home::new();
        empty_agy_settings(&home);
        fs::write(settings_path(), r#"{"model":"gemini-x"}"#).unwrap();
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        assert!(
            crate::services::ProviderService::import_default_config(&state, AppType::Agy).unwrap()
        );
    }

    #[test]
    #[serial]
    fn login_switch_and_delete_remove_only_our_launcher_and_path_block() {
        let home = Home::new();
        fs::create_dir_all(agy_dir()).unwrap();
        fs::write(home.zshenv(), "# user\nexport KEEP=1\n").unwrap();
        let db = Database::memory().unwrap();

        let relay = managed("relay");
        db.save_provider("agy", &relay).unwrap();
        agy_direct::switch_to(&db, &relay).unwrap();
        let wrapper = agy_dir().join("bin/agy");
        assert!(wrapper.is_file());
        assert!(fs::read_to_string(home.zshenv())
            .unwrap()
            .contains(PATH_MARK));

        // Switching to login mode removes both, keeps user content.
        let official = provider("official", json!({"env":{},"config":{}}), true);
        db.save_provider("agy", &official).unwrap();
        agy_direct::switch_to(&db, &official).unwrap();
        assert!(!wrapper.exists());
        assert!(!agy_dir().join("bin").exists());
        assert_eq!(
            fs::read_to_string(home.zshenv()).unwrap(),
            "# user\nexport KEEP=1\n"
        );

        // Reinstall, then deleting the last managed provider releases the launcher.
        agy_direct::switch_to(&db, &relay).unwrap();
        assert!(wrapper.is_file());
        agy_direct::release_launcher_if_unmanaged(&db).unwrap();
        assert!(
            wrapper.is_file(),
            "a managed current provider keeps the launcher"
        );
        db.set_current_provider("agy", "official").unwrap();
        db.delete_provider("agy", "relay").unwrap();
        agy_direct::release_launcher_if_unmanaged(&db).unwrap();
        assert!(!wrapper.exists());
        assert_eq!(
            fs::read_to_string(home.zshenv()).unwrap(),
            "# user\nexport KEEP=1\n"
        );
    }

    #[test]
    #[serial]
    fn cleanup_never_touches_an_unmanaged_launcher_or_foreign_profile_lines() {
        let home = Home::new();
        let bin = agy_dir().join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("agy"), "#!/bin/sh\necho user script\n").unwrap();
        fs::write(
            home.zshenv(),
            "export PATH=\"$HOME/x:$PATH\"\n# my own note about cc-switch agy wrapper\n",
        )
        .unwrap();
        remove_launcher().unwrap();
        assert!(bin.join("agy").exists());
        assert_eq!(
            fs::read_to_string(home.zshenv()).unwrap(),
            "export PATH=\"$HOME/x:$PATH\"\n# my own note about cc-switch agy wrapper\n"
        );
    }

    #[test]
    #[serial]
    fn path_block_removal_covers_bash_profiles_and_deletes_files_we_created() {
        let home = Home::new();
        let bin = agy_dir().join("bin");
        ensure_wrapper_on_path_in(home.dir.path(), "/bin/bash", &bin).unwrap();
        assert!(home.dir.path().join(".bashrc").exists());
        assert!(home.dir.path().join(".profile").exists());
        remove_launcher().unwrap();
        assert!(!home.dir.path().join(".bashrc").exists());
        assert!(!home.dir.path().join(".profile").exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[serial]
    fn agy_fresh_zsh_keeps_launcher_after_path_resets_and_restores_profiles() {
        let home = Home::new();
        let profiles = [".zshenv", ".zprofile", ".zshrc", ".zlogin"];
        let original = "# user PATH reset\nexport PATH=\"$HOME/.local/bin:/usr/bin:/bin\"";
        for name in profiles {
            fs::write(home.dir.path().join(name), original).unwrap();
        }
        let real = fake_agy(&home.dir.path().join(".local/bin"));
        fs::write(
            &real,
            "#!/bin/sh\nprintf '%s\\n' \"$GEMINI_API_KEY\" \"$GOOGLE_GEMINI_BASE_URL\" \"$@\"\n",
        )
        .unwrap();
        let db = Database::memory().unwrap();
        let relay = managed("relay");
        db.save_provider("agy", &relay).unwrap();
        agy_direct::switch_to(&db, &relay).unwrap();
        agy_direct::reapply(&db, &relay).unwrap();
        let wrapper = agy_dir().join("bin/agy");
        for flags in ["-lc", "-lic"] {
            let output = Command::new("/bin/zsh")
                .args([flags, "command -v agy; agy --print fixture"])
                .env("HOME", home.dir.path())
                .env("ZDOTDIR", home.dir.path())
                .env("AGY_REAL_BIN", &real)
                .env_remove("GEMINI_API_KEY")
                .env_remove("GOOGLE_GEMINI_BASE_URL")
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                format!(
                    "{}\nfixture\nhttps://example.test\n--model\nm\n--print\nfixture\n",
                    wrapper.display()
                ),
                "launcher must survive {flags} startup"
            );
        }
        for name in profiles {
            assert_eq!(
                fs::read_to_string(home.dir.path().join(name))
                    .unwrap()
                    .matches(&format!("# {PATH_MARK}"))
                    .count(),
                1
            );
        }
        let login = provider("login", json!({"env":{},"config":{}}), true);
        db.save_provider("agy", &login).unwrap();
        agy_direct::switch_to(&db, &login).unwrap();
        for name in profiles {
            assert_eq!(
                fs::read_to_string(home.dir.path().join(name)).unwrap(),
                original
            );
        }
        assert!(!wrapper.exists());
    }

    #[test]
    #[serial]
    fn cleanup_preserves_whitespace_and_foreign_marked_path_entries() {
        let home = Home::new();
        let bin = agy_dir().join("bin");
        for original in [
            "# user without final newline",
            " \n\t\n",
            "\n# cc-switch agy wrapper\nexport PATH='/some/other/bin':\"$PATH\"\n",
        ] {
            fs::write(home.zshenv(), original).unwrap();
            append_path_entry(&home.zshenv(), &bin).unwrap();
            assert!(fs::read_to_string(home.zshenv())
                .unwrap()
                .contains(&shell_quote(&bin.to_string_lossy())));
            remove_launcher().unwrap();
            assert_eq!(fs::read_to_string(home.zshenv()).unwrap(), original);
        }
    }

    fn fake_agy(dir: &Path) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join("agy");
        fs::write(&path, "#!/bin/sh\nprintf 'real:%s\\n' \"$@\"\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn resolver_searches_common_dirs_when_the_gui_path_is_minimal() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let wrapper_dir = home.join(".gemini/antigravity-cli/bin");
        let gui_path = std::ffi::OsString::from("/usr/bin:/bin:/usr/sbin:/sbin");
        let brew = tmp.path().join("brew");

        // Nothing installed: no fabricated ~/.local/bin/agy fallback.
        assert_eq!(
            resolve_real_agy(
                Some(&wrapper_dir),
                &gui_path,
                &home,
                std::slice::from_ref(&brew),
                false
            ),
            None
        );
        // Homebrew-style location.
        let found = fake_agy(&brew);
        assert_eq!(
            resolve_real_agy(
                Some(&wrapper_dir),
                &gui_path,
                &home,
                std::slice::from_ref(&brew),
                false
            ),
            Some(found)
        );
        // ~/.local/bin wins over system dirs.
        let local = fake_agy(&home.join(".local/bin"));
        assert_eq!(
            resolve_real_agy(Some(&wrapper_dir), &gui_path, &home, &[brew], false),
            Some(local)
        );
    }

    #[test]
    fn resolver_skips_our_wrapper_and_non_executables() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let wrapper_dir = home.join(".gemini/antigravity-cli/bin");
        fs::create_dir_all(&wrapper_dir).unwrap();
        let wrapper = wrapper_dir.join("agy");
        fs::write(&wrapper, format!("#!/bin/sh\n# {WRAPPER_MARK}\n")).unwrap();
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
        // A marked wrapper elsewhere on PATH is not the real binary either.
        let other = tmp.path().join("other");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("agy"), format!("# {WRAPPER_MARK}\n")).unwrap();
        fs::set_permissions(other.join("agy"), fs::Permissions::from_mode(0o755)).unwrap();
        // A non-executable file is ignored.
        let plain = tmp.path().join("plain");
        fs::create_dir_all(&plain).unwrap();
        fs::write(plain.join("agy"), "binary").unwrap();
        let real_dir = tmp.path().join("real");
        let real = fake_agy(&real_dir);
        let path = std::env::join_paths([&wrapper_dir, &other, &plain, &real_dir]).unwrap();
        assert_eq!(
            resolve_real_agy(Some(&wrapper_dir), &path, &home, &[], false),
            Some(real)
        );
    }

    #[test]
    fn launcher_resolves_the_real_binary_at_run_time_without_recursing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("agy-dir");
        install_wrapper(&root.join("bin/agy"), &AgyProjection::of(&json!({}))).unwrap();
        // Simulate an install done with a GUI PATH: nothing baked in.
        let script = fs::read_to_string(root.join("bin/agy")).unwrap();
        let script = script
            .lines()
            .map(|line| {
                if line.starts_with("DEFAULT_REAL=") {
                    "DEFAULT_REAL=''"
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(root.join("bin/agy"), script).unwrap();
        let real_dir = tmp.path().join("real");
        fake_agy(&real_dir);
        // The wrapper's own directory comes first on PATH: it must not exec itself.
        let path = std::env::join_paths([
            root.join("bin"),
            real_dir,
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
        ])
        .unwrap();
        let output = Command::new(root.join("bin/agy"))
            .arg("hello")
            .env("PATH", path)
            .env("HOME", tmp.path())
            .env_remove("AGY_REAL_BIN")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "real:hello\n");
    }
}
