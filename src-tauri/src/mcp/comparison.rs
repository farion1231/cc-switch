//! Compare connections in the format the selected client actually receives.
//! Writer defaults and lossy native transport names are not independent settings.

use crate::app_config::AppType;
use serde_json::{json, Value};

pub(crate) fn without_empty_collections(mut spec: Value) -> Value {
    if let Some(object) = spec.as_object_mut() {
        for key in ["args", "env", "headers", "http_headers", "environment"] {
            if object.get(key).is_some_and(|value| {
                value.as_array().is_some_and(Vec::is_empty)
                    || value.as_object().is_some_and(serde_json::Map::is_empty)
            }) {
                object.remove(key);
            }
        }
    }
    spec
}

fn native_connection(app: &AppType, spec: &Value) -> Option<Value> {
    let native = match app {
        AppType::Claude => {
            let path = crate::config::get_claude_mcp_path();
            let mut value = crate::claude_mcp::native_server_spec(
                "comparison",
                spec,
                crate::config::is_wsl_path(&path),
            )
            .ok()?;
            value
                .as_object_mut()?
                .entry("type")
                .or_insert_with(|| json!("stdio"));
            value
        }
        AppType::Codex => table_value(super::codex::json_server_to_codex_toml_table(spec).ok()?)?,
        AppType::GrokBuild => {
            table_value(super::grokbuild::json_server_to_grokbuild_toml_table(spec).ok()?)?
        }
        AppType::Gemini => crate::gemini_mcp::native_server_spec("comparison", spec).ok()?,
        AppType::OpenCode => super::opencode::convert_to_opencode_format(spec).ok()?,
        AppType::Hermes => super::hermes::convert_to_hermes_format(spec).ok()?,
        AppType::Pi => {
            let mut value = super::pi::pi_fields(spec);
            value
                .as_object_mut()?
                .extend(super::pi::native_transport(spec).ok()?.as_object()?.clone());
            value
                .as_object_mut()?
                .entry("type")
                .or_insert_with(|| json!("stdio"));
            value
        }
        AppType::Omp => {
            let mut value = super::pi::unified_spec(spec);
            value.as_object_mut()?.remove("enabled");
            value
        }
        AppType::Mcode | AppType::Kimi | AppType::Agy => {
            let mut value = match app {
                AppType::Mcode => super::mcode::unified_spec(spec),
                AppType::Kimi => super::kimi::unified_spec(spec),
                _ => super::agy::unified_spec(spec),
            };
            value.as_object_mut()?.remove("enabled");
            if *app == AppType::Agy {
                value.as_object_mut()?.remove("disabled");
            }
            value
        }
        AppType::Dsh => {
            let mut value = spec.clone();
            value.as_object_mut()?.insert(
                "type".into(),
                json!(if spec.get("url").is_some() {
                    "http"
                } else {
                    "stdio"
                }),
            );
            value
        }
        AppType::ClaudeDesktop | AppType::OpenClaw => return None,
    };
    Some(without_empty_collections(native))
}

fn table_value(table: toml_edit::Table) -> Option<Value> {
    let mut document = toml_edit::DocumentMut::new();
    document["server"] = toml_edit::Item::Table(table);
    let parsed: toml::Value = toml::from_str(&document.to_string()).ok()?;
    serde_json::to_value(parsed.get("server")?).ok()
}

pub(crate) fn same_connection(app: &AppType, left: &Value, right: &Value) -> bool {
    match (native_connection(app, left), native_connection(app, right)) {
        (Some(projected_left), Some(projected_right)) => projected_left == projected_right,
        _ => left == right,
    }
}
