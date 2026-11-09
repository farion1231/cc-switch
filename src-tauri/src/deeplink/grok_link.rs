//! Grok Build 供应商链接附带的 `config.toml`：只收选中的那张 `[model."<名称>"]` 表。
//!
//! 一个供应商在 live 里只拥有一张模型表（`live/project/grok.rs`），所以链接里的其他表不
//! 导入。选中的表按白名单收：显示名、说明、接口类型、窗口、采样和重试参数等照搬；地址、
//! Key、模型名用合并后的值（URL 参数优先）。
//!
//! 链接是不可信输入：让 Grok 运行凭据提供器（`auth_provider`）、读环境变量
//! （`env_http_headers`）或本机证书（`mtls_cert_dir`）的表直接拒绝；`env_key` 丢掉，Key
//! 用链接自己给的（只给了 `env_key` 的链接由调用方拒绝）。

use toml_edit::{DocumentMut, Item, Table, TableLike, Value};

use crate::error::AppError;
use crate::grok_config::{DEFAULT_API_BACKEND, DEFAULT_CONTEXT_WINDOW};

/// 可以从链接带进来的键（Grok Build 1.0.50 的模型表，去掉地址、凭据和下面拒绝的）。
const PROFILE_KEYS: &[&str] = &[
    "name",
    "description",
    "notice",
    "model_family",
    "api_backend",
    "auth_scheme",
    "extra_headers",
    "query_params",
    "context_window",
    "context_windows",
    "max_completion_tokens",
    "temperature",
    "top_p",
    "max_request_bytes",
    "auto_compact_threshold_percent",
    "system_prompt_label",
    "use_concise",
    "agent_type",
    "inference_idle_timeout_secs",
    "max_retries",
    "rate_limit_retry_threshold",
    "subagent_rate_limit_max_attempts",
    "hidden",
    "supports_reasoning_effort",
    "reasoning_efforts",
    "supports_backend_search",
    "compaction_at_tokens",
    "show_model_fingerprint",
    "stream_tool_calls",
    "reasoning_summary",
];

/// 运行凭据提供器、读环境变量或本机证书：链接带了就拒绝导入。
const REJECTED_PROFILE_KEYS: &[&str] = &["auth_provider", "env_http_headers", "mtls_cert_dir"];

/// 链接里选中的模型表，已按白名单过滤。
pub(super) struct GrokLinkProfile {
    /// 表名（`models.default` 指向的名字）。
    pub name: String,
    table: Table,
    model: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
    /// 表用 `env_key` 取 Key。
    pub declares_env_key: bool,
}

impl GrokLinkProfile {
    pub(super) fn parse(text: &str) -> Result<Self, AppError> {
        let doc = text
            .parse::<DocumentMut>()
            .map_err(|e| AppError::InvalidInput(format!("Invalid Grok Build config.toml: {e}")))?;
        let (name, source) = selected_profile(&doc)?;
        if let Some(key) = REJECTED_PROFILE_KEYS
            .iter()
            .find(|key| source.get(key).is_some())
        {
            return Err(AppError::InvalidInput(format!(
                "The Grok Build config uses '{key}', which makes Grok run a credential provider or \
                 read credentials from this computer; it cannot be imported from a link. Add the \
                 provider manually instead."
            )));
        }

        let mut table = Table::new();
        for key in PROFILE_KEYS {
            if let Some(item) = source.get(key) {
                table.insert(key, undecorated(item));
            }
        }
        Ok(Self {
            name,
            table,
            model: non_empty_str(source.get("model")),
            base_url: non_empty_str(source.get("base_url")),
            api_key: non_empty_str(source.get("api_key")),
            declares_env_key: source.get("env_key").is_some(),
        })
    }

    pub(super) fn model(&self) -> Option<String> {
        self.model.clone()
    }

    pub(super) fn base_url(&self) -> Option<String> {
        self.base_url.clone()
    }

    pub(super) fn api_key(&self) -> Option<String> {
        self.api_key.clone()
    }

    /// 写成供应商行的 `config`。地址、Key、模型名用合并后的值；表里没写显示名、接口类型、
    /// 窗口时补上和模板一样的默认值（live 的形状校验要求它们）。
    pub(super) fn render(
        &self,
        display_name: &str,
        model: &str,
        endpoint: &str,
        api_key: &str,
    ) -> String {
        let mut table = Table::new();
        table.insert("model", toml_edit::value(model));
        table.insert("base_url", toml_edit::value(endpoint));
        table.insert(
            "name",
            self.table
                .get("name")
                .filter(|item| non_empty_str(Some(item)).is_some())
                .cloned()
                .unwrap_or_else(|| toml_edit::value(display_name)),
        );
        table.insert("api_key", toml_edit::value(api_key));
        table.insert(
            "api_backend",
            self.table
                .get("api_backend")
                .cloned()
                .unwrap_or_else(|| toml_edit::value(DEFAULT_API_BACKEND)),
        );
        table.insert(
            "context_window",
            self.table
                .get("context_window")
                .cloned()
                .unwrap_or_else(|| toml_edit::value(DEFAULT_CONTEXT_WINDOW)),
        );
        for (key, item) in self.table.iter() {
            if !table.contains_key(key) {
                table.insert(key, item.clone());
            }
        }

        let mut doc = DocumentMut::new();
        let mut models = Table::new();
        models.insert("default", toml_edit::value(&self.name));
        doc.insert("models", Item::Table(models));
        let mut model_tables = Table::new();
        model_tables.set_implicit(true);
        model_tables.insert(&self.name, Item::Table(table));
        doc.insert("model", Item::Table(model_tables));
        doc.to_string()
    }
}

/// `models.default` 指向的表；没写或指不到时取唯一的一张表。
fn selected_profile(doc: &DocumentMut) -> Result<(String, &dyn TableLike), AppError> {
    let tables = doc
        .get("model")
        .and_then(Item::as_table_like)
        .ok_or_else(|| {
            AppError::InvalidInput(
                "The Grok Build config has no [model.\"<name>\"] table to import".to_string(),
            )
        })?;
    let default = non_empty_str(doc.get("models").and_then(|models| models.get("default")));
    if let Some(name) = default {
        if let Some(table) = tables.get(&name).and_then(Item::as_table_like) {
            return Ok((name, table));
        }
    }
    let mut candidates = tables
        .iter()
        .filter_map(|(name, item)| Some((name.to_string(), item.as_table_like()?)));
    match (candidates.next(), candidates.next()) {
        (Some(only), None) => Ok(only),
        (None, _) => Err(AppError::InvalidInput(
            "The Grok Build config has no [model.\"<name>\"] table to import".to_string(),
        )),
        _ => Err(AppError::InvalidInput(
            "The Grok Build config has several [model.*] tables; set [models] default to the one \
             to import"
                .to_string(),
        )),
    }
}

fn non_empty_str(item: Option<&Item>) -> Option<String> {
    item.and_then(Item::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn undecorated_value(value: &Value) -> Value {
    let mut value = value.clone();
    value.decor_mut().clear();
    value
}

/// 去掉原文的注释和空白；子表保留成子表。
fn undecorated(item: &Item) -> Item {
    match item {
        Item::Value(value) => Item::Value(undecorated_value(value)),
        Item::Table(table) => {
            let mut table = table.clone();
            table.decor_mut().clear();
            table.set_implicit(false);
            for (_, value) in table.iter_mut() {
                if let Some(value) = value.as_value_mut() {
                    value.decor_mut().clear();
                }
            }
            Item::Table(table)
        }
        other => other.clone(),
    }
}
