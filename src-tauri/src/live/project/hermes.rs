//! Hermes 的投影与补丁：路由供应商 → `config.yaml` 的 `custom_providers` 条目和 `model:` 段。
//!
//! Hermes 是累加式应用：供应商条目在 `custom_providers` 里共存。代理模式只多写一条指向
//! 本地代理的路由条目（名字固定 [`ROUTE_ENTRY_NAME`]，专属 CC Switch），并把顶层 `model:`
//! 段指过去；退出代理删这条、把 `model:` 段指回直连指针那家。用户自己的其余条目、段落、
//! 注释字节不动（段级替换，见 [`crate::hermes_config::replace_yaml_section`]）。
//!
//! 补丁以现有文件为底：解析失败报错、绝不从空文档写起（`LiveWriteError::Parse`）。

use std::path::Path;

use serde_json::{json, Value};

use crate::error::AppError;
use crate::live::patch::{KeyPath, LivePatch, LiveWriteError};

/// CC Switch 写进 `custom_providers` 的路由条目名。这个名字专属 CC Switch，退出代理
/// 按它删除；用户手工建同名条目会被代理模式接管。
pub const ROUTE_ENTRY_NAME: &str = "cc-switch-route";

/// 代理条目对 Hermes 声明的协议。本地代理只完整实现 Responses（SSE、`session_id`
/// 会话粘性、`/responses/compact` 压缩），`chat_completions` 留作手改 `api_mode` 的
/// 兜底路由。
pub const PROXY_API_MODE: &str = "codex_responses";

/// 要写进 live 的目标状态：`custom_providers` 里的路由条目 + `model:` 段的指向。
///
/// 投影是纯函数（不读盘）；和现有文件合并发生在补丁 [`HermesConfigPatch`] 里：
/// `model:` 段的其余键（`context_length` / `max_tokens` / 未知键）以现有文件为底保留。
#[derive(Debug, Clone, PartialEq)]
pub struct HermesProjection {
    /// 写进 `custom_providers` 的路由条目（JSON，`models` 用 UI 数组形态，落盘前由
    /// [`crate::hermes_config::build_custom_provider_entry`] 规范化）；`None` = 删掉
    /// CC Switch 写的那条（退出代理）。
    pub entry: Option<Value>,
    /// `model.provider` 的目标值；空串 = 删掉这个键（Hermes 回落到内置供应商）。
    pub model_provider: String,
    /// `model.default` 的覆盖值；`None` = 保留现有默认（与 `apply_switch_defaults`
    /// 「新供应商不声明模型就不动默认」的语义一致）。
    pub model_default: Option<String>,
}

impl HermesProjection {
    /// 代理契约：路由条目指向本地代理（`{proxy_base}/responses`，codex_responses），
    /// 发布路由供应商声明的模型列表，`model:` 段指向路由条目。
    pub fn proxy_contract(
        route_settings: &Value,
        proxy_base_url: &str,
        placeholder: &str,
    ) -> Result<Self, AppError> {
        let models = route_settings
            .get("models")
            .cloned()
            .unwrap_or_else(|| json!([]));
        let entry = json!({
            "name": ROUTE_ENTRY_NAME,
            "base_url": proxy_base_url,
            "api_key": placeholder,
            "api_mode": PROXY_API_MODE,
            // Hermes 对自定义供应商默认不带任何会话标识（#86241：端点没要就不发）。
            // 声明这个 opt-in 字段后，每轮请求（含压缩/标题等辅助调用）都会带
            // `session_id: <会话根 id>` 头，本地代理据此做会话粘性负载均衡；
            // 不认识该字段的旧版 Hermes 会忽略它，无副作用。
            "session_affinity_header": "session_id",
            "models": models,
        });
        Ok(Self {
            entry: Some(entry),
            model_provider: ROUTE_ENTRY_NAME.to_string(),
            model_default: first_model_id(route_settings),
        })
    }

    /// 直连投影：删路由条目，`model:` 段指回直连指针那家；没有直连指针就删掉
    /// `model.provider`（Hermes 回落到内置供应商）。
    pub fn direct(direct: Option<(&str, &Value)>) -> Self {
        match direct {
            Some((id, settings)) => Self {
                entry: None,
                model_provider: id.to_string(),
                model_default: first_model_id(settings),
            },
            None => Self {
                entry: None,
                model_provider: String::new(),
                model_default: None,
            },
        }
    }

    /// 契约摘要用的规范形式。
    pub fn to_value(&self) -> Value {
        json!({
            "entry": self.entry,
            "model_provider": self.model_provider,
            "model_default": self.model_default,
        })
    }
}

/// 行里声明的第一个模型 id（UI 数组形态）；没有声明模型时为 `None`。
fn first_model_id(settings: &Value) -> Option<String> {
    settings
        .get("models")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|m| m.get("id"))
        .and_then(|id| id.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// 按投影改写 `config.yaml`：路由条目 upsert / 删除，`model:` 段换指向。
pub struct HermesConfigPatch<'a> {
    projection: &'a HermesProjection,
}

impl<'a> HermesConfigPatch<'a> {
    pub fn new(projection: &'a HermesProjection) -> Self {
        Self { projection }
    }
}

impl LivePatch for HermesConfigPatch<'_> {
    fn apply(&self, path: &Path, pre: Option<&[u8]>) -> Result<Vec<u8>, LiveWriteError> {
        use serde_yaml::Value as Yaml;

        let raw = pre
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .unwrap_or_default();
        let config: Yaml =
            serde_yaml::from_str(&raw).map_err(|error| yaml_parse_error(path, &error))?;

        // 1) custom_providers：路由条目 upsert（按 name 匹配）或删除。
        let mut providers: Vec<Yaml> = config
            .get("custom_providers")
            .and_then(|v| v.as_sequence())
            .cloned()
            .unwrap_or_default();
        match &self.projection.entry {
            Some(entry) => {
                let yaml_entry = crate::hermes_config::build_custom_provider_entry(
                    ROUTE_ENTRY_NAME,
                    entry.clone(),
                )
                .map_err(|_| LiveWriteError::Shape {
                    path: path.to_path_buf(),
                    key_path: KeyPath::new(&["custom_providers", ROUTE_ENTRY_NAME]),
                    expected: "合法的供应商条目",
                })?;
                match providers
                    .iter_mut()
                    .find(|p| p.get("name").and_then(Yaml::as_str) == Some(ROUTE_ENTRY_NAME))
                {
                    Some(existing) => *existing = yaml_entry,
                    None => providers.push(yaml_entry),
                }
            }
            None => {
                providers.retain(|p| p.get("name").and_then(Yaml::as_str) != Some(ROUTE_ENTRY_NAME))
            }
        }
        let mut raw = crate::hermes_config::replace_yaml_section(
            &raw,
            "custom_providers",
            &Yaml::Sequence(providers),
        )
        .map_err(|_| LiveWriteError::Shape {
            path: path.to_path_buf(),
            key_path: KeyPath::new(&["custom_providers"]),
            expected: "可序列化的 YAML 段",
        })?;

        // 2) model 段：以现有内容为底，只换 provider / default 两个键。
        let had_model = config.get("model").is_some_and(|v| !v.is_null());
        let needs_model_write = had_model
            || !self.projection.model_provider.is_empty()
            || self.projection.model_default.is_some();
        if needs_model_write {
            let mut model = config
                .get("model")
                .and_then(|v| v.as_mapping())
                .cloned()
                .unwrap_or_default();
            let key = Yaml::String("provider".to_string());
            if self.projection.model_provider.is_empty() {
                model.remove(&key);
            } else {
                model.insert(key, Yaml::String(self.projection.model_provider.clone()));
            }
            if let Some(default) = &self.projection.model_default {
                model.insert(
                    Yaml::String("default".to_string()),
                    Yaml::String(default.clone()),
                );
            }
            raw = crate::hermes_config::replace_yaml_section(&raw, "model", &Yaml::Mapping(model))
                .map_err(|_| LiveWriteError::Shape {
                    path: path.to_path_buf(),
                    key_path: KeyPath::new(&["model"]),
                    expected: "可序列化的 YAML 段",
                })?;
        }

        Ok(raw.into_bytes())
    }
}

/// serde_yaml 解析错误 → 补丁层的 `Parse`（带行列），绝不退回空文档。
fn yaml_parse_error(path: &Path, error: &serde_yaml::Error) -> LiveWriteError {
    let (line, column) = error
        .location()
        .map(|location| (location.line(), location.column()))
        .unwrap_or((0, 0));
    LiveWriteError::Parse {
        path: path.to_path_buf(),
        line,
        column,
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route_settings() -> Value {
        json!({
            "name": "kimi",
            "base_url": "https://api.kimi.com/coding/",
            "api_key": "sk-live",
            "api_mode": "codex_responses",
            "models": [
                { "id": "kimi-k2-0905", "context_length": 262144 },
                { "id": "kimi-k2-thinking", "context_length": 262144 },
            ],
        })
    }

    fn apply_on(raw: &str, projection: &HermesProjection) -> String {
        let patch = HermesConfigPatch::new(projection);
        let bytes = patch
            .apply(Path::new("/tmp/config.yaml"), Some(raw.as_bytes()))
            .expect("patch applies");
        String::from_utf8(bytes).expect("utf-8")
    }

    #[test]
    fn proxy_patch_upserts_route_entry_and_points_model_at_it() {
        let raw = "model:\n  default: kimi-k2-0905\n  provider: kimi\ncustom_providers:\n  - name: kimi\n    base_url: https://api.kimi.com/coding/\n    api_key: sk-live\n";
        let projection = HermesProjection::proxy_contract(
            &route_settings(),
            "http://127.0.0.1:15721/hermes/v1",
            "sk-cc-switch-managed",
        )
        .expect("contract");

        let out = apply_on(raw, &projection);
        let yaml: serde_yaml::Value = serde_yaml::from_str(&out).expect("parses");
        // 路由条目就位，字段指向本地代理
        let entry = yaml
            .get("custom_providers")
            .and_then(|v| v.as_sequence())
            .expect("custom_providers")
            .iter()
            .find(|p| p.get("name").and_then(|n| n.as_str()) == Some(ROUTE_ENTRY_NAME))
            .expect("route entry present");
        assert_eq!(
            entry.get("base_url").and_then(|v| v.as_str()),
            Some("http://127.0.0.1:15721/hermes/v1")
        );
        assert_eq!(
            entry.get("api_mode").and_then(|v| v.as_str()),
            Some("codex_responses")
        );
        assert_eq!(
            entry
                .get("session_affinity_header")
                .and_then(|v| v.as_str()),
            Some("session_id")
        );
        // models 数组转成了字典，首模型进单数 model 字段
        assert!(entry.get("models").and_then(|v| v.as_mapping()).is_some());
        assert_eq!(
            entry.get("model").and_then(|v| v.as_str()),
            Some("kimi-k2-0905")
        );
        // model 段换指向，其余键（default 以外）保留
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("provider"))
                .and_then(|v| v.as_str()),
            Some(ROUTE_ENTRY_NAME)
        );
        // 用户条目原样保留
        assert!(yaml
            .get("custom_providers")
            .and_then(|v| v.as_sequence())
            .expect("custom_providers")
            .iter()
            .any(|p| p.get("name").and_then(|n| n.as_str()) == Some("kimi")));
    }

    #[test]
    fn direct_patch_removes_route_entry_and_restores_model_provider() {
        let raw = format!(
            "model:\n  provider: {ROUTE_ENTRY_NAME}\n  default: kimi-k2-0905\ncustom_providers:\n  - name: kimi\n  - name: {ROUTE_ENTRY_NAME}\n"
        );
        let projection = HermesProjection::direct(Some(("kimi", &route_settings())));

        let out = apply_on(&raw, &projection);
        let yaml: serde_yaml::Value = serde_yaml::from_str(&out).expect("parses");
        let providers = yaml
            .get("custom_providers")
            .and_then(|v| v.as_sequence())
            .expect("custom_providers");
        assert_eq!(providers.len(), 1, "路由条目被删，用户条目保留");
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("provider"))
                .and_then(|v| v.as_str()),
            Some("kimi")
        );
    }

    #[test]
    fn direct_patch_without_provider_clears_model_provider() {
        let raw = format!(
            "model:\n  provider: {ROUTE_ENTRY_NAME}\n  default: kimi-k2-0905\ncustom_providers:\n  - name: {ROUTE_ENTRY_NAME}\n"
        );
        let projection = HermesProjection::direct(None);

        let out = apply_on(&raw, &projection);
        let yaml: serde_yaml::Value = serde_yaml::from_str(&out).expect("parses");
        assert!(yaml.get("model").and_then(|m| m.get("provider")).is_none());
        // default 保留，不跟着清掉
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("default"))
                .and_then(|v| v.as_str()),
            Some("kimi-k2-0905")
        );
    }

    #[test]
    fn unparseable_config_is_never_rewritten_from_scratch() {
        let raw = "custom_providers: [ not closed";
        let projection = HermesProjection::direct(None);
        let patch = HermesConfigPatch::new(&projection);
        let result = patch.apply(Path::new("/tmp/config.yaml"), Some(raw.as_bytes()));
        assert!(matches!(result, Err(LiveWriteError::Parse { .. })));
    }
}
