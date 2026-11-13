//! Pi：插件就是 Pi 包（npm / git / 本地目录，打包扩展、技能、提示词模板、主题）。
//! 命令是 `pi list`、`pi install|remove|update <source>`（按 pi 1.5.0 的实现写）。
//!
//! - 插件 id 就是 `settings.json` 里 `packages` 条目的来源字符串（`npm:@scope/pkg`、
//!   `git:github.com/o/r@v1`、`./local`），交给 CLI 的也是它；CLI 按包身份（npm 包名、
//!   去掉 ref 的仓库地址、本地绝对路径）匹配。
//! - 已安装：`pi list` 没有 `--json`，解析它的文本（`User packages:` 段里两格缩进是来源，
//!   四格缩进是安装目录）；版本、描述读安装目录的 `package.json`。工作目录是 agent 目录，
//!   不会读到某个项目的 `.pi/settings.json`，所以只有 user 段。
//! - 发现：Pi 包画廊收录带 `pi-package` 关键词的 npm 包，直接查 npm registry 的搜索接口
//!   （走 CC Switch 的全局代理），按 `npm:<包名>` 安装。
//! - 启停：Pi 没有包级的开关命令。官方的过滤语法里资源数组写 `[]` 表示一项都不加载，
//!   停用就把条目写成对象形式、四类资源都设为 `[]`；用户原有的过滤先存进
//!   `ccSwitchFilters`，重新启用时还原（Pi 不认这个键，会原样保留它）。这一步由 CC Switch
//!   经写入引擎改 `settings.json`，只动这一个条目。
//! - 参数只能是安全字符（见 `is_safe_tool_command_arg`）：`~/x`、`C:\x` 这类本地来源交不给
//!   CLI，卸载时直接删条目（`pi remove` 对本地来源本来也只删条目）；交不出去的远程来源
//!   （`npm:x@^1`）卸载、更新时提示用户去终端里做。

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value};

use super::{str_field, PluginEntry};
use crate::app_config::AppType;
use crate::database::Database;
use crate::live::engine::LiveFile;
use crate::live::patch::json::{parse as parse_json_doc, serialize as serialize_json_doc};
use crate::live::patch::{KeyPath, LivePatch, LiveWriteError};
use crate::mode::operation::{AppWrite, FileChange};
use crate::mode::state::{op, PendingTarget};

/// 资源过滤的四个键；全为 `[]` 时这个包什么都不加载。
const RESOURCE_KEYS: [&str; 4] = ["extensions", "skills", "prompts", "themes"];
/// 停用前用户自己的过滤存在这里，启用时还原。
const STASH_KEY: &str = "ccSwitchFilters";

const REGISTRY_SEARCH_URL: &str = "https://registry.npmjs.org/-/v1/search";
const CATALOG_KEYWORD: &str = "keywords:pi-package";
/// npm 搜索接口单页上限。
const CATALOG_PAGE_SIZE: usize = 250;
/// 目录最多取这么多项（按 npm 的综合排序）。
const CATALOG_MAX: usize = 2000;
const CATALOG_TIMEOUT: Duration = Duration::from_secs(30);

// ───────────────────────────── 来源 ─────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    Npm,
    Git,
    Local,
}

/// 和 Pi 的 `parseSource` 一致：`npm:` 前缀是 npm；`git:` 前缀或 `http(s)/ssh/git://` 是 git；
/// 其余都是本地路径。
fn source_kind(source: &str) -> SourceKind {
    let lower = source.trim().to_ascii_lowercase();
    if lower.starts_with("npm:") {
        SourceKind::Npm
    } else if lower.starts_with("git:")
        || ["http://", "https://", "ssh://", "git://"]
            .iter()
            .any(|scheme| lower.starts_with(scheme))
    {
        SourceKind::Git
    } else {
        SourceKind::Local
    }
}

/// `npm:<spec>` 的包名（去掉版本）：`@scope/name@1.2.3` → `@scope/name`。
fn npm_package_name(source: &str) -> Option<&str> {
    let spec = source.trim().strip_prefix("npm:")?.trim();
    let version_at = if let Some(rest) = spec.strip_prefix('@') {
        rest.find('@').map(|i| i + 1)
    } else {
        spec.find('@')
    };
    let name = version_at.map_or(spec, |i| &spec[..i]);
    (!name.is_empty()).then_some(name)
}

/// 没读到 `package.json` 时显示的名字。
fn name_from_source(source: &str) -> String {
    if let Some(name) = npm_package_name(source) {
        return name.to_string();
    }
    let trimmed = source
        .trim()
        .trim_start_matches("git:")
        .trim_end_matches(['/', '\\']);
    let last = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    let last = match source_kind(source) {
        // `repo@v1` 的 ref 去掉
        SourceKind::Git => last.split('@').next().unwrap_or(last),
        _ => last,
    };
    let last = last.trim_end_matches(".git");
    if last.is_empty() {
        source.to_string()
    } else {
        last.to_string()
    }
}

/// 能原样交给 CLI 的来源。`pi`、`self` 会被 `pi update` 当成更新 Pi 自己。
pub(super) fn can_pass_to_cli(source: &str) -> bool {
    !source.starts_with('-')
        && !matches!(source, "pi" | "self")
        && crate::commands::is_safe_tool_command_arg(source)
}

/// 前端传来的插件 id：就是 `settings.json` 里的来源字符串。
pub(super) fn validate_source(source: &str) -> Result<(), String> {
    if source.trim().is_empty()
        || source != source.trim()
        || source.len() > super::MAX_SOURCE_LEN
        || source.chars().any(char::is_control)
    {
        return Err(format!("Invalid plugin id: {source}"));
    }
    Ok(())
}

pub(super) fn is_local_source(source: &str) -> bool {
    source_kind(source) == SourceKind::Local
}

/// CLI 交不出去时的提示（来源里有空格、`~`、`\` 之类）。
pub(super) fn terminal_hint(command: &str, source: &str) -> String {
    format!("This package source can't be passed safely from CC Switch. Run `pi {command} {source}` in a terminal instead.")
}

// ───────────────────────────── settings.json ─────────────────────────────

fn settings_path() -> Result<PathBuf, String> {
    crate::pi_config::get_pi_settings_path().map_err(|e| e.to_string())
}

/// `settings.json` 里的 `packages`；文件不存在时为空。
fn read_settings_packages() -> Result<Vec<Value>, String> {
    let path = settings_path()?;
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Failed to read {}: {e}", path.display())),
    };
    packages_in_settings_text(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn packages_in_settings_text(text: &str) -> Result<Vec<Value>, String> {
    let text = text.trim_start_matches('\u{feff}');
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    Ok(value
        .get("packages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

/// 条目的来源：字符串形式就是它本身，对象形式是 `source`。
fn package_source(pkg: &Value) -> Option<&str> {
    match pkg {
        Value::String(source) => Some(source),
        Value::Object(map) => map.get("source").and_then(Value::as_str),
        _ => None,
    }
}

fn find_package<'a>(packages: &'a [Value], source: &str) -> Option<&'a Value> {
    packages
        .iter()
        .find(|pkg| package_source(pkg) == Some(source))
}

/// 四类资源都显式写成 `[]`：这个包什么都不加载，视为停用。
fn is_disabled(pkg: &Value) -> bool {
    pkg.as_object().is_some_and(|map| {
        RESOURCE_KEYS.iter().all(|key| {
            map.get(*key)
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
        })
    })
}

/// 停用后的条目：资源数组都设为 `[]`，原有的过滤存进 `ccSwitchFilters`。
fn disabled_entry(pkg: &Value) -> Option<Value> {
    if is_disabled(pkg) {
        return None;
    }
    let mut map = match pkg {
        Value::String(source) => {
            let mut map = Map::new();
            map.insert("source".into(), Value::String(source.clone()));
            map
        }
        Value::Object(map) => map.clone(),
        _ => return None,
    };
    let stash: Map<String, Value> = RESOURCE_KEYS
        .iter()
        .filter_map(|key| map.get(*key).map(|v| (key.to_string(), v.clone())))
        .collect();
    // 用户手动启用过时留下的旧存档不算数，以现在的过滤为准
    if stash.is_empty() {
        map.shift_remove(STASH_KEY);
    } else {
        map.insert(STASH_KEY.into(), Value::Object(stash));
    }
    for key in RESOURCE_KEYS {
        // 已有的键原位改值，没有的追加到末尾
        map.insert(key.into(), Value::Array(Vec::new()));
    }
    Some(Value::Object(map))
}

/// 启用后的条目：去掉 `[]`，还原存下的过滤；只剩 `source` 时回到字符串形式。
fn enabled_entry(pkg: &Value) -> Option<Value> {
    if !is_disabled(pkg) {
        return None;
    }
    let mut map = pkg.as_object()?.clone();
    for key in RESOURCE_KEYS {
        map.shift_remove(key);
    }
    if let Some(Value::Object(stash)) = map.shift_remove(STASH_KEY) {
        for key in RESOURCE_KEYS {
            if let Some(value) = stash.get(key) {
                map.insert(key.into(), value.clone());
            }
        }
    }
    if map.len() == 1 {
        if let Some(Value::String(source)) = map.get("source") {
            return Some(Value::String(source.clone()));
        }
    }
    Some(Value::Object(map))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageEdit {
    SetEnabled(bool),
    Remove,
}

/// 只改 `packages` 里来源等于 `source` 的那个条目，其余内容原样保留。
struct PackagePatch<'a> {
    source: &'a str,
    edit: PackageEdit,
}

impl LivePatch for PackagePatch<'_> {
    fn apply(&self, path: &Path, pre: Option<&[u8]>) -> Result<Vec<u8>, LiveWriteError> {
        let (mut doc, style) = parse_json_doc(path, pre)?;
        let shape_error = || LiveWriteError::Shape {
            path: path.to_path_buf(),
            key_path: KeyPath::new(&["packages"]),
            expected: "数组",
        };
        let packages = doc
            .get_mut("packages")
            .and_then(Value::as_array_mut)
            .ok_or_else(shape_error)?;
        if let Some(index) = packages
            .iter()
            .position(|pkg| package_source(pkg) == Some(self.source))
        {
            match self.edit {
                PackageEdit::Remove => {
                    packages.remove(index);
                }
                PackageEdit::SetEnabled(enabled) => {
                    let next = if enabled {
                        enabled_entry(&packages[index])
                    } else {
                        disabled_entry(&packages[index])
                    };
                    if let Some(next) = next {
                        packages[index] = next;
                    }
                }
            }
        }
        serialize_json_doc(path, &doc, &style)
    }
}

/// 经写入引擎改 `settings.json` 里的一个条目；返回文件是否有变化。
fn write_package(db: &Database, source: &str, edit: PackageEdit) -> Result<bool, String> {
    let packages = read_settings_packages()?;
    if find_package(&packages, source).is_none() {
        return Err(format!("Package not found in Pi settings: {source}"));
    }
    let write = AppWrite::begin(db, AppType::Pi.as_str()).map_err(|e| e.to_string())?;
    let report = write
        .run(
            op::APPLY,
            &[FileChange {
                file: LiveFile::shared(settings_path()?),
                patch: &PackagePatch { source, edit },
            }],
            PendingTarget::default(),
        )
        .map_err(|e| e.to_string())?;
    Ok(!report.changed.is_empty())
}

pub(super) fn set_enabled(db: &Database, source: &str, enabled: bool) -> Result<bool, String> {
    write_package(db, source, PackageEdit::SetEnabled(enabled))
}

/// 交不给 CLI 的本地来源：直接删条目（`pi remove` 对本地来源也只做这一步）。
pub(super) fn remove_local_entry(db: &Database, source: &str) -> Result<bool, String> {
    write_package(db, source, PackageEdit::Remove)
}

// ───────────────────────────── 已安装 ─────────────────────────────

/// 去掉 ANSI 颜色序列（用户设了 `FORCE_COLOR` 时 chalk 照样上色）。
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// `pi list` 的 user 段：`[(来源, 安装目录)]`。
///
/// ```text
/// User packages:
///   npm:@scope/tools
///     /home/me/.pi/agent/npm/node_modules/@scope/tools
///   ./local-pkg (filtered)
/// ```
fn parse_list_output(stdout: &str) -> Vec<(String, Option<String>)> {
    let text = strip_ansi(stdout);
    let mut in_user = false;
    let mut packages: Vec<(String, Option<String>)> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() {
            continue;
        }
        let indent = trimmed.len() - trimmed.trim_start().len();
        let content = trimmed.trim_start();
        if indent == 0 {
            in_user = content == "User packages:";
            continue;
        }
        if !in_user {
            continue;
        }
        if indent >= 4 {
            if let Some(last) = packages.last_mut() {
                if last.1.is_none() {
                    last.1 = Some(content.to_string());
                }
            }
        } else {
            let source = content.strip_suffix(" (filtered)").unwrap_or(content);
            packages.push((source.to_string(), None));
        }
    }
    packages
}

#[derive(Default)]
struct PackageManifest {
    name: Option<String>,
    version: Option<String>,
    description: Option<String>,
}

fn read_package_manifest(dir: &Path) -> PackageManifest {
    std::fs::read_to_string(dir.join("package.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).ok())
        .map(|value| PackageManifest {
            name: str_field(&value, "name"),
            version: str_field(&value, "version"),
            description: str_field(&value, "description"),
        })
        .unwrap_or_default()
}

fn installed_entries(
    listed: Vec<(String, Option<String>)>,
    settings_packages: &[Value],
) -> Vec<PluginEntry> {
    listed
        .into_iter()
        .map(|(source, install_path)| {
            let manifest = install_path
                .as_deref()
                .map(|p| read_package_manifest(Path::new(p)))
                .unwrap_or_default();
            let enabled = !find_package(settings_packages, &source).is_some_and(is_disabled);
            // 都给卸载、更新入口：交不给 CLI 的远程来源（`npm:x@^1`）点了会提示去终端里做，
            // 标成不可卸载的话前端会显示成「内置」，更误导
            PluginEntry {
                name: manifest.name.unwrap_or_else(|| name_from_source(&source)),
                marketplace: String::new(),
                version: manifest.version,
                description: manifest.description,
                installed: true,
                enabled,
                manageable: true,
                removable: true,
                installable: true,
                install_path,
                id: source,
                ..Default::default()
            }
        })
        .collect()
}

pub(super) fn list_installed() -> Result<Vec<PluginEntry>, String> {
    let out = super::run_cli(super::PluginApp::Pi, &["list"], super::LIST_TIMEOUT)?;
    if !out.success {
        return Err(super::failure_detail(&out));
    }
    let listed = parse_list_output(&out.stdout);
    // CLI 刚读过同一份文件；这里读不了只影响开关状态的显示
    let settings_packages = read_settings_packages().unwrap_or_default();
    Ok(installed_entries(listed, &settings_packages))
}

/// `pi update <source>` 真的更新时会先打印 `Updating <source>...`；已是最新只有 `Updated …`。
pub(super) fn update_changed(stdout: &str) -> bool {
    strip_ansi(stdout)
        .lines()
        .any(|line| line.trim_start().starts_with("Updating "))
}

// ───────────────────────────── 目录 ─────────────────────────────

/// npm 搜索接口一页的结果：`(条目, 总数)`。
fn parse_search_page(value: &Value) -> (Vec<(String, PluginEntry)>, usize) {
    let total = value
        .get("total")
        .and_then(Value::as_u64)
        .map_or(0, |t| t as usize);
    let entries = value
        .get("objects")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let pkg = item.get("package")?;
                    let name = str_field(pkg, "name")?;
                    let entry = PluginEntry {
                        id: format!("npm:{name}"),
                        name: name.clone(),
                        marketplace: String::new(),
                        version: str_field(pkg, "version"),
                        description: str_field(pkg, "description"),
                        installable: true,
                        manageable: true,
                        removable: true,
                        updated_at: str_field(pkg, "date"),
                        ..Default::default()
                    };
                    Some((name, entry))
                })
                .collect()
        })
        .unwrap_or_default();
    (entries, total)
}

/// 已经按别的写法装过（钉了版本 `npm:x@1.2.3`）的，目录条目指向那个来源，「发现」里不再列。
fn link_installed_sources(catalog: &mut [PluginEntry], settings_packages: &[Value]) {
    for entry in catalog.iter_mut() {
        let configured = settings_packages
            .iter()
            .filter_map(package_source)
            .find(|source| npm_package_name(source) == Some(entry.name.as_str()));
        if let Some(source) = configured {
            if source != entry.id {
                entry.installed_id = Some(source.to_string());
            }
        }
    }
}

async fn fetch_catalog() -> Result<Vec<PluginEntry>, String> {
    let client = crate::proxy::http_client::get_for_url(REGISTRY_SEARCH_URL);
    let mut seen = std::collections::HashSet::new();
    let mut catalog = Vec::new();
    let mut from = 0usize;
    loop {
        let response = client
            .get(REGISTRY_SEARCH_URL)
            .query(&[
                ("text", CATALOG_KEYWORD.to_string()),
                ("size", CATALOG_PAGE_SIZE.to_string()),
                ("from", from.to_string()),
            ])
            .timeout(CATALOG_TIMEOUT)
            .send()
            .await
            .map_err(|e| format!("Failed to search the npm registry: {e}"))?
            .error_for_status()
            .map_err(|e| format!("Failed to search the npm registry: {e}"))?;
        let value: Value = response
            .json()
            .await
            .map_err(|e| format!("Unexpected response from the npm registry: {e}"))?;
        let (page, total) = parse_search_page(&value);
        let fetched = page.len();
        for (name, entry) in page {
            if seen.insert(name) {
                catalog.push(entry);
            }
        }
        from += fetched;
        if fetched == 0 || from >= total || from >= CATALOG_MAX {
            break;
        }
    }
    Ok(catalog)
}

pub(super) fn list_catalog() -> Result<Vec<PluginEntry>, String> {
    let mut catalog = tauri::async_runtime::block_on(fetch_catalog())?;
    let settings_packages = read_settings_packages().unwrap_or_default();
    link_installed_sources(&mut catalog, &settings_packages);
    Ok(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn source_kinds_follow_pi() {
        assert_eq!(source_kind("npm:@a/b"), SourceKind::Npm);
        assert_eq!(source_kind("git:github.com/o/r@v1"), SourceKind::Git);
        assert_eq!(source_kind("https://github.com/o/r"), SourceKind::Git);
        assert_eq!(source_kind("ssh://git@host/o/r"), SourceKind::Git);
        assert_eq!(source_kind("./pkg"), SourceKind::Local);
        assert_eq!(source_kind("~/dev/pkg"), SourceKind::Local);
        assert_eq!(source_kind(r"C:\dev\pkg"), SourceKind::Local);
    }

    #[test]
    fn names_from_sources() {
        assert_eq!(
            npm_package_name("npm:@scope/tools@1.2.3"),
            Some("@scope/tools")
        );
        assert_eq!(npm_package_name("npm:tools@^1"), Some("tools"));
        assert_eq!(npm_package_name("npm:tools"), Some("tools"));
        assert_eq!(npm_package_name("git:github.com/o/r"), None);
        assert_eq!(name_from_source("git:github.com/o/pi-tools@v1"), "pi-tools");
        assert_eq!(
            name_from_source("https://github.com/o/pi-tools.git"),
            "pi-tools"
        );
        assert_eq!(name_from_source(r"C:\dev\my-pkg\"), "my-pkg");
        assert_eq!(name_from_source("./my-pkg"), "my-pkg");
    }

    #[test]
    fn cli_arguments_exclude_self_update_and_unsafe_paths() {
        assert!(can_pass_to_cli("npm:@scope/tools"));
        assert!(can_pass_to_cli("git:github.com/o/r@v1"));
        assert!(can_pass_to_cli("./pkg"));
        for bad in [
            "pi",
            "self",
            "--force",
            "~/pkg",
            r"C:\pkg",
            "npm:x@^1.0",
            "a b",
        ] {
            assert!(!can_pass_to_cli(bad), "{bad}");
        }
        assert!(validate_source("~/my pkg").is_ok());
        for bad in ["", " npm:x", "a\nb"] {
            assert!(validate_source(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn list_output_reads_user_section_only() {
        let stdout = "\u{1b}[1mUser packages:\u{1b}[22m\n  npm:@scope/tools\n\u{1b}[2m    /home/me/.pi/agent/npm/node_modules/@scope/tools\u{1b}[22m\n  ./local-pkg (filtered)\n  git:github.com/o/missing\n\nProject packages:\n  npm:project-only\n    /work/.pi/npm/node_modules/project-only\n";
        assert_eq!(
            parse_list_output(stdout),
            vec![
                (
                    "npm:@scope/tools".to_string(),
                    Some("/home/me/.pi/agent/npm/node_modules/@scope/tools".to_string())
                ),
                ("./local-pkg".to_string(), None),
                ("git:github.com/o/missing".to_string(), None),
            ]
        );
        assert!(parse_list_output("No packages installed.\n").is_empty());
    }

    #[test]
    fn installed_entries_read_manifest_and_toggle_state() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"@scope/tools","version":"1.4.0","description":"Handy tools"}"#,
        )
        .unwrap();
        let settings = vec![
            json!("npm:@scope/tools"),
            json!({"source": "~/dev/off", "extensions": [], "skills": [], "prompts": [], "themes": []}),
            json!({"source": "npm:x@^1", "skills": []}),
        ];
        let entries = installed_entries(
            vec![
                (
                    "npm:@scope/tools".into(),
                    Some(dir.path().to_string_lossy().into_owned()),
                ),
                ("~/dev/off".into(), None),
                ("npm:x@^1".into(), None),
            ],
            &settings,
        );
        let tools = &entries[0];
        assert_eq!(tools.name, "@scope/tools");
        assert_eq!(tools.version.as_deref(), Some("1.4.0"));
        assert_eq!(tools.description.as_deref(), Some("Handy tools"));
        assert!(tools.enabled && tools.removable && tools.manageable);
        let off = &entries[1];
        assert_eq!(off.name, "off");
        assert!(!off.enabled, "all resources filtered out");
        let ranged = &entries[2];
        assert!(ranged.enabled, "a partial filter still loads the package");
        assert!(ranged.removable);
    }

    #[test]
    fn disable_and_enable_round_trip_filters() {
        let plain = json!("npm:tools");
        let off = disabled_entry(&plain).unwrap();
        assert_eq!(
            off,
            json!({"source": "npm:tools", "extensions": [], "skills": [], "prompts": [], "themes": []})
        );
        assert!(is_disabled(&off));
        assert_eq!(disabled_entry(&off), None, "already disabled");
        assert_eq!(enabled_entry(&off).unwrap(), plain);
        assert_eq!(enabled_entry(&plain), None, "already enabled");

        let filtered = json!({"source": "npm:tools", "autoload": false, "skills": ["skills/a"], "prompts": []});
        let off = disabled_entry(&filtered).unwrap();
        assert!(is_disabled(&off));
        assert_eq!(
            off["ccSwitchFilters"],
            json!({"skills": ["skills/a"], "prompts": []})
        );
        assert_eq!(off["autoload"], json!(false));
        let back = enabled_entry(&off).unwrap();
        assert_eq!(back, filtered);
        assert_eq!(
            back.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["source", "autoload", "skills", "prompts"],
            "key order preserved"
        );
    }

    #[test]
    fn package_patch_edits_only_the_matching_entry() {
        let path = Path::new("settings.json");
        let before = "{\n  \"defaultModel\": \"m\",\n  \"packages\": [\n    \"npm:a\",\n    \"npm:b\"\n  ]\n}\n";
        let apply = |source: &str, edit: PackageEdit, pre: &str| -> Value {
            let bytes = PackagePatch { source, edit }
                .apply(path, Some(pre.as_bytes()))
                .unwrap();
            serde_json::from_slice(&bytes).unwrap()
        };
        let off = apply("npm:b", PackageEdit::SetEnabled(false), before);
        assert_eq!(off["defaultModel"], json!("m"));
        assert_eq!(off["packages"][0], json!("npm:a"));
        assert!(is_disabled(&off["packages"][1]));

        let removed = apply("npm:a", PackageEdit::Remove, before);
        assert_eq!(removed["packages"], json!(["npm:b"]));

        let untouched = apply("npm:zzz", PackageEdit::Remove, before);
        assert_eq!(untouched["packages"], json!(["npm:a", "npm:b"]));

        assert!(PackagePatch {
            source: "npm:a",
            edit: PackageEdit::Remove
        }
        .apply(path, Some(b"{\"packages\": 1}"))
        .is_err());
    }

    #[test]
    fn update_detection() {
        assert!(update_changed("Updating npm:tools...\nUpdated npm:tools\n"));
        assert!(!update_changed("Updated npm:tools\n"));
    }

    #[test]
    fn search_page_and_installed_links() {
        let value = json!({
            "total": 3,
            "objects": [
                {"package": {"name": "@scope/tools", "version": "1.4.0", "description": "Tools", "date": "2026-10-01T00:00:00.000Z"}},
                {"package": {"name": "pinned", "version": "2.0.0"}},
                {"package": {}}
            ]
        });
        let (page, total) = parse_search_page(&value);
        assert_eq!(total, 3);
        let mut catalog: Vec<PluginEntry> = page.into_iter().map(|(_, e)| e).collect();
        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog[0].id, "npm:@scope/tools");
        assert!(catalog[0].installable && !catalog[0].installed);

        link_installed_sources(
            &mut catalog,
            &[
                json!("npm:@scope/tools"),
                json!({"source": "npm:pinned@1.0.0"}),
            ],
        );
        assert_eq!(catalog[0].installed_id, None, "same source");
        assert_eq!(catalog[1].installed_id.as_deref(), Some("npm:pinned@1.0.0"));
    }

    #[test]
    fn settings_packages_tolerate_missing_and_bom() {
        assert!(packages_in_settings_text("").unwrap().is_empty());
        assert_eq!(
            packages_in_settings_text("\u{feff}{\"packages\":[\"npm:a\"]}").unwrap(),
            vec![json!("npm:a")]
        );
        assert!(packages_in_settings_text("{ bad").is_err());
    }
}
