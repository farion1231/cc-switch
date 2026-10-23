//! Hermes 的投影与补丁：路由供应商 → `config.yaml` 的 `custom_providers` 条目和 `model:` 段。
//!
//! Hermes 是累加式应用：供应商条目在 `custom_providers` 里共存。代理模式只多写一条指向
//! 本地代理的路由条目（名字固定 [`ROUTE_ENTRY_NAME`]，专属 CC Switch），并把顶层 `model:`
//! 段指过去；退出代理删这条、把 `model:` 段恢复原样。用户自己的其余条目、段落、注释字节
//! 不动（段级替换，见 [`crate::hermes_config::replace_yaml_section`]）。
//!
//! Hermes 的「多个 agent」是命名 profile（`<home>/profiles/<name>/`，各自独立的
//! config.yaml，不从默认 home 继承）。统一路由模式对每个 home 各跑一份本补丁；
//! `model.default` 在路由那家仍在提供的模型里就保留（各 agent 自己的模型选择不被
//! 破坏），否则换首个模型，退出时按进入时记录的原值恢复。
//!
//! 补丁以现有文件为底：解析失败报错、绝不从空文档写起（`LiveWriteError::Parse`）。

use std::path::Path;

use serde_json::{json, Value};

use crate::error::AppError;
use crate::live::patch::{KeyPath, LivePatch, LiveWriteError};

/// CC Switch 写进 `custom_providers` 的路由条目名。这个名字专属 CC Switch，退出代理
/// 按它删除；用户手工建同名条目会被代理模式接管。
pub const ROUTE_ENTRY_NAME: &str = "cc-switch-route";

/// 旧版接管写的条目名与占位 Key（识别残留用：只清这个名字 + 这个占位符的组合）。
pub const LEGACY_TAKEOVER_ENTRY: &str = "cc-switch";
pub const LEGACY_TAKEOVER_TOKEN: &str = "PROXY_MANAGED";

/// 代理条目对 Hermes 声明的协议。本地代理只完整实现 Responses（SSE、`session_id`
/// 会话粘性、`/responses/compact` 压缩），`chat_completions` 留作手改 `api_mode` 的
/// 兜底路由。
pub const PROXY_API_MODE: &str = "codex_responses";

/// `model:` 段的目标状态。
#[derive(Debug, Clone, PartialEq)]
pub enum ModelTarget {
    /// 进入代理：`provider` 指向路由条目；`default` 在路由条目发布的模型列表里就
    /// 保留，否则换成首个模型（没有模型就不动）。
    Route,
    /// 退出代理：`provider` 恢复记录值（`None` = 原本没有这个键，删掉，Hermes 回落
    /// 内置供应商）；`default` 有记录值就恢复，`None` = 不动（原本没有默认的 home
    /// 保留路由期的可用默认，比删掉后无默认更稳）。
    Restore {
        provider: Option<String>,
        default: Option<String>,
    },
    /// 退出代理但这个 home 没有进入时的记录（进入之后新增的 profile）：只删路由
    /// 条目，`model:` 段原样不动。
    Untouched,
}

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
    /// `model:` 段的目标。
    pub model: ModelTarget,
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
            model: ModelTarget::Route,
        })
    }

    /// 默认 home 的直连投影：删路由条目，`model:` 段指回直连指针那家（没有就删掉
    /// 指向）。默认 home 是 CC Switch 托管的面（切换供应商就写它），按直连指针恢复。
    pub fn direct(direct: Option<(&str, &Value)>) -> Self {
        match direct {
            Some((id, settings)) => Self {
                entry: None,
                model: ModelTarget::Restore {
                    provider: Some(id.to_string()),
                    default: first_model_id(settings),
                },
            },
            None => Self {
                entry: None,
                model: ModelTarget::Restore {
                    provider: None,
                    default: None,
                },
            },
        }
    }

    /// 契约摘要用的规范形式（只反映进入代理的目标状态）。
    pub fn to_value(&self) -> Value {
        json!({
            "entry": self.entry,
            "model": match &self.model {
                ModelTarget::Route => "route",
                ModelTarget::Restore { .. } => "restore",
                ModelTarget::Untouched => "untouched",
            },
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

/// 按投影改写 `config.yaml`：路由条目 upsert / 删除（顺带清旧接管残留），`model:` 段
/// 换指向。
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

        // 1) custom_providers：路由条目 upsert（按 name 匹配）或删除；进入代理时顺带
        //    清掉旧版接管的残留条目（同名 cc-switch + PROXY_MANAGED 占位 Key）。
        let mut providers: Vec<Yaml> = config
            .get("custom_providers")
            .and_then(|v| v.as_sequence())
            .cloned()
            .unwrap_or_default();
        let removing = self.projection.entry.is_none();
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
                let published: Vec<String> = entry
                    .get("models")
                    .and_then(|v| v.as_array())
                    .map(|models| {
                        models
                            .iter()
                            .filter_map(|m| m.get("id").and_then(Value::as_str))
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default();
                // 先清两代接管残留，再落现行条目——顺序反了会把刚写入的条目清掉
                providers.retain(|p| !is_legacy_takeover_entry(p));
                match providers
                    .iter_mut()
                    .find(|p| p.get("name").and_then(Yaml::as_str) == Some(ROUTE_ENTRY_NAME))
                {
                    Some(existing) => *existing = yaml_entry,
                    None => providers.push(yaml_entry),
                }
                let _ = published;
            }
            None => {
                providers.retain(|p| p.get("name").and_then(Yaml::as_str) != Some(ROUTE_ENTRY_NAME))
            }
        }
        if providers.is_empty() && config.get("custom_providers").is_none() && !removing {
            // 原本没有这一段且要写条目：照常写。
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

        // 2) model 段：以现有内容为底，按目标换 provider / default。
        let had_model = config.get("model").is_some_and(|v| !v.is_null());
        match &self.projection.model {
            ModelTarget::Untouched => {}
            model_target => {
                let mut model = config
                    .get("model")
                    .and_then(|v| v.as_mapping())
                    .cloned()
                    .unwrap_or_default();
                let (provider, default) = match model_target {
                    ModelTarget::Route => {
                        let published: Vec<String> = self
                            .projection
                            .entry
                            .as_ref()
                            .and_then(|entry| entry.get("models"))
                            .and_then(|v| v.as_array())
                            .map(|models| {
                                models
                                    .iter()
                                    .filter_map(|m| m.get("id").and_then(Value::as_str))
                                    .map(String::from)
                                    .collect()
                            })
                            .unwrap_or_default();
                        let keep = model
                            .get(&Yaml::String("default".to_string()))
                            .and_then(Yaml::as_str)
                            .is_some_and(|current| published.iter().any(|m| m == current));
                        let fallback = published.first().cloned();
                        (
                            Some(ROUTE_ENTRY_NAME.to_string()),
                            if keep { None } else { fallback },
                        )
                    }
                    ModelTarget::Restore { provider, default } => {
                        (provider.clone(), default.clone())
                    }
                    ModelTarget::Untouched => unreachable!(),
                };
                if !had_model && provider.is_none() && default.is_none() {
                    return Ok(raw.into_bytes());
                }
                let key = Yaml::String("provider".to_string());
                match provider {
                    Some(value) => {
                        model.insert(key, Yaml::String(value));
                    }
                    None => {
                        model.remove(&key);
                    }
                }
                if let Some(value) = default {
                    model.insert(Yaml::String("default".to_string()), Yaml::String(value));
                }
                raw = crate::hermes_config::replace_yaml_section(
                    &raw,
                    "model",
                    &Yaml::Mapping(model),
                )
                .map_err(|_| LiveWriteError::Shape {
                    path: path.to_path_buf(),
                    key_path: KeyPath::new(&["model"]),
                    expected: "可序列化的 YAML 段",
                })?;
            }
        }

        Ok(raw.into_bytes())
    }
}

/// 旧版接管残留：占位 Key 是 `PROXY_MANAGED` 的路由条目。条目名有两代——
/// 官方旧接管的 `cc-switch` 与现行路由的 `cc-switch-route`（profile 里上次
/// 接管留下的同名条目）。调用方先清后写，现行条目由本次 upsert 重新落位。
fn is_legacy_takeover_entry(entry: &serde_yaml::Value) -> bool {
    let name = entry.get("name").and_then(|v| v.as_str());
    (name == Some(LEGACY_TAKEOVER_ENTRY) || name == Some(ROUTE_ENTRY_NAME))
        && entry.get("api_key").and_then(|v| v.as_str()) == Some(LEGACY_TAKEOVER_TOKEN)
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

    fn proxy_projection() -> HermesProjection {
        HermesProjection::proxy_contract(
            &route_settings(),
            "http://127.0.0.1:15721/hermes/v1",
            "sk-cc-switch-managed",
        )
        .expect("contract")
    }

    #[test]
    fn proxy_patch_upserts_route_entry_and_points_model_at_it() {
        let raw = "model:\n  default: kimi-k2-0905\n  provider: kimi\ncustom_providers:\n  - name: kimi\n    base_url: https://api.kimi.com/coding/\n    api_key: sk-live\n";
        let out = apply_on(raw, &proxy_projection());
        let yaml: serde_yaml::Value = serde_yaml::from_str(&out).expect("parses");
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
        assert!(entry.get("models").and_then(|v| v.as_mapping()).is_some());
        assert_eq!(
            entry.get("model").and_then(|v| v.as_str()),
            Some("kimi-k2-0905")
        );
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("provider"))
                .and_then(|v| v.as_str()),
            Some(ROUTE_ENTRY_NAME)
        );
        assert!(yaml
            .get("custom_providers")
            .and_then(|v| v.as_sequence())
            .expect("custom_providers")
            .iter()
            .any(|p| p.get("name").and_then(|n| n.as_str()) == Some("kimi")));
    }

    #[test]
    fn proxy_patch_keeps_default_when_route_still_serves_it() {
        // profile home 的默认模型在路由那家的列表里：保留，不换首模型。
        let raw = "model:\n  default: kimi-k2-thinking\n  provider: kimi\ncustom_providers: []\n";
        let out = apply_on(raw, &proxy_projection());
        let yaml: serde_yaml::Value = serde_yaml::from_str(&out).expect("parses");
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("default"))
                .and_then(|v| v.as_str()),
            Some("kimi-k2-thinking")
        );
    }

    #[test]
    fn proxy_patch_swaps_default_route_does_not_serve() {
        let raw = "model:\n  default: glm-5.3-flash\n  provider: kimi\ncustom_providers: []\n";
        let out = apply_on(raw, &proxy_projection());
        let yaml: serde_yaml::Value = serde_yaml::from_str(&out).expect("parses");
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("default"))
                .and_then(|v| v.as_str()),
            Some("kimi-k2-0905"),
            "路由不提供的默认模型换成首个发布模型"
        );
    }

    #[test]
    fn proxy_patch_removes_legacy_takeover_entry() {
        let raw = format!(
            "model:\n  default: aim\n  provider: custom:{LEGACY_TAKEOVER_ENTRY}\ncustom_providers:\n  - name: {LEGACY_TAKEOVER_ENTRY}\n    api_key: {LEGACY_TAKEOVER_TOKEN}\n    base_url: http://127.0.0.1:15721\n  - name: mine\n"
        );
        let out = apply_on(&raw, &proxy_projection());
        let yaml: serde_yaml::Value = serde_yaml::from_str(&out).expect("parses");
        let names: Vec<&str> = yaml
            .get("custom_providers")
            .and_then(|v| v.as_sequence())
            .expect("custom_providers")
            .iter()
            .filter_map(|p| p.get("name").and_then(|n| n.as_str()))
            .collect();
        assert!(!names.contains(&LEGACY_TAKEOVER_ENTRY), "旧接管残留被清掉");
        assert!(names.contains(&"mine") && names.contains(&ROUTE_ENTRY_NAME));
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
    fn direct_patch_restores_recorded_profile_state() {
        // profile home 的恢复：按进入时记录的原值，而不是直连指针。
        let raw = format!(
            "model:\n  provider: {ROUTE_ENTRY_NAME}\n  default: glm-5.3-flash\n  context_length: 131072\ncustom_providers:\n  - name: {ROUTE_ENTRY_NAME}\n"
        );
        let projection = HermesProjection {
            entry: None,
            model: ModelTarget::Restore {
                provider: Some("custom:cc-switch".to_string()),
                default: Some("aim".to_string()),
            },
        };
        let out = apply_on(&raw, &projection);
        let yaml: serde_yaml::Value = serde_yaml::from_str(&out).expect("parses");
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("provider"))
                .and_then(|v| v.as_str()),
            Some("custom:cc-switch")
        );
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("default"))
                .and_then(|v| v.as_str()),
            Some("aim")
        );
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("context_length"))
                .and_then(|v| v.as_i64()),
            Some(131072),
            "model 段其余键保留"
        );
    }

    #[test]
    fn direct_patch_untouched_leaves_model_section_alone() {
        let raw = format!(
            "model:\n  provider: other\n  default: whatever\ncustom_providers:\n  - name: other\n  - name: {ROUTE_ENTRY_NAME}\n"
        );
        let projection = HermesProjection {
            entry: None,
            model: ModelTarget::Untouched,
        };
        let out = apply_on(&raw, &projection);
        let yaml: serde_yaml::Value = serde_yaml::from_str(&out).expect("parses");
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("provider"))
                .and_then(|v| v.as_str()),
            Some("other"),
            "没有记录的 home 只删条目，model 段不动"
        );
        assert!(yaml
            .get("custom_providers")
            .and_then(|v| v.as_sequence())
            .expect("custom_providers")
            .iter()
            .all(|p| p.get("name").and_then(|n| n.as_str()) != Some(ROUTE_ENTRY_NAME)));
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
