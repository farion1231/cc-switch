//! Grok Build：`grok plugin list|install|uninstall|enable|disable|update` 和
//! `grok plugin marketplace list|add|remove|update`（实测 grok 1.0.70）。
//!
//! - 插件按名字识别，不是 `name@marketplace`。
//! - `plugin list --json` 不带启停状态。状态在 `<GROK_HOME>/config.toml` 的
//!   `[plugins] enabled / disabled` 两个数组里（CLI 的 enable / disable 只改它们），
//!   装好的插件默认启用。
//! - 安装必须带 `--trust`；`marketplace add` 不认 `--json`，成败只看退出码。

use std::path::Path;

use serde_json::Value;

use super::{read_manifest_any, str_field, PluginEntry, PluginListing, PluginMarketplace};

/// 插件清单可能放的位置：Grok 自己的、Claude Code 兼容的、插件根目录。
const MANIFEST_DIRS: [&str; 3] = [".grok-plugin", ".claude-plugin", ""];

pub(super) fn read_config_text() -> String {
    std::fs::read_to_string(crate::grok_config::get_grok_config_path()).unwrap_or_default()
}

fn plugin_list(config: &toml::Table, key: &str) -> Vec<String> {
    config
        .get("plugins")
        .and_then(|plugins| plugins.get(key))
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// 数组里的一项是插件名，或 `<scope>/<hash>/<name>` 形式的完整 id。
fn names_plugin(item: &str, name: &str) -> bool {
    item == name || item.rsplit_once('/').is_some_and(|(_, last)| last == name)
}

/// 没被 `disabled` 点名，或被 `enabled` 强制打开。配置读不了时按默认（启用）算。
pub(super) fn is_enabled(config_text: &str, name: &str) -> bool {
    let Ok(config) = config_text.parse::<toml::Table>() else {
        return true;
    };
    let listed = |key: &str| {
        plugin_list(&config, key)
            .iter()
            .any(|item| names_plugin(item, name))
    };
    !listed("disabled") || listed("enabled")
}

fn parse_entry(item: &Value, config_text: &str) -> Option<PluginEntry> {
    let name = str_field(item, "name")?;
    let installed = str_field(item, "status").as_deref() == Some("installed");
    let path = installed.then(|| str_field(item, "path")).flatten();
    let manifest = path
        .as_deref()
        .map(|p| read_manifest_any(Path::new(p), &MANIFEST_DIRS))
        .unwrap_or_default();
    Some(PluginEntry {
        id: name.clone(),
        marketplace: str_field(item, "marketplace").unwrap_or_default(),
        display_name: manifest.display_name,
        version: str_field(item, "version"),
        description: str_field(item, "description").or(manifest.description),
        category: manifest.category,
        installed,
        enabled: installed && is_enabled(config_text, &name),
        scope: None,
        manageable: true,
        removable: true,
        installable: true,
        installed_id: None,
        install_path: path,
        updated_at: None,
        name,
    })
}

/// `plugin list --json [--available]`：一个数组，每项的 `status` 是 `installed` 或 `available`。
pub(super) fn parse_listing(value: &Value, config_text: &str) -> PluginListing {
    let entries: Vec<PluginEntry> = value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| parse_entry(item, config_text))
                .collect()
        })
        .unwrap_or_default();
    let (installed, mut available): (Vec<_>, Vec<_>) =
        entries.into_iter().partition(|entry| entry.installed);
    available.retain(|a| !installed.iter().any(|i| i.id == a.id));
    PluginListing {
        installed,
        available,
    }
}

/// `plugin marketplace list --json`：`[{name, kind, source: {path | url | repo | git}}]`。
pub(super) fn parse_marketplaces(value: &Value) -> Vec<PluginMarketplace> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let name = str_field(item, "name")?;
                    let source_obj = item.get("source");
                    let source = source_obj.and_then(|s| {
                        s.as_str().map(str::to_string).or_else(|| {
                            ["url", "repo", "git", "path"]
                                .iter()
                                .find_map(|key| str_field(s, key))
                        })
                    });
                    Some(PluginMarketplace {
                        name,
                        location: source_obj.and_then(|s| str_field(s, "path")),
                        source,
                        removable: true,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `plugin update <name>` 每更新一项打印 `<id>: updated (<旧版本> -> <新版本>)`；版本都没变
/// 就算已是最新。看不出来时按有更新算。
pub(super) fn update_changed(stdout: &str) -> bool {
    let pairs: Vec<(String, String)> = stdout
        .lines()
        .filter_map(|line| {
            let inner = line.trim().strip_suffix(')')?;
            let inner = &inner[inner.rfind('(')? + 1..];
            let (from, to) = inner.split_once(" -> ")?;
            Some((from.trim().to_string(), to.trim().to_string()))
        })
        .collect();
    if pairs.is_empty() {
        let lower = stdout.to_ascii_lowercase();
        return !(lower.contains("up to date") || lower.contains("no update"));
    }
    pairs.iter().any(|(from, to)| from != to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 实测 `grok plugin list --json --available` 的输出（字段原样）。
    fn listing_fixture(path: &str) -> Value {
        json!([
            {
                "status": "installed",
                "name": "hello",
                "repo_key": "c--mkt-plugins-hello-0bb4609f",
                "version": "0.1.0",
                "path": path,
                "source": "C:\\mkt\\plugins\\hello",
                "marketplace": "mkt"
            },
            {
                "status": "available",
                "name": "other",
                "version": "0.2.0",
                "description": "Says hello",
                "marketplace": "mkt",
                "skill_count": 1,
                "has_hooks": false,
                "has_agents": false,
                "has_mcp": false
            }
        ])
    }

    #[test]
    fn listing_reads_status_manifest_and_config_toggle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude-plugin")).unwrap();
        std::fs::write(
            dir.path().join(".claude-plugin/plugin.json"),
            r#"{"name":"hello","description":"Says hello from manifest"}"#,
        )
        .unwrap();
        let value = listing_fixture(&dir.path().to_string_lossy());

        let listing = parse_listing(&value, "");
        assert_eq!(listing.installed.len(), 1);
        let hello = &listing.installed[0];
        assert_eq!(hello.id, "hello");
        assert_eq!(hello.marketplace, "mkt");
        assert!(hello.installed && hello.enabled && hello.removable);
        assert_eq!(
            hello.description.as_deref(),
            Some("Says hello from manifest")
        );
        assert_eq!(listing.available.len(), 1);
        assert_eq!(listing.available[0].id, "other");
        assert!(!listing.available[0].installed);
        assert_eq!(
            listing.available[0].description.as_deref(),
            Some("Says hello")
        );

        // CLI 停用后写的就是这个样子
        let disabled = "[plugins]\nenabled = []\ndisabled = [\"hello\"]\n";
        assert!(!parse_listing(&value, disabled).installed[0].enabled);
    }

    #[test]
    fn toggle_state_matches_names_and_full_ids() {
        assert!(is_enabled("", "a"));
        assert!(is_enabled("not [valid", "a"));
        assert!(!is_enabled("[plugins]\ndisabled = [\"a\"]", "a"));
        assert!(!is_enabled(
            "[plugins]\ndisabled = [\"user/a1b2c3d4/a\"]",
            "a"
        ));
        assert!(is_enabled("[plugins]\ndisabled = [\"user/x/ab\"]", "a"));
        assert!(is_enabled(
            "[plugins]\ndisabled = [\"a\"]\nenabled = [\"a\"]",
            "a"
        ));
    }

    #[test]
    fn marketplaces_read_local_and_git_sources() {
        let value = json!([
            { "name": "mkt", "kind": "local", "source": { "path": "C:\\mkt" } },
            { "name": "team", "kind": "git", "source": { "url": "https://x/y.git" } }
        ]);
        let markets = parse_marketplaces(&value);
        assert_eq!(markets.len(), 2);
        assert_eq!(markets[0].source.as_deref(), Some("C:\\mkt"));
        assert_eq!(markets[0].location.as_deref(), Some("C:\\mkt"));
        assert_eq!(markets[1].source.as_deref(), Some("https://x/y.git"));
        assert!(markets.iter().all(|m| m.removable));
    }

    #[test]
    fn update_output_tells_whether_anything_changed() {
        // 实测输出
        assert!(!update_changed(
            "c--mkt-plugins-hello-0bb4609f: updated (0.1.0 -> 0.1.0)\n"
        ));
        assert!(update_changed("x: updated (0.1.0 -> 0.2.0)\n"));
        assert!(!update_changed("All plugins are up to date.\n"));
        assert!(update_changed("Updated hello\n"));
    }
}
