//! Antigravity CLI (`agy`) 的 live 配置。
//!
//! `agy` 只从进程环境读取 `GEMINI_API_KEY` 和 `GOOGLE_GEMINI_BASE_URL`，`settings.json`
//! 只负责 `modelProvider` 和 `model`（只认显示名）。CC Switch 管不到用户自己敲的 `agy`，
//! 所以卡片没有「启用」，只有「打开终端」，每张卡只作用于它打开的那个终端：
//!
//! - **官方登录卡**：`agy --model=<显示名>`（没设模型就是 `agy`），模型只对这次会话生效。
//!   登录凭据由 agy 自己保管（不在 `~/.gemini` 里）。
//! - **第三方 Key 卡**：`agy --gemini_dir=<私有目录>`，私有目录在
//!   `~/.cc-switch/agy/<供应商>/`，它的 `antigravity-cli/settings.json` 写
//!   `modelProvider: gemini` 和模型，`config` 软链回 `~/.gemini/config`（MCP、Skills、
//!   提示词照常共享）；Key 和地址只经子进程环境传入。
//!
//! 真实的 `~/.gemini/antigravity-cli/settings.json` 一律不写，直接敲 `agy` 永远是官方登录，
//! 模型由 agy 自己的 `/model` 管。唯一的例外是撤掉 #8268 测试版留下的 `modelProvider`：
//! 测试版还装过包装脚本、改过 shell 启动文件，启动时由 [`remove_legacy_launcher`] 只删带
//! 标记的内容。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

use crate::config::{atomic_write_private, get_app_config_dir, get_home_dir, write_text_file};
use crate::error::AppError;
use crate::live::patch::json::JsonPatch;
use crate::live::patch::{KeyPath, LivePatch};

/// `~/.gemini/antigravity-cli`
pub fn agy_dir() -> PathBuf {
    get_home_dir().join(".gemini").join("antigravity-cli")
}

/// `~/.gemini/config`：CLI、桌面版和 IDE 共用的 MCP / Skills / 提示词目录。
pub fn customizations_dir() -> PathBuf {
    get_home_dir().join(".gemini").join("config")
}

pub fn settings_path() -> PathBuf {
    agy_dir().join("settings.json")
}

/// Key 模式各供应商私有 `--gemini_dir` 的上级目录。
pub fn key_profiles_root() -> PathBuf {
    get_app_config_dir().join("agy")
}

/// 某个供应商的私有 `--gemini_dir`。供应商 ID 不是安全的路径段时换成带哈希的名字，
/// 名字要跨版本稳定：会话记录就存在这个目录里。
pub fn key_profile_dir(provider_id: &str) -> PathBuf {
    key_profiles_root().join(profile_dir_name(provider_id))
}

fn profile_dir_name(provider_id: &str) -> String {
    let safe = !provider_id.is_empty()
        && provider_id != "."
        && provider_id != ".."
        && provider_id.len() <= 64
        && provider_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if safe {
        return provider_id.to_string();
    }
    let prefix: String = provider_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .take(32)
        .collect();
    format!("{prefix}-{:016x}", fnv1a(provider_id.as_bytes()))
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// 从 live 读回编辑器用的 `{ env, config }`。Key 不在任何文件里，所以 `env` 恒为空。
pub fn read_live_settings() -> Result<Value, AppError> {
    let settings_file = settings_path();
    if !settings_file.exists() {
        return Err(AppError::localized(
            "agy.config.missing",
            "Antigravity 配置文件不存在",
            "Antigravity configuration files are missing",
        ));
    }
    let raw = fs::read(&settings_file).map_err(|e| AppError::io(&settings_file, e))?;
    let settings: Value = serde_json::from_slice(&raw)
        .map_err(|e| AppError::Config(format!("Invalid agy settings.json: {e}")))?;
    let model = settings.get("model").cloned().filter(|v| !v.is_null());
    Ok(json!({
        "env": {},
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

    /// 带 Key 才是第三方模式；地址必须和 Key 一起出现（`agy_direct` 已校验）。
    fn is_key_mode(&self) -> bool {
        self.api_key.is_some()
    }
}

fn env_string(env: Option<&Value>, key: &str) -> Option<String> {
    env?.get(key)?.as_str().map(str::to_string)
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

/// 配置里是否带有值得导入的真实内容（Key、端点或模型）。
pub(crate) fn has_managed_content(settings: &Value) -> bool {
    let projection = AgyProjection::of(settings);
    projection.api_key.is_some() || projection.base_url.is_some() || projection.model.is_some()
}

/// 私有目录里 `settings.json` 的补丁：Key 模式加上卡上的模型。
fn key_profile_patch(projection: &AgyProjection) -> JsonPatch {
    let mut patch = JsonPatch::default();
    patch
        .set
        .push((KeyPath::new(&["modelProvider"]), json!("gemini")));
    let model = KeyPath::new(&["model"]);
    match &projection.model {
        Some(name) => patch.set.push((model, json!(name))),
        None => patch.remove.push(model),
    }
    patch
}

/// 撤掉 #8268 测试版写进真实 `settings.json` 的 `modelProvider`，模型和其余键不动。
/// 这是 CC Switch 对真实 `settings.json` 唯一的写入。
pub(crate) fn release_model_provider(
    db: &crate::database::Database,
) -> Result<crate::mode::operation::OperationReport, AppError> {
    use crate::live::engine::LiveFile;
    use crate::mode::operation::{AppWrite, FileChange};
    use crate::mode::state::{op, PendingTarget};
    let write = AppWrite::begin(db, "agy")?;
    let path = settings_path();
    if !path.exists() {
        return write.run(op::APPLY, &[], PendingTarget::default());
    }
    let pre = fs::read(&path).map_err(|e| AppError::io(&path, e))?;
    let mut patch = JsonPatch::default();
    patch.remove.push(KeyPath::new(&["modelProvider"]));
    // 先验证：文件坏了就停。
    patch.apply(&path, Some(&pre)).map_err(AppError::from)?;
    write.run(
        op::APPLY,
        &[FileChange {
            file: LiveFile::private(path),
            patch: &patch,
        }],
        PendingTarget::default(),
    )
}

/// 在终端里启动这张卡要用的参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgyLaunch {
    /// Key 模式的私有 `--gemini_dir`；官方登录卡为 `None`，直接跑原生 `agy`。
    pub gemini_dir: Option<PathBuf>,
    /// 官方登录卡的 `--model`（显示名，只对这次会话生效）；Key 卡的模型写在私有目录里，为 `None`。
    pub model: Option<String>,
    /// 只给这一个 agy 进程的环境变量；`None` 表示要清掉（用户 rc 里可能 export 过）。
    pub env: Vec<(&'static str, Option<String>)>,
}

/// 准备在终端里启动某张卡：Key 卡会建好（或更新）它的私有目录。
pub(crate) fn prepare_launch(provider_id: &str, settings: &Value) -> Result<AgyLaunch, AppError> {
    let projection = AgyProjection::of(settings);
    if !projection.is_key_mode() {
        return Ok(AgyLaunch {
            gemini_dir: None,
            model: projection.model,
            env: Vec::new(),
        });
    }
    let dir = key_profile_dir(provider_id);
    prepare_key_profile(&dir, &projection)?;
    Ok(AgyLaunch {
        gemini_dir: Some(dir),
        model: None,
        env: vec![
            ("GEMINI_API_KEY", projection.api_key.clone()),
            ("GOOGLE_GEMINI_BASE_URL", projection.base_url.clone()),
        ],
    })
}

fn prepare_key_profile(dir: &Path, projection: &AgyProjection) -> Result<(), AppError> {
    let settings = dir.join("antigravity-cli").join("settings.json");
    let pre = if settings.exists() {
        Some(fs::read(&settings).map_err(|e| AppError::io(&settings, e))?)
    } else {
        None
    };
    let bytes = key_profile_patch(projection)
        .apply(&settings, pre.as_deref())
        .map_err(AppError::from)?;
    if pre.as_deref() != Some(bytes.as_slice()) {
        if let Some(parent) = settings.parent() {
            fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }
        atomic_write_private(&settings, &bytes)?;
    }
    link_shared_config(&dir.join("config"), &customizations_dir())
}

/// `<私有目录>/config` 指向共享的 `~/.gemini/config`。已经是别的真实目录时不动它：
/// 那是 agy 自己建的，里面可能有用户的东西。
fn link_shared_config(link: &Path, target: &Path) -> Result<(), AppError> {
    fs::create_dir_all(target).map_err(|e| AppError::io(target, e))?;
    match fs::symlink_metadata(link) {
        Ok(meta) if meta.file_type().is_symlink() => {
            if fs::read_link(link).ok().as_deref() == Some(target) {
                return Ok(());
            }
            remove_link(link)?;
        }
        Ok(_) => {
            log::warn!(
                "agy key profile config is a real directory, leaving it as is: {}",
                link.display()
            );
            return Ok(());
        }
        Err(_) => {}
    }
    create_dir_link(target, link)
}

#[cfg(unix)]
fn create_dir_link(target: &Path, link: &Path) -> Result<(), AppError> {
    std::os::unix::fs::symlink(target, link).map_err(|e| AppError::io(link, e))
}

/// Windows 建符号链接要开发者模式，失败时退回目录联接（junction），普通用户也能建。
#[cfg(windows)]
fn create_dir_link(target: &Path, link: &Path) -> Result<(), AppError> {
    if std::os::windows::fs::symlink_dir(target, link).is_ok() {
        return Ok(());
    }
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let status = Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| AppError::io(link, e))?;
    if status.success() {
        Ok(())
    } else {
        Err(AppError::Message(format!(
            "无法创建目录链接: {}",
            link.display()
        )))
    }
}

fn remove_link(link: &Path) -> Result<(), AppError> {
    // Windows 上目录符号链接和 junction 要按目录删；Unix 上符号链接按文件删。
    fs::remove_file(link)
        .or_else(|_| fs::remove_dir(link))
        .map_err(|e| AppError::io(link, e))
}

/// 会话页的恢复命令。Key 模式的会话在私有目录里，要带上同一个 `--gemini_dir`；Key 本身
/// 不进命令（命令会被显示和复制），一键恢复时由 [`crate::services::provider::agy_direct::key_resume_target`]
/// 认出私有目录再补上。
pub fn resume_command(gemini_dir: Option<&Path>, conversation: &str) -> String {
    match gemini_dir {
        Some(dir) => format!(
            "agy {} --conversation={conversation}",
            shell_word(&format!("--gemini_dir={}", dir.to_string_lossy()))
        ),
        None => format!("agy --conversation={conversation}"),
    }
}

/// shell 里的一个词：只含安全字符时原样写，否则加单引号。
fn shell_word(value: &str) -> String {
    let plain = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '='));
    if plain {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', r"'\''"))
    }
}

/// 定位真实的 `agy`：先 PATH，再常见安装目录，最后问一次登录 shell。
/// 从 Dock / Finder 启动时 PATH 只有系统目录，所以后两步不可省。
/// 旧版测试装的包装脚本永远不会被返回。
pub(crate) fn real_agy_bin() -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    // install.ps1 installs to %LOCALAPPDATA%\agy\bin, which a GUI process's PATH may lack.
    let system_dirs: Vec<PathBuf> = if cfg!(windows) {
        dirs::data_local_dir()
            .map(|dir| dir.join("agy").join("bin"))
            .into_iter()
            .collect()
    } else {
        vec![
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ]
    };
    resolve_real_agy(
        &path_var,
        &get_home_dir(),
        &system_dirs,
        !cfg!(test) && cfg!(unix),
    )
}

fn resolve_real_agy(
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
    let names: &[&str] = if cfg!(windows) {
        &["agy.exe", "agy.cmd", "agy"]
    } else {
        &["agy"]
    };
    for dir in dirs {
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
    if !path.is_file() || legacy::is_wrapper(path) {
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

/// 撤掉 #8268 测试版装的启动器：`bin/agy` 包装脚本、带 Key 的 `env` 文件和各 shell
/// 启动文件里的 PATH 片段。只动带标记的内容。返回是否清掉过 `env`（说明当时可能写过
/// `modelProvider`，调用方要按新规则重写一次 settings.json）。
pub(crate) fn remove_legacy_launcher() -> Result<bool, AppError> {
    legacy::remove(&agy_dir(), &get_home_dir())
}

mod legacy {
    use super::*;

    pub(super) const WRAPPER_MARK: &str = "cc-switch-agy-wrapper";
    pub(super) const PATH_MARK: &str = "cc-switch agy wrapper";

    pub(super) fn remove(agy_dir: &Path, home: &Path) -> Result<bool, AppError> {
        let bin_dir = agy_dir.join("bin");
        let wrapper = bin_dir.join("agy");
        if wrapper.is_file() && is_wrapper(&wrapper) {
            fs::remove_file(&wrapper).map_err(|e| AppError::io(&wrapper, e))?;
        }
        // 目录为空才会删除；里面有别的东西时保持原样。
        let _ = fs::remove_dir(&bin_dir);
        let env = agy_dir.join("env");
        let had_env = is_marked_env(&env);
        if had_env {
            fs::remove_file(&env).map_err(|e| AppError::io(&env, e))?;
        }
        let block = path_entry_block(&bin_dir);
        for profile in shell_profiles(home) {
            remove_path_entry(&profile, &block)?;
        }
        Ok(had_env)
    }

    pub(super) fn is_wrapper(path: &Path) -> bool {
        head_contains(path, WRAPPER_MARK)
    }

    fn is_marked_env(path: &Path) -> bool {
        path.is_file() && head_contains(path, &format!("# {PATH_MARK}"))
    }

    fn head_contains(path: &Path, mark: &str) -> bool {
        use std::io::Read;
        let mut prefix = [0; 256];
        fs::File::open(path)
            .and_then(|mut file| file.read(&mut prefix))
            .map(|len| String::from_utf8_lossy(&prefix[..len]).contains(mark))
            .unwrap_or(false)
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

    pub(super) fn path_entry_block(bin_dir: &Path) -> String {
        format!(
            "\n# {PATH_MARK}\nexport PATH={}:\"$PATH\"\n",
            shell_quote(&bin_dir.to_string_lossy())
        )
    }

    /// 只删我们那个目录对应的整块，其余字节不动；只剩这块的文件当初是我们建的，整个删掉。
    fn remove_path_entry(profile: &Path, block: &str) -> Result<(), AppError> {
        if !profile.is_file() {
            return Ok(());
        }
        let existing = fs::read_to_string(profile).map_err(|e| AppError::io(profile, e))?;
        if !existing.contains(block) {
            return Ok(());
        }
        let next = existing.replace(block, "");
        if next.is_empty() {
            fs::remove_file(profile).map_err(|e| AppError::io(profile, e))
        } else {
            write_text_file(profile, &next)
        }
    }

    fn shell_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projection(settings: Value) -> AgyProjection {
        AgyProjection::of(&settings)
    }

    fn patched(patch: &JsonPatch, before: &str) -> Value {
        let bytes = patch
            .apply(Path::new("settings.json"), Some(before.as_bytes()))
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[test]
    fn key_profile_patch_sets_key_mode_and_the_card_model() {
        let p = projection(json!({
            "env":{"GEMINI_API_KEY":"fixture","GOOGLE_GEMINI_BASE_URL":"https://example.test"},
            "config":{"model":{"name":"Gemini 3.1 Pro (High)"}}
        }));
        let doc = patched(&key_profile_patch(&p), r#"{"theme":"dark"}"#);
        assert_eq!(doc["modelProvider"], "gemini");
        assert_eq!(doc["model"], "Gemini 3.1 Pro (High)");
        assert_eq!(doc["theme"], "dark");
        assert!(!serde_json::to_string(&doc).unwrap().contains("fixture"));
    }

    #[test]
    fn profile_dir_names_are_safe_and_stable() {
        assert_eq!(profile_dir_name("default"), "default");
        assert_eq!(profile_dir_name("3f2a-uuid_1.x"), "3f2a-uuid_1.x");
        for unsafe_id in ["", ".", "..", "a/b", "../x", "名字", "a b"] {
            let name = profile_dir_name(unsafe_id);
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')),
                "{name}"
            );
            assert_eq!(name, profile_dir_name(unsafe_id));
        }
        assert_ne!(profile_dir_name("a/b"), profile_dir_name("a_b"));
        // 名字落在会话目录上，算法变了会话就找不到了。
        assert_eq!(
            profile_dir_name("a/b"),
            format!("ab-{:016x}", fnv1a(b"a/b"))
        );
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}

#[cfg(all(test, unix))]
mod integration_tests {
    use super::*;
    use crate::app_config::AppType;
    use crate::database::Database;
    use crate::mode::{
        current::{provider_for, Purpose},
        operation::failpoint,
    };
    use crate::provider::Provider;
    use crate::services::provider::agy_direct;
    use crate::store::AppState;
    use serial_test::serial;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;

    struct HomeGuard(tempfile::TempDir, Option<std::ffi::OsString>);
    impl HomeGuard {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            crate::settings::reload_settings().unwrap();
            Self(dir, previous)
        }
        fn path(&self) -> &Path {
            self.0.path()
        }
    }
    impl Drop for HomeGuard {
        fn drop(&mut self) {
            failpoint::crash_at(None);
            match self.1.take() {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            let _ = crate::settings::reload_settings();
        }
    }

    fn provider(id: &str, settings: Value, official: bool) -> Provider {
        let mut p = Provider::with_id(id.into(), id.into(), settings, None);
        if official {
            p.category = Some("official".into());
        }
        p
    }

    fn relay(id: &str) -> Provider {
        provider(
            id,
            json!({"env":{"GEMINI_API_KEY":"fixture","GOOGLE_GEMINI_BASE_URL":"https://example.test"},"config":{"model":{"name":"Gemini 3.1 Pro (High)"}}}),
            false,
        )
    }

    fn read_json(path: &Path) -> Value {
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() && !path.is_symlink() {
                out.extend(walk(&path));
            } else if path.is_file() {
                out.push(path);
            }
        }
        out
    }

    #[test]
    #[serial]
    fn cards_never_write_the_real_settings_credentials_or_shell_files() {
        use crate::services::ProviderService;
        let home = HomeGuard::new();
        fs::create_dir_all(agy_dir()).unwrap();
        let original = r#"{"model":"Gemini 3.8 Flash (Medium)","theme":"dark"}"#;
        fs::write(settings_path(), original).unwrap();
        // Sibling Gemini files are never targets either.
        let sibling = home.path().join(".gemini/settings.json");
        fs::write(&sibling, "{\"keep\":true}").unwrap();
        let db = Arc::new(Database::memory().unwrap());
        let state = AppState::new(db.clone());
        let login = provider(
            "login",
            json!({"env":{},"config":{"model":{"name":"Gemini 3.1 Pro (High)"}}}),
            true,
        );
        let relay = relay("relay");
        for row in [&login, &relay] {
            assert!(ProviderService::add(&state, AppType::Agy, row.clone(), true).unwrap());
            let error = ProviderService::switch(&state, AppType::Agy, &row.id).unwrap_err();
            assert!(error.to_string().contains("Open terminal"), "{error}");
        }
        assert_eq!(
            provider_for(&db, &AppType::Agy, Purpose::Direct).unwrap(),
            None
        );

        // A pointer left by an older version is neither re-applied by a full
        // sync nor a reason to refuse deleting the card.
        db.set_current_provider("agy", "login").unwrap();
        if let Err(error) = ProviderService::sync_current_to_live(&state) {
            assert!(!error.to_string().contains("provider/agy"), "{error}");
        }
        ProviderService::delete(&state, AppType::Agy, "login").unwrap();

        prepare_launch(&relay.id, &relay.settings_config).unwrap();
        assert_eq!(fs::read_to_string(settings_path()).unwrap(), original);
        assert_eq!(fs::read_to_string(&sibling).unwrap(), "{\"keep\":true}");
        for file in walk(home.path()) {
            let text = fs::read(&file).unwrap_or_default();
            assert!(
                !String::from_utf8_lossy(&text).contains("fixture"),
                "key leaked into {}",
                file.display()
            );
        }
        assert!(!agy_dir().join("bin").exists());
        assert!(!agy_dir().join("env").exists());
        for name in [".zshenv", ".zprofile", ".zshrc", ".bashrc", ".profile"] {
            assert!(!home.path().join(name).exists());
        }
    }

    #[test]
    #[serial]
    fn key_launch_prepares_a_private_profile_linked_to_shared_config() {
        let _home = HomeGuard::new();
        let shared = customizations_dir();
        fs::create_dir_all(&shared).unwrap();
        fs::write(shared.join("mcp_config.json"), "{}").unwrap();
        let row = relay("relay");

        let launch = prepare_launch(&row.id, &row.settings_config).unwrap();
        let dir = key_profile_dir("relay");
        assert_eq!(launch.gemini_dir.as_deref(), Some(dir.as_path()));
        assert_eq!(
            launch.env,
            vec![
                ("GEMINI_API_KEY", Some("fixture".to_string())),
                (
                    "GOOGLE_GEMINI_BASE_URL",
                    Some("https://example.test".to_string())
                ),
            ]
        );
        let private = dir.join("antigravity-cli/settings.json");
        let doc = read_json(&private);
        assert_eq!(doc["modelProvider"], "gemini");
        assert_eq!(doc["model"], "Gemini 3.1 Pro (High)");
        assert_eq!(
            fs::metadata(&private).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(!String::from_utf8_lossy(&fs::read(&private).unwrap()).contains("fixture"));
        assert_eq!(fs::read_link(dir.join("config")).unwrap(), shared);
        assert!(dir.join("config/mcp_config.json").is_file());
        // The global prompt lives in the shared customization root, so the
        // private profile loads it too.
        let prompt = crate::prompt_files::prompt_file_path(&AppType::Agy).unwrap();
        assert_eq!(prompt, shared.join("AGENTS.md"));
        fs::write(&prompt, "rules").unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("config/AGENTS.md")).unwrap(),
            "rules"
        );

        // agy's own keys in the private settings survive later launches; a key
        // without an endpoint must clear any endpoint the user's shell exported.
        let mut doc = read_json(&private);
        doc["trustedWorkspaces"] = json!(["/w"]);
        fs::write(&private, serde_json::to_vec(&doc).unwrap()).unwrap();
        let google = provider(
            "relay",
            json!({"env":{"GEMINI_API_KEY":"fixture"},"config":{}}),
            false,
        );
        let launch = prepare_launch(&google.id, &google.settings_config).unwrap();
        assert_eq!(launch.env[1], ("GOOGLE_GEMINI_BASE_URL", None));
        let doc = read_json(&private);
        assert_eq!(doc["trustedWorkspaces"][0], "/w");
        assert!(doc.get("model").is_none());
    }

    #[test]
    #[serial]
    fn key_profile_sessions_are_listed_and_resume_with_their_provider() {
        let _home = HomeGuard::new();
        let db = Database::memory().unwrap();
        let row = relay("relay one");
        db.save_provider("agy", &row).unwrap();
        let launch = prepare_launch(&row.id, &row.settings_config).unwrap();
        let dir = launch.gemini_dir.unwrap();
        let logs = dir.join("antigravity-cli/brain/abc-123/.system_generated/logs");
        fs::create_dir_all(&logs).unwrap();
        fs::write(
            logs.join("transcript_full.jsonl"),
            r#"{"step_index":0,"type":"USER_INPUT","content":"hi"}"#,
        )
        .unwrap();

        let sessions = crate::session_manager::providers::agy::scan_sessions();
        assert_eq!(sessions.len(), 1);
        let command = sessions[0].resume_command.clone().unwrap();
        assert!(!command.contains("fixture"), "{command}");
        assert_eq!(command, resume_command(Some(&dir), "abc-123"));
        let (found, id) = agy_direct::key_resume_target(&db, &command)
            .unwrap()
            .unwrap();
        assert_eq!(found.id, "relay one");
        assert_eq!(id, "abc-123");
        // Login sessions and tampered ids are left to the plain command.
        assert!(
            agy_direct::key_resume_target(&db, "agy --conversation=abc-123")
                .unwrap()
                .is_none()
        );
        assert!(
            agy_direct::key_resume_target(&db, &format!("{command}; rm -rf /"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    #[serial]
    fn login_launch_uses_native_agy_without_a_private_profile() {
        let _home = HomeGuard::new();
        let row = provider(
            "login",
            json!({"env":{},"config":{"model":{"name":"Gemini 3.8 Flash (Low)"}}}),
            true,
        );
        let launch = prepare_launch(&row.id, &row.settings_config).unwrap();
        assert_eq!(launch.gemini_dir, None);
        assert_eq!(launch.model.as_deref(), Some("Gemini 3.8 Flash (Low)"));
        assert!(launch.env.is_empty());
        assert!(!key_profiles_root().exists());
        assert!(!settings_path().exists());
    }

    #[test]
    #[serial]
    fn existing_empty_agy_settings_are_not_imported() {
        let _home = HomeGuard::new();
        fs::create_dir_all(agy_dir()).unwrap();
        fs::write(settings_path(), r#"{"theme":"dark"}"#).unwrap();
        let db = Arc::new(Database::memory().unwrap());
        let state = AppState::new(db.clone());
        assert!(
            !crate::services::ProviderService::import_default_config(&state, AppType::Agy).unwrap()
        );
        assert!(db.get_all_providers("agy").unwrap().is_empty());

        // A stored provider is not applied by a full sync either.
        let blank = provider("blank", json!({"env":{},"config":{}}), false);
        db.save_provider("agy", &blank).unwrap();
        db.set_current_provider("agy", "blank").unwrap();
        let result = crate::services::ProviderService::sync_current_to_live(&state);
        if let Err(error) = &result {
            assert!(!error.to_string().contains("provider/agy"), "{error}");
        }
        assert_eq!(
            fs::read_to_string(settings_path()).unwrap(),
            r#"{"theme":"dark"}"#
        );
    }

    #[test]
    #[serial]
    fn settings_with_a_model_are_still_imported() {
        let _home = HomeGuard::new();
        fs::create_dir_all(agy_dir()).unwrap();
        fs::write(settings_path(), r#"{"model":"Gemini 3.8 Flash (High)"}"#).unwrap();
        let db = Arc::new(Database::memory().unwrap());
        let state = AppState::new(db.clone());
        assert!(
            crate::services::ProviderService::import_default_config(&state, AppType::Agy).unwrap()
        );
        // The key never lives in a file, so anything imported is agy's own login.
        let imported = db.get_provider_by_id("default", "agy").unwrap().unwrap();
        assert_eq!(imported.category.as_deref(), Some("official"));
        assert!(agy_direct::is_official(&imported));
    }

    /// 造一份 #8268 测试版留下的现场。
    fn legacy_install(home: &Path) -> PathBuf {
        let bin = agy_dir().join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(
            bin.join("agy"),
            format!("#!/bin/sh\n# {}\nexec agy \"$@\"\n", legacy::WRAPPER_MARK),
        )
        .unwrap();
        fs::write(
            agy_dir().join("env"),
            format!("# {}\nGEMINI_API_KEY='fixture'\n", legacy::PATH_MARK),
        )
        .unwrap();
        let block = legacy::path_entry_block(&bin);
        fs::write(
            home.join(".zshenv"),
            format!("# user\nexport KEEP=1\n{block}"),
        )
        .unwrap();
        fs::write(home.join(".zshrc"), &block).unwrap();
        fs::write(
            home.join(".profile"),
            format!("# user without newline{block}"),
        )
        .unwrap();
        bin
    }

    #[test]
    #[serial]
    fn legacy_launcher_cleanup_removes_only_marked_content() {
        let home = HomeGuard::new();
        let bin = legacy_install(home.path());
        assert!(remove_legacy_launcher().unwrap());
        assert!(!bin.exists());
        assert!(!agy_dir().join("env").exists());
        assert_eq!(
            fs::read_to_string(home.path().join(".zshenv")).unwrap(),
            "# user\nexport KEEP=1\n"
        );
        assert!(!home.path().join(".zshrc").exists(), "file we created");
        assert_eq!(
            fs::read_to_string(home.path().join(".profile")).unwrap(),
            "# user without newline"
        );
        // Idempotent.
        assert!(!remove_legacy_launcher().unwrap());
    }

    #[test]
    #[serial]
    fn legacy_cleanup_never_touches_unmanaged_files() {
        let home = HomeGuard::new();
        let bin = agy_dir().join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("agy"), "#!/bin/sh\necho user script\n").unwrap();
        fs::write(agy_dir().join("env"), "GEMINI_API_KEY=mine\n").unwrap();
        let foreign = format!(
            "export PATH=\"$HOME/x:$PATH\"\n# my own note about {}\n\n# {}\nexport PATH='/some/other/bin':\"$PATH\"\n",
            legacy::PATH_MARK,
            legacy::PATH_MARK
        );
        fs::write(home.path().join(".zshenv"), &foreign).unwrap();
        assert!(!remove_legacy_launcher().unwrap());
        assert!(bin.join("agy").exists());
        assert!(agy_dir().join("env").exists());
        assert_eq!(
            fs::read_to_string(home.path().join(".zshenv")).unwrap(),
            foreign
        );
    }

    #[test]
    #[serial]
    fn legacy_migration_rewrites_settings_back_to_login_mode() {
        let home = HomeGuard::new();
        legacy_install(home.path());
        fs::write(
            settings_path(),
            r#"{"modelProvider":"gemini","model":"Gemini 3.1 Pro (High)"}"#,
        )
        .unwrap();
        let db = Database::memory().unwrap();
        agy_direct::migrate_legacy_launcher(&db).unwrap();
        let doc = read_json(&settings_path());
        assert!(doc.get("modelProvider").is_none());
        assert_eq!(doc["model"], "Gemini 3.1 Pro (High)");
        assert!(!agy_dir().join("env").exists());

        // Without a legacy install nothing is touched, not even a stray modelProvider.
        let untouched = r#"{"modelProvider":"gemini"}"#;
        fs::write(settings_path(), untouched).unwrap();
        agy_direct::migrate_legacy_launcher(&db).unwrap();
        assert_eq!(fs::read_to_string(settings_path()).unwrap(), untouched);
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
        let gui_path = std::ffi::OsString::from("/usr/bin:/bin:/usr/sbin:/sbin");
        let brew = tmp.path().join("brew");

        // Nothing installed: no fabricated ~/.local/bin/agy fallback.
        assert_eq!(
            resolve_real_agy(&gui_path, &home, std::slice::from_ref(&brew), false),
            None
        );
        let found = fake_agy(&brew);
        assert_eq!(
            resolve_real_agy(&gui_path, &home, std::slice::from_ref(&brew), false),
            Some(found)
        );
        // ~/.local/bin wins over system dirs.
        let local = fake_agy(&home.join(".local/bin"));
        assert_eq!(
            resolve_real_agy(&gui_path, &home, &[brew], false),
            Some(local)
        );
    }

    #[test]
    fn resolver_skips_legacy_wrappers_and_non_executables() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let wrapper_dir = tmp.path().join("wrapper");
        fs::create_dir_all(&wrapper_dir).unwrap();
        fs::write(
            wrapper_dir.join("agy"),
            format!("#!/bin/sh\n# {}\n", legacy::WRAPPER_MARK),
        )
        .unwrap();
        fs::set_permissions(wrapper_dir.join("agy"), fs::Permissions::from_mode(0o755)).unwrap();
        let plain = tmp.path().join("plain");
        fs::create_dir_all(&plain).unwrap();
        fs::write(plain.join("agy"), "binary").unwrap();
        let real_dir = tmp.path().join("real");
        let real = fake_agy(&real_dir);
        let path = std::env::join_paths([&wrapper_dir, &plain, &real_dir]).unwrap();
        assert_eq!(resolve_real_agy(&path, &home, &[], false), Some(real));
    }
}
