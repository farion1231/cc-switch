//! Antigravity CLI（`agy`）：`agy plugin list --json` 和
//! `agy plugin install|uninstall|enable|disable`（实测 agy 1.5.14）。
//!
//! - 安装目录是 `~/.gemini/config/plugins/<dir>`，启用开关由 `agy` 写在
//!   `~/.gemini/config/config.json`（不手改它，避免与语言服务器或桌面端状态冲突）。
//! - 已安装列表按目录名识别（`name` 字段即目录名），`enable` / `disable` / `uninstall`
//!   都必须传目录名；传入 `name@marketplace` 时 `disable` 会报找不到、`uninstall` 会空转。
//! - CLI 没有 `--available` 列表命令，也没有自定义市场管理；官方（`antigravity-plugins-official`）
//!   和合作伙伴（`antigravity-plugins-partners`）两个目录托管在 `www.gstatic.com`，
//!   「发现」直接拉取这两份 `marketplace.json`（走 CC Switch 的全局代理），安装时传
//!   `<name>@<marketplace>`，并把 `installed_id` 设为 `<name>` 与已安装列表对齐。
//! - CLI 没有独立的 `plugin update` 命令，对市场来源先比对远端目录版本号，已是最新则直接返回，
//!   避免无意义的覆盖安装触发 Windows 目录占用报错；有新版本（或无法确认远端版本）时再执行
//!   `agy plugin install <name>@<marketplace>` 原地更新，同时保留 `config.json` 里的停用状态和其它键；
//!   本地/导入的插件没有远端市场，直接视为无需更新。
//! - `plugin list --json` 不带 `displayName` / `description` / `category`，从安装目录的
//!   `plugin.json`（支持 JSONC 注释与尾随逗号）补读。

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use super::{
    failure_detail, is_name_segment, resolve_cli_path, run_cli, run_json, str_field,
    validate_plugin_id, wsl_root_for_app, CliOutput, PluginActionOutcome, PluginApp, PluginEntry,
    LIST_TIMEOUT, NETWORK_TIMEOUT, PLUGIN_IN_USE, TOGGLE_TIMEOUT, UPDATE_NEEDS_REINSTALL,
};

const OFFICIAL_MARKETPLACE: &str = "antigravity-plugins-official";
const PARTNERS_MARKETPLACE: &str = "antigravity-plugins-partners";

const OFFICIAL_CATALOG_URL: &str = "https://www.gstatic.com/antigravity/plugins/marketplace.json";
const PARTNERS_CATALOG_URL: &str =
    "https://www.gstatic.com/antigravity/plugins-partners/marketplace.json";

const CATALOG_TIMEOUT: Duration = Duration::from_secs(30);

/// 插件清单可能放的位置：原生根目录、`.antigravity-plugin`、`.gemini-plugin`、`.claude-plugin`。
const MANIFEST_DIRS: [&str; 4] = [
    "",
    ".antigravity-plugin",
    ".gemini-plugin",
    ".claude-plugin",
];

/// 本地安装或从其它工具导入时 `agy plugin list --json` 填入的 `source`（不是市场名）。
const NON_MARKETPLACE_SOURCES: [&str; 3] = ["antigravity", "claude", "gemini"];

// ───────────────────────────── 参数校验 ─────────────────────────────

/// 旧版 Antigravity App 安装的市场插件可能落在纯数字（可带负号）的目录里，例如 `-1870732469773246571`。
fn is_legacy_numeric_dir(id: &str) -> bool {
    id.strip_prefix('-')
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

/// 安装参数必须是 `<name>@<marketplace>`：合作伙伴插件只传名字会默认去官方市场找而失败。
pub(super) fn validate_install_id(id: &str) -> Result<(), String> {
    validate_plugin_id(id)
}

/// 启停、卸载、更新参数是已安装目录名（拒绝 `name@marketplace` 和 `-flag`）。
pub(super) fn validate_installed_id(id: &str) -> Result<(), String> {
    if is_name_segment(id) || is_legacy_numeric_dir(id) {
        Ok(())
    } else {
        Err(format!("Invalid plugin id: {id}"))
    }
}

// ───────────────────────────── 插件清单（JSONC） ─────────────────────────────

#[derive(Default)]
struct AgyManifest {
    name: Option<String>,
    display_name: Option<String>,
    version: Option<String>,
    description: Option<String>,
    category: Option<String>,
}

fn read_manifest_file(path: &Path) -> Option<AgyManifest> {
    let text = std::fs::read_to_string(path).ok()?;
    let text = text.trim_start_matches('\u{feff}');
    // Antigravity 的 plugin.json 允许 JSONC 注释与尾随逗号；常规 JSON 走快路径，失败时退回 json5。
    let value = serde_json::from_str::<Value>(text)
        .or_else(|_| json5::from_str::<Value>(text))
        .ok()?;
    let interface = value.get("interface").cloned().unwrap_or(Value::Null);
    Some(AgyManifest {
        name: str_field(&value, "name"),
        display_name: str_field(&value, "displayName")
            .or_else(|| str_field(&interface, "displayName")),
        version: str_field(&value, "version"),
        description: str_field(&value, "description")
            .or_else(|| str_field(&interface, "shortDescription")),
        category: str_field(&value, "category").or_else(|| str_field(&interface, "category")),
    })
}

fn read_manifest(dir: &Path) -> AgyManifest {
    MANIFEST_DIRS
        .iter()
        .find_map(|manifest_dir| read_manifest_file(&dir.join(manifest_dir).join("plugin.json")))
        .unwrap_or_default()
}

/// 内置插件位于 `/builtin/plugins/`（只能开关，不能卸载）。
fn is_builtin_path(path: Option<&str>) -> bool {
    path.is_some_and(|p| {
        p.replace('\\', "/")
            .to_ascii_lowercase()
            .contains("/builtin/plugins/")
    })
}

fn marketplace_from_source(source: Option<String>) -> String {
    source
        .filter(|s| !NON_MARKETPLACE_SOURCES.contains(&s.as_str()))
        .unwrap_or_default()
}

// ───────────────────────────── 已安装 ─────────────────────────────

/// `agy plugin list --json`：`[{name, enabled, version, source?, path?, skills?, …}]`
pub(super) fn parse_installed(value: &Value, wsl_root: Option<&Path>) -> Vec<PluginEntry> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let id = str_field(item, "name")?;
                    let install_path = str_field(item, "path");
                    let manifest = install_path
                        .as_deref()
                        .and_then(|p| resolve_cli_path(p, wsl_root))
                        .map(|p| read_manifest(&p))
                        .unwrap_or_default();
                    let marketplace = marketplace_from_source(str_field(item, "source"));
                    let display_name = manifest
                        .display_name
                        .or_else(|| manifest.name.clone())
                        .filter(|label| label != &id);
                    // 目录名和清单名不同时（例如旧版数字目录 `-1870…`），用清单名与目录条目的 installed_id 对齐。
                    let catalog_id = manifest.name.filter(|manifest_name| {
                        manifest_name != &id && is_name_segment(manifest_name)
                    });
                    let removable = !is_builtin_path(install_path.as_deref());
                    Some(PluginEntry {
                        name: id.clone(),
                        marketplace,
                        display_name,
                        version: str_field(item, "version").or(manifest.version),
                        description: str_field(item, "description").or(manifest.description),
                        category: str_field(item, "category").or(manifest.category),
                        installed: true,
                        enabled: item
                            .get("enabled")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        scope: None,
                        manageable: true,
                        removable,
                        installable: true,
                        installed_id: None,
                        catalog_id,
                        install_path,
                        updated_at: None,
                        id,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn list_installed() -> Result<Vec<PluginEntry>, String> {
    let value = run_json(PluginApp::Agy, &["plugin", "list", "--json"], LIST_TIMEOUT)?;
    let wsl_root = wsl_root_for_app(PluginApp::Agy);
    Ok(parse_installed(&value, wsl_root.as_deref()))
}

// ───────────────────────────── 目录 ─────────────────────────────

/// 解析单份 `marketplace.json`：`{name, description, plugins: [{name, displayName, version, description, category, …}]}`
pub(super) fn parse_catalog_document(value: &Value, default_marketplace: &str) -> Vec<PluginEntry> {
    let marketplace = str_field(value, "name")
        .filter(|m| is_name_segment(m))
        .unwrap_or_else(|| default_marketplace.to_string());
    value
        .get("plugins")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let name = str_field(item, "name").filter(|n| is_name_segment(n))?;
                    let id = format!("{name}@{marketplace}");
                    let display_name = str_field(item, "displayName").filter(|d| d != &name);
                    Some(PluginEntry {
                        id,
                        installed_id: Some(name.clone()),
                        name,
                        marketplace: marketplace.clone(),
                        display_name,
                        version: str_field(item, "version"),
                        description: str_field(item, "description"),
                        category: str_field(item, "category"),
                        installed: false,
                        enabled: false,
                        scope: None,
                        manageable: true,
                        removable: true,
                        installable: true,
                        catalog_id: None,
                        install_path: None,
                        updated_at: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn fetch_marketplace(
    url: &str,
    default_marketplace: &str,
) -> Result<Vec<PluginEntry>, String> {
    let client = crate::proxy::http_client::get_for_url(url);
    let response = client
        .get(url)
        .timeout(CATALOG_TIMEOUT)
        .send()
        .await
        .map_err(|e| {
            format!("Failed to fetch Antigravity marketplace ({default_marketplace}): {e}")
        })?
        .error_for_status()
        .map_err(|e| {
            format!("Failed to fetch Antigravity marketplace ({default_marketplace}): {e}")
        })?;
    let value: Value = response.json().await.map_err(|e| {
        format!("Unexpected Antigravity marketplace response ({default_marketplace}): {e}")
    })?;
    Ok(parse_catalog_document(&value, default_marketplace))
}

fn dedup_entries(entries: impl IntoIterator<Item = PluginEntry>) -> Vec<PluginEntry> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for entry in entries {
        if seen.insert(entry.id.clone()) {
            out.push(entry);
        }
    }
    out
}

/// 合并官方与合作伙伴目录：两者都成功则合并去重；单边失败但另一边有有效条目时降级返回可用部分并记告警；
/// 两者都失败（或成功侧为空）时返回错误，让前端展示重试入口。
fn merge_catalog_results(
    official: Result<Vec<PluginEntry>, String>,
    partners: Result<Vec<PluginEntry>, String>,
) -> Result<Vec<PluginEntry>, String> {
    match (official, partners) {
        (Ok(official), Ok(partners)) => Ok(dedup_entries(official.into_iter().chain(partners))),
        (Ok(official), Err(err)) if !official.is_empty() => {
            log::warn!("Antigravity partners catalog unavailable, showing official only: {err}");
            Ok(dedup_entries(official))
        }
        (Ok(_), Err(err)) => Err(err),
        (Err(err), Ok(partners)) if !partners.is_empty() => {
            log::warn!("Antigravity official catalog unavailable, showing partners only: {err}");
            Ok(dedup_entries(partners))
        }
        (Err(err), Ok(_)) => Err(err),
        (Err(official_err), Err(partners_err)) => Err(format!("{official_err}\n{partners_err}")),
    }
}

async fn fetch_catalog() -> Result<Vec<PluginEntry>, String> {
    let (official, partners) = tokio::join!(
        fetch_marketplace(OFFICIAL_CATALOG_URL, OFFICIAL_MARKETPLACE),
        fetch_marketplace(PARTNERS_CATALOG_URL, PARTNERS_MARKETPLACE),
    );
    merge_catalog_results(official, partners)
}

pub(super) fn list_catalog() -> Result<Vec<PluginEntry>, String> {
    tauri::async_runtime::block_on(fetch_catalog())
}

// ───────────────────────────── 安装与更新 ─────────────────────────────

/// `agy` 在 Windows 上替换或移除插件目录时，若目录正被其它 Antigravity 进程（`agy` 会话、
/// Antigravity Hub、remote-control 守护进程）占用，会输出 `plugin is in use by another process`。
pub(super) fn is_plugin_in_use_error(detail: &str) -> bool {
    detail
        .to_ascii_lowercase()
        .contains("plugin is in use by another process")
}

pub(super) fn map_cli_error(out: &CliOutput) -> String {
    let detail = failure_detail(out);
    if is_plugin_in_use_error(&detail) {
        PLUGIN_IN_USE.to_string()
    } else {
        detail
    }
}

/// 声明了必填 MCP 变量的插件（如 `arize-skills`）安装后会保持停用，CLI 在首行打印
/// `"It is disabled until its MCP servers are configured: open /plugin …"`。
pub(super) fn install_needs_configuration(stdout: &str) -> bool {
    stdout
        .lines()
        .next()
        .is_some_and(|first_line| first_line.to_ascii_lowercase().contains("disabled until"))
}

pub(super) fn install_plugin(id: &str) -> Result<PluginActionOutcome, String> {
    let out = run_cli(PluginApp::Agy, &["plugin", "install", id], NETWORK_TIMEOUT)?;
    if !out.success {
        return Err(map_cli_error(&out));
    }
    Ok(PluginActionOutcome {
        changed: true,
        needs_configuration: install_needs_configuration(&out.stdout),
        ..Default::default()
    })
}

pub(super) fn uninstall_plugin(id: &str) -> Result<PluginActionOutcome, String> {
    let out = run_cli(PluginApp::Agy, &["plugin", "uninstall", id], TOGGLE_TIMEOUT)?;
    if !out.success {
        return Err(map_cli_error(&out));
    }
    Ok(PluginActionOutcome {
        changed: true,
        ..Default::default()
    })
}

/// 市场来源的插件用 `<name>@<marketplace>` 重装即更新；本地 / 导入插件和旧版数字目录不走这条路。
fn update_target(entry: &PluginEntry) -> Option<String> {
    if !is_name_segment(&entry.id)
        || entry.marketplace.is_empty()
        || !is_name_segment(&entry.marketplace)
    {
        return None;
    }
    Some(format!("{}@{}", entry.id, entry.marketplace))
}

fn catalog_url_for_marketplace(marketplace: &str) -> Option<&'static str> {
    match marketplace {
        OFFICIAL_MARKETPLACE => Some(OFFICIAL_CATALOG_URL),
        PARTNERS_MARKETPLACE => Some(PARTNERS_CATALOG_URL),
        _ => None,
    }
}

fn fetch_remote_version(marketplace: &str, name: &str) -> Option<String> {
    let url = catalog_url_for_marketplace(marketplace)?;
    match tauri::async_runtime::block_on(fetch_marketplace(url, marketplace)) {
        Ok(entries) => entries
            .into_iter()
            .find(|entry| entry.name == name)
            .and_then(|entry| entry.version),
        Err(err) => {
            log::warn!(
                "Failed to check remote Antigravity plugin version for {name}@{marketplace}: {err}"
            );
            None
        }
    }
}

fn is_up_to_date(local: Option<&str>, remote: Option<&str>) -> bool {
    matches!((local, remote), (Some(a), Some(b)) if a == b)
}

/// 旧版 Antigravity App 从市场装进数字目录的插件：重装时 `agy` 按清单名建目录，
/// 不一定原地覆盖这个数字目录，可能另装一份，所以不代为更新，让用户卸载后重装。
fn needs_reinstall_to_update(entry: &PluginEntry) -> bool {
    is_legacy_numeric_dir(&entry.id) && !entry.marketplace.is_empty()
}

fn version_changed(before: Option<&str>, after: Option<&str>) -> bool {
    match (before, after) {
        (Some(a), Some(b)) => a != b,
        _ => true,
    }
}

fn update_plugin_with(
    id: &str,
    list_installed: impl Fn() -> Result<Vec<PluginEntry>, String>,
    remote_version: impl FnOnce(&str, &str) -> Option<String>,
    run_install: impl FnOnce(&str) -> Result<(), String>,
) -> Result<PluginActionOutcome, String> {
    let installed = list_installed()?;
    let entry = installed
        .into_iter()
        .find(|entry| entry.id == id)
        .ok_or_else(|| format!("Plugin not found: {id}"))?;
    if needs_reinstall_to_update(&entry) {
        return Err(UPDATE_NEEDS_REINSTALL.to_string());
    }
    let Some(target) = update_target(&entry) else {
        // 本地 / 导入插件没有远端市场来源，无需更新。
        return Ok(PluginActionOutcome {
            changed: false,
            ..Default::default()
        });
    };
    let before_version = entry.version;
    if let Some(local) = before_version.as_deref() {
        let remote = remote_version(&entry.marketplace, &entry.id);
        if is_up_to_date(Some(local), remote.as_deref()) {
            return Ok(PluginActionOutcome {
                changed: false,
                ..Default::default()
            });
        }
    }
    run_install(&target)?;
    let after_version = list_installed()
        .ok()
        .and_then(|entries| entries.into_iter().find(|e| e.id == id))
        .and_then(|e| e.version);
    Ok(PluginActionOutcome {
        changed: version_changed(before_version.as_deref(), after_version.as_deref()),
        ..Default::default()
    })
}

pub(super) fn update_plugin(id: &str) -> Result<PluginActionOutcome, String> {
    update_plugin_with(id, list_installed, fetch_remote_version, |target| {
        let out = run_cli(
            PluginApp::Agy,
            &["plugin", "install", target],
            NETWORK_TIMEOUT,
        )?;
        if !out.success {
            return Err(map_cli_error(&out));
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn install_and_installed_ids_are_validated_separately() {
        assert!(validate_install_id("gcalendar@antigravity-plugins-official").is_ok());
        assert!(validate_install_id("arize-skills@antigravity-plugins-partners").is_ok());
        for bad in ["gcalendar", "--help", "@official", "a b@m", "a@m;rm"] {
            assert!(validate_install_id(bad).is_err(), "{bad}");
        }

        for ok in [
            "gcalendar",
            "arize-skills",
            "google_maps_platform",
            "a.b",
            "-1870732469773246571",
        ] {
            assert!(validate_installed_id(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "-",
            "--help",
            "-v",
            "-123abc",
            "gcalendar@antigravity-plugins-official",
            "../evil",
            "a b",
            "a/b",
        ] {
            assert!(validate_installed_id(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn installed_list_reads_jsonc_and_nested_manifests_and_handles_local_sources() {
        let dir = tempfile::tempdir().unwrap();

        let official_dir = dir.path().join("gcalendar");
        std::fs::create_dir_all(&official_dir).unwrap();
        std::fs::write(
            official_dir.join("plugin.json"),
            "// JSONC comment\n{\"name\":\"gcalendar\",\"displayName\":\"Google Calendar\",\"version\":\"0.0.8\",\"description\":\"Check availability.\",}",
        )
        .unwrap();

        let nested_dir = dir.path().join("nested-plug");
        std::fs::create_dir_all(nested_dir.join(".antigravity-plugin")).unwrap();
        std::fs::write(
            nested_dir.join(".antigravity-plugin/plugin.json"),
            r#"{"name":"nested-plug","displayName":"Nested Plugin","description":"From nested manifest","category":"Developer Tools"}"#,
        )
        .unwrap();

        let legacy_dir = dir.path().join("-1870732469773246571");
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(
            legacy_dir.join("plugin.json"),
            r#"{"name":"firebase","displayName":"Firebase","version":"1.0.3","description":"Legacy dir install"}"#,
        )
        .unwrap();

        let value = json!([
            {
                "name": "gcalendar",
                "enabled": true,
                "version": "0.0.8",
                "source": "antigravity-plugins-official",
                "path": official_dir.to_string_lossy(),
                "skills": ["gcalendar"]
            },
            {
                "name": "nested-plug",
                "enabled": false,
                "version": "2.0.0",
                "source": "antigravity",
                "path": nested_dir.to_string_lossy()
            },
            {
                "name": "-1870732469773246571",
                "enabled": true,
                "version": "1.0.3",
                "source": "antigravity-plugins-official",
                "path": legacy_dir.to_string_lossy()
            },
            {
                "name": "builtin-sample",
                "enabled": true,
                "path": r"C:\Users\me\.gemini\antigravity\builtin\plugins\builtin-sample"
            }
        ]);

        let entries = parse_installed(&value, None);
        assert_eq!(entries.len(), 4);

        let gc = &entries[0];
        assert_eq!(gc.id, "gcalendar");
        assert_eq!(gc.marketplace, "antigravity-plugins-official");
        assert_eq!(gc.display_name.as_deref(), Some("Google Calendar"));
        assert_eq!(gc.description.as_deref(), Some("Check availability."));
        assert_eq!(gc.catalog_id, None);
        assert!(gc.installed && gc.enabled && gc.manageable && gc.removable);

        let nested = &entries[1];
        assert_eq!(nested.id, "nested-plug");
        assert_eq!(nested.marketplace, "", "local source is not a marketplace");
        assert_eq!(nested.display_name.as_deref(), Some("Nested Plugin"));
        assert_eq!(nested.category.as_deref(), Some("Developer Tools"));
        assert!(nested.installed && !nested.enabled && nested.removable);

        let legacy = &entries[2];
        assert_eq!(legacy.id, "-1870732469773246571");
        assert_eq!(legacy.display_name.as_deref(), Some("Firebase"));
        assert_eq!(legacy.catalog_id.as_deref(), Some("firebase"));
        assert!(legacy.removable);

        let builtin = &entries[3];
        assert!(builtin.manageable && !builtin.removable);
    }

    #[test]
    fn catalog_document_maps_marketplace_ids_to_installed_dir_names() {
        let doc = json!({
            "name": "antigravity-plugins-partners",
            "description": "Antigravity partner plugins marketplace catalog.",
            "plugins": [
                {
                    "name": "arize-skills",
                    "displayName": "Arize AX",
                    "version": "1.4.0",
                    "description": "Query Arize AX projects.",
                    "category": "Developer Tools"
                },
                {
                    "name": "plain",
                    "displayName": "plain",
                    "version": "0.1.0"
                },
                {
                    "name": "--bad-name",
                    "version": "1.0.0"
                }
            ]
        });

        let entries = parse_catalog_document(&doc, PARTNERS_MARKETPLACE);
        assert_eq!(entries.len(), 2);

        let arize = &entries[0];
        assert_eq!(arize.id, "arize-skills@antigravity-plugins-partners");
        assert_eq!(arize.name, "arize-skills");
        assert_eq!(arize.installed_id.as_deref(), Some("arize-skills"));
        assert_eq!(arize.marketplace, "antigravity-plugins-partners");
        assert_eq!(arize.display_name.as_deref(), Some("Arize AX"));
        assert_eq!(arize.version.as_deref(), Some("1.4.0"));
        assert_eq!(
            arize.description.as_deref(),
            Some("Query Arize AX projects.")
        );
        assert_eq!(arize.category.as_deref(), Some("Developer Tools"));
        assert!(!arize.installed && !arize.enabled && arize.installable);

        assert_eq!(entries[1].display_name, None, "same as name");
    }

    #[test]
    fn update_target_only_includes_marketplace_entries_with_safe_names() {
        let mkt = PluginEntry {
            id: "gcalendar".into(),
            marketplace: "antigravity-plugins-official".into(),
            ..Default::default()
        };
        assert_eq!(
            update_target(&mkt).as_deref(),
            Some("gcalendar@antigravity-plugins-official")
        );

        let local = PluginEntry {
            id: "demo".into(),
            marketplace: String::new(),
            ..Default::default()
        };
        assert_eq!(update_target(&local), None);

        let legacy = PluginEntry {
            id: "-1870732469773246571".into(),
            marketplace: "antigravity-plugins-official".into(),
            ..Default::default()
        };
        assert_eq!(update_target(&legacy), None);
        assert!(needs_reinstall_to_update(&legacy));
        assert!(!needs_reinstall_to_update(&mkt));
        assert!(!needs_reinstall_to_update(&local));
        let legacy_local = PluginEntry {
            id: "-42".into(),
            marketplace: String::new(),
            ..Default::default()
        };
        assert!(!needs_reinstall_to_update(&legacy_local));

        assert!(!version_changed(Some("0.0.8"), Some("0.0.8")));
        assert!(version_changed(Some("0.0.7"), Some("0.0.8")));
        assert!(version_changed(None, Some("0.0.8")));
        assert!(version_changed(Some("0.0.8"), None));
    }

    #[test]
    fn install_stdout_detects_plugins_disabled_until_configured() {
        assert!(install_needs_configuration(
            "Installed plugin \"arize-skills\" from antigravity-plugins-partners. It is disabled until its MCP servers are configured: open /plugin and turn it on to fill them in.\n\nGoogle disclaimer..."
        ));
        assert!(!install_needs_configuration(
            "Installed plugin \"gcalendar\" from antigravity-plugins-official\n"
        ));
        assert!(!install_needs_configuration(
            "Installed plugin \"context7\" from antigravity-plugins-partners. If the context7 MCP server needs you to sign in, open /plugin, expand context7 under Installed, and choose Sign in.\n"
        ));
        assert!(!install_needs_configuration(""));
    }

    #[test]
    fn merge_catalog_degrades_gracefully_when_one_marketplace_fails() {
        let entry = |id: &str| PluginEntry {
            id: id.into(),
            ..Default::default()
        };

        let merged = merge_catalog_results(
            Ok(vec![entry("a@official"), entry("b@official")]),
            Ok(vec![entry("b@official"), entry("c@partners")]),
        )
        .unwrap();
        assert_eq!(
            merged.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["a@official", "b@official", "c@partners"]
        );

        let official_only =
            merge_catalog_results(Ok(vec![entry("a@official")]), Err("partners 503".into()))
                .unwrap();
        assert_eq!(official_only.len(), 1);
        assert_eq!(official_only[0].id, "a@official");

        let partners_only =
            merge_catalog_results(Err("official 503".into()), Ok(vec![entry("c@partners")]))
                .unwrap();
        assert_eq!(partners_only.len(), 1);
        assert_eq!(partners_only[0].id, "c@partners");

        // 成功侧为空时不要伪装成“无新插件”，而是暴露另一侧的失败错误
        assert_eq!(
            merge_catalog_results(Ok(vec![]), Err("partners 503".into())).unwrap_err(),
            "partners 503"
        );
        assert_eq!(
            merge_catalog_results(Err("official 503".into()), Ok(vec![])).unwrap_err(),
            "official 503"
        );

        let both_err = merge_catalog_results(
            Err("official timeout".into()),
            Err("partners timeout".into()),
        )
        .unwrap_err();
        assert!(both_err.contains("official timeout") && both_err.contains("partners timeout"));
    }

    #[test]
    fn update_skips_reinstall_when_remote_version_matches_local() {
        let installed = vec![PluginEntry {
            id: "gmail".into(),
            name: "gmail".into(),
            marketplace: OFFICIAL_MARKETPLACE.into(),
            version: Some("0.0.7".into()),
            installed: true,
            enabled: true,
            ..Default::default()
        }];
        let mut install_called = false;
        let outcome = update_plugin_with(
            "gmail",
            || Ok(installed.clone()),
            |marketplace, name| {
                assert_eq!(marketplace, OFFICIAL_MARKETPLACE);
                assert_eq!(name, "gmail");
                Some("0.0.7".into())
            },
            |_| {
                install_called = true;
                Ok(())
            },
        )
        .unwrap();
        assert!(
            !install_called,
            "should skip install when already up to date"
        );
        assert!(!outcome.changed);
    }

    #[test]
    fn update_reinstalls_when_newer_or_remote_unknown_and_surfaces_in_use_error() {
        let before = vec![PluginEntry {
            id: "gmail".into(),
            name: "gmail".into(),
            marketplace: OFFICIAL_MARKETPLACE.into(),
            version: Some("0.0.7".into()),
            installed: true,
            enabled: true,
            ..Default::default()
        }];
        let after = vec![PluginEntry {
            version: Some("0.0.8".into()),
            ..before[0].clone()
        }];

        // 有新版本时执行重装并返回 changed = true
        let calls = std::cell::Cell::new(0);
        let mut target_seen = String::new();
        let outcome = update_plugin_with(
            "gmail",
            || {
                let n = calls.get();
                calls.set(n + 1);
                Ok(if n == 0 {
                    before.clone()
                } else {
                    after.clone()
                })
            },
            |_, _| Some("0.0.8".into()),
            |target| {
                target_seen = target.to_string();
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(target_seen, "gmail@antigravity-plugins-official");
        assert!(outcome.changed);

        // 远端版本拉取失败（None）时退回执行 agy plugin install，遇到占用报错返回 PLUGIN_IN_USE
        let err = update_plugin_with(
            "gmail",
            || Ok(before.clone()),
            |_, _| None,
            |_| {
                let out = CliOutput {
                    success: false,
                    stdout: String::new(),
                    stderr: "Error: plugin is in use by another process: another Antigravity process is running it; close it and retry: Access is denied.".into(),
                };
                Err(map_cli_error(&out))
            },
        )
        .unwrap_err();
        assert_eq!(err, PLUGIN_IN_USE);

        // 其它 CLI 错误保持原样透传
        let other_err = map_cli_error(&CliOutput {
            success: false,
            stdout: String::new(),
            stderr: "Error: network unreachable".into(),
        });
        assert_eq!(other_err, "network unreachable");

        assert_eq!(
            catalog_url_for_marketplace(OFFICIAL_MARKETPLACE),
            Some(OFFICIAL_CATALOG_URL)
        );
        assert_eq!(
            catalog_url_for_marketplace(PARTNERS_MARKETPLACE),
            Some(PARTNERS_CATALOG_URL)
        );
        assert_eq!(catalog_url_for_marketplace("custom"), None);
    }
}
