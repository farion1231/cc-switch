//! MiniMax Code：`mcode plugin list [--available] --json` 和
//! `mcode plugin add|remove|enable|disable <name@marketplace> --json`（列表实测 mcode 0.7.4）。
//!
//! 列表格式和 Codex 一样是 `{installed, available}`、条目用 `pluginId`，但显示名和描述直接在
//! JSON 里，不用读插件清单。CLI 没有更新命令；自定义市场只在内部构建里可用，正式版只有
//! 官方市场，所以不提供市场页。

use serde_json::Value;

use super::{split_id, str_field, PluginEntry, PluginListing};

fn parse_entry(item: &Value, installed: bool) -> Option<PluginEntry> {
    let id = str_field(item, "pluginId")?;
    let (name, market) = split_id(&id);
    Some(PluginEntry {
        name: str_field(item, "name").unwrap_or(name),
        marketplace: str_field(item, "marketplaceName").unwrap_or(market),
        display_name: str_field(item, "displayName"),
        version: str_field(item, "version"),
        description: str_field(item, "description"),
        category: None,
        installed,
        enabled: installed
            && item
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        scope: None,
        manageable: true,
        removable: true,
        installable: true,
        installed_id: None,
        install_path: None,
        updated_at: None,
        id,
    })
}

pub(super) fn parse_listing(value: &Value) -> PluginListing {
    let collect = |key: &str, installed: bool| -> Vec<(PluginEntry, bool)> {
        value
            .get(key)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let flagged = item
                            .get("installed")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        parse_entry(item, installed).map(|entry| (entry, flagged))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let installed: Vec<PluginEntry> = collect("installed", true)
        .into_iter()
        .map(|(entry, _)| entry)
        .collect();
    let available = collect("available", false)
        .into_iter()
        .filter(|(entry, flagged)| !flagged && !installed.iter().any(|i| i.id == entry.id))
        .map(|(entry, _)| entry)
        .collect();
    PluginListing {
        installed,
        available,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn listing_uses_inline_display_name_and_description() {
        // 字段取自实测 `mcode plugin list --available --json`
        let value = json!({
            "installed": [{
                "pluginId": "computer-use@official",
                "name": "computer-use",
                "displayName": "电脑操控",
                "marketplaceName": "official",
                "version": "1.1.1",
                "description": "通过原生能力查看应用",
                "author": "MiniMax",
                "installed": true,
                "enabled": false,
                "capabilities": { "appCount": 0, "mcpServerCount": 0, "skillCount": 1 }
            }],
            "available": [
                { "pluginId": "computer-use@official", "name": "computer-use", "installed": true },
                {
                    "pluginId": "pdf@official",
                    "name": "pdf",
                    "displayName": "PDF",
                    "marketplaceName": "official",
                    "installed": false,
                    "enabled": false
                }
            ],
            "unavailableMarketplaces": [{ "name": "team", "error": "offline" }]
        });
        let listing = parse_listing(&value);
        assert_eq!(listing.installed.len(), 1);
        let cu = &listing.installed[0];
        assert_eq!(cu.display_name.as_deref(), Some("电脑操控"));
        assert_eq!(cu.description.as_deref(), Some("通过原生能力查看应用"));
        assert!(cu.installed && !cu.enabled && cu.removable);
        assert_eq!(
            listing
                .available
                .iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            ["pdf@official"]
        );
        assert!(!listing.available[0].enabled);
    }

    #[test]
    fn empty_listing_is_empty() {
        let listing = parse_listing(&json!({ "installed": [], "available": [] }));
        assert!(listing.installed.is_empty() && listing.available.is_empty());
    }
}
