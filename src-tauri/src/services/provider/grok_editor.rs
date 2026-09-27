//! Grok Build 供应商编辑器：底部的 config.toml 就是「切到这个供应商之后 config.toml 的样子」。
//!
//! - 显示：在内存里对当前 live 做一次切换投影（和切换用同一个补丁）：`models.default` 和
//!   CC Switch 写的模型表换成这个供应商的，其余部分是 live 原样。
//! - 保存：`models.default` 和模型表写回这个供应商的行（行里其余内容原样保留）；其余改动
//!   是 Grok Build 的全局设置，经引擎写进 live，只改用户动过的键。编辑的是直连模式下的
//!   当前供应商时，模型表在同一次写入里也换进 live。
//! - 三方比较：同 Codex 编辑器（`editor_toml`）。
//!
//! 改动的粒度：顶层的值；顶层表里的每个键；`[model.*]` 里 CC Switch 的表以外的每张表。

use serde_json::{Map, Value};
use toml_edit::DocumentMut;

use crate::database::Database;
use crate::error::AppError;
use crate::live::engine::{read_current, DeviceStore};
use crate::live::patch::toml::parse;
use crate::live::project::claude::PROXY_TOKEN_PLACEHOLDER;
use crate::live::project::grok::{GrokConfigPatch, GrokProjection};
use crate::mode::state::{op, PendingTarget};
use crate::provider::Provider;
use crate::store::AppState;

use super::claude_editor::{ConflictPolicy, EditorView, InactiveField};
use super::editor_toml::{self, config_text, Entry, TomlEdits};
use super::grok_direct;

fn parse_text(text: &str, what: &str) -> Result<DocumentMut, AppError> {
    editor_toml::parse_text(
        text,
        "provider.grokbuild.editor.invalid_toml",
        "Grok Build",
        what,
    )
}

/// 全局设置的每个位置：`models.default` 和 `cc_tables`（CC Switch 写的模型表）不算。
fn entries(doc: &DocumentMut, cc_tables: &[&str]) -> Vec<Entry> {
    let mut entries = Vec::new();
    for (key, item) in doc.as_table().iter() {
        match item.as_table_like() {
            Some(table) => {
                for (child, child_item) in table.iter() {
                    if (key == "models" && child == "default")
                        || (key == "model" && cc_tables.contains(&child))
                    {
                        continue;
                    }
                    entries.push(Entry {
                        path: vec![key.to_string(), child.to_string()],
                        item: child_item.clone(),
                    });
                }
            }
            None => entries.push(Entry {
                path: vec![key.to_string()],
                item: item.clone(),
            }),
        }
    }
    entries
}

/// 编辑器显示的内容。`settings_config` 是这个供应商的行；`category` 用来认出官方卡。
///
/// 行本身过不了校验（比如旧版回填后 `models.default` 指不到表，行里又有多张表）时照原样
/// 显示行的内容，让用户在编辑器里改好；保存时再校验。
pub fn view(
    state: &AppState,
    settings_config: &Value,
    category: Option<&str>,
) -> Result<EditorView, AppError> {
    let official = category == Some("official");
    let Ok(projection) = GrokProjection::of(settings_config, official) else {
        return Ok(EditorView {
            settings: settings_config.clone(),
            inactive: Vec::new(),
        });
    };

    let file = grok_direct::config_file();
    let pre = read_current(&file.path)?;
    let mut doc = parse(&file.path, pre.as_deref())?;
    let live_owner =
        crate::mode::current::direct_provider(&state.db, &crate::app_config::AppType::GrokBuild)?;
    let retired = grok_direct::retired_tables(&DeviceStore::for_device(), live_owner.as_ref())?;
    GrokConfigPatch::direct(&projection, retired, PROXY_TOKEN_PLACEHOLDER)
        .apply_to(&file.path, &mut doc)?;

    let mut settings = settings_config
        .as_object()
        .cloned()
        .unwrap_or_else(Map::new);
    settings.insert("config".to_string(), Value::String(doc.to_string()));
    let row_table = row_table_name(settings_config);
    Ok(EditorView {
        inactive: inactive_fields(
            config_text(settings_config),
            &doc,
            row_table.as_deref(),
            projection.table_name(),
        ),
        settings: Value::Object(settings),
    })
}

/// 行里 CC Switch 的表名（按投影的规则解析；解析不了按 `models.default`）。
fn row_table_name(settings: &Value) -> Option<String> {
    if let Ok(projection) = GrokProjection::of(settings, false) {
        return projection.table_name().map(str::to_string);
    }
    config_text(settings)
        .parse::<DocumentMut>()
        .ok()?
        .get("models")?
        .get("default")?
        .as_str()
        .map(str::to_string)
}

/// 行里保存着、但不随切换生效的全局设置。
fn inactive_fields(
    row_text: &str,
    display: &DocumentMut,
    row_table: Option<&str>,
    shown_table: Option<&str>,
) -> Vec<InactiveField> {
    let Ok(row) = row_text.parse::<DocumentMut>() else {
        return Vec::new();
    };
    let cc_tables: Vec<&str> = row_table.into_iter().chain(shown_table).collect();
    editor_toml::inactive_fields(entries(&row, &cc_tables), display)
}

/// 一次编辑器保存：存进行的内容，和要写进 live 的全局改动。
pub(crate) struct GrokEditorPlan {
    pub row_settings: Value,
    pub edits: TomlEdits,
}

/// 把编辑器里的配置拆开：`models.default` 和模型表换进行（行里其余内容原样保留），其余
/// 部分和 `base` 比，得出用户改过的全局设置。官方卡没有模型表，行原样保留。
pub(crate) fn plan_save(
    stored_row: Option<&Value>,
    edited: &Value,
    base: &Value,
    official: bool,
    on_conflict: ConflictPolicy,
) -> Result<GrokEditorPlan, AppError> {
    let edited_doc = parse_text(config_text(edited), "edited")?;
    let base_doc = parse_text(config_text(base), "base")?;
    let projection = GrokProjection::of(edited, official)?;
    let base_table = GrokProjection::of(base, official)
        .ok()
        .and_then(|base| base.table_name().map(str::to_string));
    let cc_tables: Vec<&str> = [projection.table_name(), base_table.as_deref()]
        .into_iter()
        .flatten()
        .collect();
    let edits = TomlEdits::between(
        &entries(&base_doc, &cc_tables),
        &entries(&edited_doc, &cc_tables),
        on_conflict,
    );
    let row_settings = if official {
        let mut row = edited.clone();
        row["config"] = Value::String(stored_row.map(config_text).unwrap_or_default().to_string());
        row
    } else {
        store_into_row(stored_row, edited, &projection)?
    };
    Ok(GrokEditorPlan {
        row_settings,
        edits,
    })
}

/// 把编辑器里的 `models.default` 和模型表存回行：行里原来 CC Switch 的表换成编辑器的，
/// 其余内容原样保留（降级后旧版会整份使用这些行）。
fn store_into_row(
    stored_row: Option<&Value>,
    edited: &Value,
    projection: &GrokProjection,
) -> Result<Value, AppError> {
    let mut row = edited.clone();
    let stored_text = stored_row.map(config_text).unwrap_or("");
    let mut doc = parse_text(stored_text, "stored")?;
    let retired: Vec<String> = stored_row.and_then(row_table_name).into_iter().collect();
    let patch = GrokConfigPatch {
        target: projection.table.clone(),
        retired,
        placeholder: None,
    };
    patch
        .apply_to(std::path::Path::new("config.toml"), &mut doc)
        .map_err(AppError::from)?;
    row["config"] = Value::String(doc.to_string());
    Ok(row)
}

/// 把编辑器保存的改动写进 live。`key_fields` 有值时（直连模式下编辑当前供应商）模型表在
/// 同一次写入里换成它的；`prev` 是编辑前的行，没有写入记录时用来推断旧表。`set_pointer`
/// 为新增第一个供应商，指针随操作落定。
pub(crate) fn write_live(
    db: &Database,
    edits: &TomlEdits,
    key_fields: Option<(Option<&Provider>, &Provider)>,
    set_pointer: bool,
) -> Result<(), AppError> {
    let (prev, projection) = match key_fields {
        Some((prev, target)) => (prev, Some(grok_direct::projection(target)?)),
        None => (None, None),
    };
    if projection.is_none() && edits.is_empty() {
        return Ok(());
    }
    let pointer = key_fields
        .filter(|_| set_pointer)
        .map(|(_, target)| target.id.clone());
    grok_direct::run_with_edits(
        db,
        if pointer.is_some() {
            op::SWITCH
        } else {
            op::APPLY
        },
        prev,
        projection.as_ref(),
        PendingTarget::pointer(pointer),
        Some(edits),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ROW: &str = r#"[models]
default = "grok-4.5"

[model."grok-4.5"]
model = "a"
name = "A"
base_url = "https://a.example/v1"
api_key = "key-a"
api_backend = "responses"
context_window = 500000
"#;

    const LIVE: &str = r#"[ui]
theme = "dark"

[models]
default = "grok-4.5"
web_search = "grok-4.6"

[model."grok-4.5"]
model = "a"
name = "A"
base_url = "https://a.example/v1"
api_key = "key-a"
api_backend = "responses"
context_window = 500000

[model.mine]
model = "m"
"#;

    #[test]
    fn global_changes_skip_the_cc_switch_table_and_the_default() {
        let edited = LIVE
            .replace("theme = \"dark\"", "theme = \"light\"")
            .replace("model = \"a\"", "model = \"b\"")
            .replace("model = \"m\"", "model = \"n\"");
        let plan = plan_save(
            Some(&json!({ "config": ROW })),
            &json!({ "config": edited }),
            &json!({ "config": LIVE }),
            false,
            ConflictPolicy::Refuse,
        )
        .unwrap();
        assert_eq!(plan.edits.paths(), vec!["ui.theme", "model.mine"]);
        let row = plan.row_settings["config"].as_str().unwrap();
        assert!(row.contains("model = \"b\""), "{row}");
        assert!(!row.contains("[ui]"), "global settings go to live: {row}");
    }

    #[test]
    fn renaming_the_table_replaces_the_one_in_the_row() {
        let edited = LIVE.replace("grok-4.5", "grok-4.6");
        let plan = plan_save(
            Some(&json!({ "config": format!("{ROW}\n[mcp_servers.fs]\ncommand = \"x\"\n") })),
            &json!({ "config": edited }),
            &json!({ "config": LIVE }),
            false,
            ConflictPolicy::Refuse,
        )
        .unwrap();
        // 显示里原来的 grok-4.5 和新的 grok-4.6 都是 CC Switch 的表，不算全局改动。
        assert!(plan.edits.is_empty(), "{:?}", plan.edits.paths());
        let row: toml::Table =
            toml::from_str(plan.row_settings["config"].as_str().unwrap()).unwrap();
        assert_eq!(row["models"]["default"].as_str(), Some("grok-4.6"));
        assert!(row["model"].get("grok-4.5").is_none());
        assert!(row["model"].get("grok-4.6").is_some());
        assert!(row.contains_key("mcp_servers"), "the rest of the row stays");
    }

    #[test]
    fn official_cards_keep_their_row() {
        let plan = plan_save(
            Some(&json!({ "config": "" })),
            &json!({ "config": "[ui]\ntheme = \"dark\"\n" }),
            &json!({ "config": "[ui]\ntheme = \"dark\"\n" }),
            true,
            ConflictPolicy::Refuse,
        )
        .unwrap();
        assert_eq!(plan.row_settings, json!({ "config": "" }));
        assert!(plan.edits.is_empty());
    }
}
