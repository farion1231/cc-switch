//! model_list 后缀语法解析与 catalog JSON 构建。
//!
//! 后缀语法：`deepseek-v4-pro[1M]` 表示 slug=deepseek-v4-pro、context_window=1000000。
//! 单位 K/k=1000、M/m=1000000；纯数字也接受。后缀在生成 catalog 时剥离。

use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCatalogEntry {
    pub slug: String,
    pub display_name: String,
    /// 来自后缀的窗口值；None 表示该条目无后缀（回落顶层默认）。
    pub suffix_window: Option<u64>,
}

/// 解析单个模型条目的后缀，返回 (slug, 可选窗口)。
/// 括号内非合法窗口 token 时，整串作为 slug 且 window=None（不剥离括号）。
pub fn parse_model_suffix(raw: &str) -> (String, Option<u64>) {
    let raw = raw.trim();
    if let Some(close) = raw.rfind(']') {
        // 仅当 ] 是最后一个字符时才视为后缀
        if close == raw.len() - 1 {
            if let Some(open) = raw[..close].rfind('[') {
                let inner = raw[open + 1..close].trim();
                let slug = raw[..open].trim();
                if !slug.is_empty() {
                    if let Some(window) = parse_window_token(inner) {
                        return (slug.to_string(), Some(window));
                    }
                }
            }
        }
    }
    (raw.to_string(), None)
}

/// 一次性迁移：把旧格式 `slug[suffix]` 的 model_list 拆成无后缀列表和窗口 map。
pub fn migrate_model_list_with_suffixes(model_list: &str) -> (String, HashMap<String, String>) {
    let mut clean_lines = Vec::new();
    let mut windows = HashMap::new();
    for raw in model_list
        .split(['\r', '\n', ','])
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        let (slug, window) = parse_model_suffix(raw);
        clean_lines.push(slug.clone());
        if let Some(window) = window {
            windows.insert(slug, window.to_string());
        }
    }
    (clean_lines.join("\n"), windows)
}

/// 解析括号内的窗口 token，如 "1M" / "200K" / "1000000"。非法或 0 返回 None。
fn parse_window_token(token: &str) -> Option<u64> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    let (num_part, multiplier) = match token.chars().last() {
        Some('K' | 'k') => (&token[..token.len() - 1], 1_000u64),
        Some('M' | 'm') => (&token[..token.len() - 1], 1_000_000u64),
        Some(_) => (token, 1u64),
        None => return None,
    };
    num_part
        .trim()
        .parse::<u64>()
        .ok()
        .map(|value| value * multiplier)
        .filter(|value| *value > 0)
}

/// 收集 profile 的全部模型条目（当前 model + model_list），去重并从 `model_windows` map 读取窗口。
/// 返回顺序：当前 model 在前。用于生成 catalog，包含全部模型以避免
/// #1064 单模型副作用（catalog 只剩当前 model）。
///
/// 当前 model 若不带后缀，但在 `model_windows` 中存在同名条目，
/// 则采纳该窗口（让当前 model 的窗口也能生效）。
pub fn collect_catalog_entries(
    model_list: &str,
    model_windows: &HashMap<String, String>,
    current_model: &str,
) -> Vec<ModelCatalogEntry> {
    // 先解析 model_list，保留顺序并去重；后缀已从 model_list 剥离，窗口来自 model_windows map。
    let mut seen = HashSet::new();
    let mut list_entries: Vec<ModelCatalogEntry> = Vec::new();
    for raw in model_list
        .split(['\r', '\n', ','])
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let (slug, _) = parse_model_suffix(raw);
        if slug.is_empty() {
            continue;
        }
        if !seen.insert(slug.clone()) {
            continue;
        }
        let suffix_window = model_windows
            .get(&slug)
            .and_then(|token| parse_window_token(token));
        list_entries.push(ModelCatalogEntry {
            display_name: slug.clone(),
            slug,
            suffix_window,
        });
    }

    // 处理当前 model，放到最前面。
    let current_model = current_model.trim();
    let mut entries = Vec::new();
    if !current_model.is_empty() {
        let (slug, _) = parse_model_suffix(current_model);
        if !slug.is_empty() {
            let suffix_window = model_windows
                .get(&slug)
                .and_then(|token| parse_window_token(token));
            entries.push(ModelCatalogEntry {
                display_name: slug.clone(),
                slug: slug.clone(),
                suffix_window,
            });
            // 从 list_entries 中移除同 slug 条目，避免重复。
            list_entries.retain(|entry| entry.slug != slug);
        }
    }

    entries.append(&mut list_entries);
    entries
}

/// 内置 codex bundled catalog 模板（assets/codex-models.json），用于 clone entry
/// 保证字段齐全，避免 codex 因缺字段忽略条目。
const BUNDLED_TEMPLATE_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/codex-models.json"
));

/// 构建 codex model_catalog_json 内容。
///
/// 采用 cc-switch 的 template-clone 思路：取 codex 自带 bundled entry 做模板，
/// 再覆盖 slug / display_name / description / context_window / max_context_window /
/// effective_context_window_percent / priority / auto_compact_token_limit 等字段。
/// 无后缀条目用 fallback_window；fallback 也无时回落 272000（codex 默认）。
/// auto_compact_token_limit 留 null：codex 内置模型即 null（按比例算，调研第六节）。
pub fn build_model_catalog_json(
    entries: &[ModelCatalogEntry],
    fallback_window: Option<u64>,
) -> String {
    build_model_catalog_json_with_template(entries, fallback_window, None)
}

/// 使用指定模板（或内置 bundled 模板）构建 catalog。
/// `template` 为单个 model entry 的 JSON Value；为 None 时使用内置模板的第一条。
pub fn build_model_catalog_json_with_template(
    entries: &[ModelCatalogEntry],
    fallback_window: Option<u64>,
    template: Option<&Value>,
) -> String {
    let template = template
        .cloned()
        .or_else(|| load_bundled_template_entry())
        .unwrap_or_else(|| json!({}));

    let models: Vec<Value> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let context_window = entry.suffix_window.or(fallback_window).unwrap_or(272_000);
            let mut model = template.clone();
            model["slug"] = json!(entry.slug);
            model["display_name"] = json!(entry.display_name);
            model["description"] = json!(entry.display_name);
            model["context_window"] = json!(context_window);
            model["max_context_window"] = json!(context_window);
            // 默认 95 会让 1M 显示为 950K，显式写 100 以显示真实窗口。
            model["effective_context_window_percent"] = json!(100);
            model["auto_compact_token_limit"] = Value::Null;
            model["priority"] = json!(1000 + index);
            model["visibility"] = json!("list");
            model["supported_in_api"] = json!(true);
            model["additional_speed_tiers"] = json!([]);
            model["service_tiers"] = json!([]);
            model["availability_nux"] = Value::Null;
            model["upgrade"] = Value::Null;
            model
        })
        .collect();
    serde_json::to_string_pretty(&json!({ "models": models })).unwrap_or_default()
}

/// 加载内置 bundled catalog 模板的第一条 model entry。
fn load_bundled_template_entry() -> Option<Value> {
    let catalog: Value = serde_json::from_str(BUNDLED_TEMPLATE_JSON).ok()?;
    catalog.get("models")?.as_array()?.first().cloned()
}
