//! OpenClaw：`openclaw plugins list --json`、`plugins marketplace entries|refresh --json`、
//! `plugins install|uninstall|enable|disable|update`（实测 openclaw 2026.10.10）。
//!
//! - 已安装列表含随程序自带的插件（`origin: bundled`），它们只能开关；装进状态目录的
//!   （`origin: global`）才能卸载、更新。
//! - 「发现」用 ClawHub 托管的目录，条目是包名。按 `clawhub:<包名>` 安装，不钉版本，之后
//!   `plugins update` 能跟到新版。装好后的插件 id 一般是包名去掉 scope 的最后一段
//!   （`@openclaw/memory-lancedb` → `memory-lancedb`），前端据此判断目录条目装没装；猜不中
//!   时只是「发现」里多留一项，再装时 CLI 会报已安装。
//! - 安装要 `--accept-capabilities`、卸载要 `--force`，否则 CLI 在终端里等确认。
//! - 改动保存给下一次 Gateway 启动。

use serde_json::Value;

use super::{run_json, str_field, PluginApp, PluginEntry, LIST_TIMEOUT};

/// 托管目录的条目没有 ClawHub 以外的来源。
const CATALOG_MARKETPLACE: &str = "clawhub";

pub(super) fn list_installed() -> Result<Vec<PluginEntry>, String> {
    run_json(
        PluginApp::Openclaw,
        &["plugins", "list", "--json"],
        LIST_TIMEOUT,
    )
    .map(|value| parse_installed(&value))
}

pub(super) fn list_catalog() -> Result<Vec<PluginEntry>, String> {
    run_json(
        PluginApp::Openclaw,
        &["plugins", "marketplace", "entries", "--json"],
        LIST_TIMEOUT,
    )
    .map(|value| parse_catalog(&value))
}

pub(super) fn install_spec(id: &str) -> String {
    format!("clawhub:{id}")
}

/// 包名装好后的插件 id：去掉 `@scope/`。
fn runtime_id(package: &str) -> &str {
    package.rsplit_once('/').map_or(package, |(_, name)| name)
}

/// `plugins update` 没东西可更新时打印 `Skipping "<id>" (source: path)` 一类的话。
pub(super) fn update_changed(stdout: &str) -> bool {
    let lower = stdout.to_ascii_lowercase();
    !(lower.contains("skipping") || lower.contains("up to date") || lower.contains("already"))
}

/// `{plugins: [{id, name, version, description, origin, enabled, status, rootDir, …}], …}`
fn parse_installed(value: &Value) -> Vec<PluginEntry> {
    value
        .get("plugins")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let id = str_field(item, "id")?;
                    let label = str_field(item, "name").filter(|name| name != &id);
                    let origin = str_field(item, "origin");
                    Some(PluginEntry {
                        name: id.clone(),
                        marketplace: String::new(),
                        display_name: label,
                        version: str_field(item, "version"),
                        description: str_field(item, "description"),
                        category: None,
                        installed: true,
                        enabled: item
                            .get("enabled")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        scope: None,
                        manageable: true,
                        removable: origin.as_deref() == Some("global"),
                        installable: true,
                        installed_id: None,
                        install_path: str_field(item, "rootDir"),
                        updated_at: None,
                        id,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `{source: "hosted", entries: [{label, id, version, install: {clawhubSpec, …}}]}`
fn parse_catalog(value: &Value) -> Vec<PluginEntry> {
    value
        .get("entries")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let id = str_field(item, "id")?;
                    let installed_id = Some(runtime_id(&id).to_string()).filter(|r| r != &id);
                    Some(PluginEntry {
                        name: id.clone(),
                        marketplace: CATALOG_MARKETPLACE.to_string(),
                        display_name: str_field(item, "label"),
                        version: str_field(item, "version"),
                        manageable: true,
                        removable: true,
                        installable: true,
                        installed_id,
                        id,
                        ..Default::default()
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn installed_list_marks_bundled_plugins_toggle_only() {
        // 字段取自实测 `openclaw plugins list --json`（去掉了空数组字段）
        let value = json!({
            "workspaceDir": "C:\\Users\\me\\.openclaw\\workspace",
            "registry": { "source": "persisted", "diagnostics": [] },
            "plugins": [
                {
                    "id": "a2a",
                    "name": "A2A",
                    "version": "2026.10.10",
                    "description": "A2A v1.0 Agent-to-Agent protocol channel plugin.",
                    "format": "openclaw",
                    "rootDir": "C:\\openclaw\\dist\\extensions\\a2a",
                    "origin": "bundled",
                    "enabled": false,
                    "status": "disabled"
                },
                {
                    "id": "hello",
                    "name": "hello",
                    "version": "0.1.0",
                    "description": "Says hello from manifest",
                    "format": "bundle",
                    "bundleFormat": "claude",
                    "rootDir": "C:\\Users\\me\\.openclaw\\extensions\\hello",
                    "origin": "global",
                    "enabled": true,
                    "status": "loaded"
                }
            ],
            "diagnostics": []
        });
        let entries = parse_installed(&value);
        assert_eq!(entries.len(), 2);
        let a2a = &entries[0];
        assert_eq!(a2a.display_name.as_deref(), Some("A2A"));
        assert!(a2a.installed && !a2a.enabled && a2a.manageable && !a2a.removable);
        assert_eq!(a2a.marketplace, "");
        let hello = &entries[1];
        assert_eq!(hello.display_name, None, "same as id");
        assert!(hello.enabled && hello.removable);
    }

    #[test]
    fn catalog_maps_package_names_to_runtime_ids() {
        // 字段取自实测 `openclaw plugins marketplace entries --json`
        let value = json!({
            "source": "hosted",
            "entries": [
                {
                    "label": "Memory LanceDB",
                    "id": "@openclaw/memory-lancedb",
                    "version": "2026.10.10",
                    "install": {
                        "clawhubSpec": "clawhub:@openclaw/memory-lancedb@2026.10.10",
                        "defaultChoice": "clawhub"
                    }
                },
                { "label": "Plain", "id": "plain-plugin", "version": "1.0.0" }
            ]
        });
        let catalog = parse_catalog(&value);
        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog[0].id, "@openclaw/memory-lancedb");
        assert_eq!(catalog[0].installed_id.as_deref(), Some("memory-lancedb"));
        assert_eq!(catalog[0].display_name.as_deref(), Some("Memory LanceDB"));
        assert_eq!(catalog[0].marketplace, "clawhub");
        assert!(!catalog[0].installed && catalog[0].installable);
        assert_eq!(catalog[1].installed_id, None);
        assert_eq!(
            install_spec(&catalog[0].id),
            "clawhub:@openclaw/memory-lancedb"
        );
    }

    #[test]
    fn update_output_detects_skips() {
        // 实测：本地路径装的插件
        assert!(!update_changed("Skipping \"hello\" (source: path).\n"));
        assert!(update_changed("Updated plugin \"x\" to 1.2.0.\n"));
    }
}
