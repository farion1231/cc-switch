//! Provider Adapter Trait
//!
//! 定义供应商适配器的统一接口，抽象不同上游供应商的处理逻辑。

use super::auth::AuthInfo;
use crate::provider::Provider;
use crate::proxy::error::ProxyError;
use serde_json::Value;

/// 供应商适配器 Trait
///
/// 所有供应商适配器都需要实现此 trait，提供统一的接口来处理：
/// - URL 构建
/// - 认证信息提取和头部注入
/// - 请求/响应格式转换（可选）
pub trait ProviderAdapter: Send + Sync {
    /// 适配器名称（用于日志和调试）
    fn name(&self) -> &'static str;

    /// 从 Provider 配置中提取 base_url
    fn extract_base_url(&self, provider: &Provider) -> Result<String, ProxyError>;

    /// 从 Provider 配置中提取认证信息
    fn extract_auth(&self, provider: &Provider) -> Option<AuthInfo>;

    /// 用指定的 api_key 字符串提取认证信息——KeyRing 轮换时调用。
    ///
    /// 默认实现：克隆 provider，把 key 写进适配器认定的"标准位置"，然后
    /// 调 `extract_auth`。各适配器应在精度重要的位置 override。
    ///
    /// OAuth 占位（GitHub Copilot / Codex OAuth）必须返回 None——它们的
    /// token 是动态获取的，不走静态 key 池。
    #[allow(dead_code)]
    fn extract_auth_with_key(
        &self,
        provider: &Provider,
        api_key: &str,
    ) -> Option<AuthInfo> {
        inject_key_into_provider(provider, api_key, self.name())
            .and_then(|p| self.extract_auth(&p))
    }

    /// 构建请求 URL
    fn build_url(&self, base_url: &str, endpoint: &str) -> String;

    /// Return auth headers as `(name, value)` pairs.
    ///
    /// The forwarder inserts these at the position of the original auth header
    /// so that header order is preserved.
    ///
    /// Returns `ProxyError::AuthError` when the credential contains characters
    /// that cannot be encoded as an HTTP header value (e.g. control chars,
    /// CR/LF), which would otherwise panic inside `HeaderValue::from_str`.
    fn get_auth_headers(
        &self,
        auth: &AuthInfo,
    ) -> Result<Vec<(http::HeaderName, http::HeaderValue)>, ProxyError>;

    /// 是否需要格式转换
    fn needs_transform(&self, _provider: &Provider) -> bool {
        false
    }

    /// 转换请求体
    fn transform_request(&self, body: Value, _provider: &Provider) -> Result<Value, ProxyError> {
        Ok(body)
    }

    /// 转换响应体
    #[allow(dead_code)]
    fn transform_response(&self, body: Value) -> Result<Value, ProxyError> {
        Ok(body)
    }
}

/// Build an HTTP `HeaderValue` from a credential / token string.
///
/// Returns `ProxyError::AuthError` when the string contains characters that
/// cannot live in an HTTP header value (control bytes, CR/LF, non-ASCII).
/// Adapters call this for every header value derived from user-pasted
/// material so a malformed key surfaces as a 401 instead of panicking
/// the worker via `HeaderValue::from_str(...).unwrap()`.
pub fn auth_header_value(s: &str) -> Result<http::HeaderValue, ProxyError> {
    http::HeaderValue::from_str(s)
        .map_err(|e| ProxyError::AuthError(format!("invalid auth header value: {e}")))
}

/// 克隆 `provider` 并把 `api_key` 写入适配器认定的"标准位置"。
///
/// KeyRing 轮换时调 `extract_auth_with_key`，默认实现走这里。设计原则：
/// - 优先**保留用户原本选用的字段**（例如 Claude 既支持 `ANTHROPIC_AUTH_TOKEN`
///   也支持 `ANTHROPIC_API_KEY`，改其中任意一个都意味着鉴权策略可能反转）。
/// - 任何路径都打不开时（`settings_config` 不是对象），返回 `None` —— 此时
///   `extract_auth` 也一定读不到 key，调用方按"没有 key"处理即可。
/// - OAuth-managed provider（`adapter_name` 为 `"Claude"` 时仍可能命中
///   Copilot/CodexOAuth 占位）会在 `extract_auth_with_key` 的 `extract_auth`
///   调用处返回 `None`，与未配置 key 的表现一致。
pub fn inject_key_into_provider(
    provider: &Provider,
    api_key: &str,
    adapter_name: &str,
) -> Option<Provider> {
    // 把 adapter_name 映射到 AppType——同一份写入路径走 Provider::set_api_key
    // （单真理来源）。OAuth-managed adapter（"Claude" 的 Copilot/CodexOAuth
    // 占位）仍由调用方在 extract_auth_with_key 里短路掉。
    let app_type = match adapter_name {
        "Claude" => crate::app_config::AppType::Claude,
        "Codex" => crate::app_config::AppType::Codex,
        "Gemini" => crate::app_config::AppType::Gemini,
        // Hermes / OpenClaw / OpenCode 已被 get_adapter 映射到 CodexAdapter
        // （见 proxy::providers::get_adapter），不会进入本函数。
        // 兜底：返回 None 让调用方走占位路径。
        _ => return None,
    };

    // settings_config 必须能写成 object 形态。Provider::set_api_key 内部
    // 直接 .expect() 抛错——这里提前 return None 把"畸形 settings_config"
    // 转换成 `None`（调用方走原始 extract_auth 路径，不注入）。
    if !provider.settings_config.is_object() {
        return None;
    }

    let mut new_provider = provider.clone();
    new_provider.set_api_key(api_key, &app_type);
    Some(new_provider)
}

/// 在 `parent[<parent_key>]`（必须是 object）下插入 `child_key = api_key`。
/// `parent_key` 不存在则创建空 object。
///
/// 已无调用方——`inject_key_into_provider` 现在直接走 `Provider::set_api_key`。
/// 保留以备未来需要给非 `Provider` 类型的 settings_config 注入 key 时复用。
#[allow(dead_code)]
fn upsert_in_subobject(
    parent: &mut serde_json::Map<String, Value>,
    parent_key: &str,
    child_key: &str,
    api_key: &str,
) {
    let entry = parent
        .entry(parent_key.to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if let Some(obj) = entry.as_object_mut() {
        obj.insert(child_key.to_string(), Value::String(api_key.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::providers::AuthStrategy;
    use serde_json::json;

    fn provider_with(settings: Value) -> Provider {
        Provider {
            id: "test".to_string(),
            name: "test".to_string(),
            settings_config: settings,
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn inject_claude_writes_to_anthropic_auth_token_by_default() {
        let p = provider_with(json!({}));
        let p2 = inject_key_into_provider(&p, "sk-rotated", "Claude").unwrap();
        assert_eq!(
            p2.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
            "sk-rotated"
        );
    }

    #[test]
    fn inject_claude_preserves_anthropic_api_key_choice() {
        // 用户原本用的是 ANTHROPIC_API_KEY（Claude 官方 x-api-key 鉴权），
        // 轮换时不能误写到 ANTHROPIC_AUTH_TOKEN，否则鉴权策略会反转。
        let p = provider_with(json!({"env": {"ANTHROPIC_API_KEY": "sk-original"}}));
        let p2 = inject_key_into_provider(&p, "sk-rotated", "Claude").unwrap();
        assert_eq!(p2.settings_config["env"]["ANTHROPIC_API_KEY"], "sk-rotated");
        assert!(p2.settings_config["env"]
            .get("ANTHROPIC_AUTH_TOKEN")
            .is_none());
    }

    #[test]
    fn inject_codex_writes_to_canonical_path() {
        // Codex 走单路径 auth.OPENAI_API_KEY——与 AppType::api_key_settings_path
        // 一致（Codex 列表里只有 1 项）。与读端 first_non_empty 顺序对称：
        // 读端会先看 env.OPENAI_API_KEY 再看 auth.OPENAI_API_KEY（auth 优先），
        // 所以写入用 auth 是正确的（保留用户原选）。
        let p = provider_with(json!({}));
        let p2 = inject_key_into_provider(&p, "sk-rotated", "Codex").unwrap();
        assert_eq!(p2.settings_config["auth"]["OPENAI_API_KEY"], "sk-rotated");
        // 单路径：不在 env / apiKey / options 写副本
        assert!(p2.settings_config.get("env").is_none());
        assert!(p2.settings_config.get("apiKey").is_none());
        assert!(p2.settings_config.get("options").is_none());
    }

    #[test]
    fn inject_gemini_writes_to_env() {
        let p = provider_with(json!({}));
        let p2 = inject_key_into_provider(&p, "AIza-rotated", "Gemini").unwrap();
        assert_eq!(p2.settings_config["env"]["GEMINI_API_KEY"], "AIza-rotated");
    }

    #[test]
    fn inject_unknown_adapter_returns_none() {
        let p = provider_with(json!({}));
        assert!(inject_key_into_provider(&p, "x", "Unknown").is_none());
    }

    #[test]
    fn inject_settings_not_object_returns_none() {
        let p = provider_with(Value::String("not-an-object".to_string()));
        assert!(inject_key_into_provider(&p, "x", "Claude").is_none());
    }

    #[test]
    fn extract_auth_with_key_default_impl_uses_injection() {
        // 默认实现 = inject_key + extract_auth。验证 Claude fixture
        // 走完整路径能拿到非占位 key。
        struct StubAdapter;
        impl ProviderAdapter for StubAdapter {
            fn name(&self) -> &'static str {
                "Claude"
            }
            fn extract_base_url(&self, _: &Provider) -> Result<String, ProxyError> {
                Ok("https://api.anthropic.com".to_string())
            }
            fn extract_auth(&self, p: &Provider) -> Option<AuthInfo> {
                let key = p.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"]
                    .as_str()?
                    .to_string();
                Some(AuthInfo::new(key, AuthStrategy::Bearer))
            }
            fn build_url(&self, base: &str, ep: &str) -> String {
                format!("{base}{ep}")
            }
            fn get_auth_headers(
                &self,
                _: &AuthInfo,
            ) -> Result<Vec<(http::HeaderName, http::HeaderValue)>, ProxyError> {
                Ok(vec![])
            }
        }
        let adapter = StubAdapter;
        let p = provider_with(json!({}));
        let auth = adapter.extract_auth_with_key(&p, "sk-rotated").unwrap();
        assert_eq!(auth.api_key, "sk-rotated");
        assert_eq!(auth.strategy, AuthStrategy::Bearer);
    }
}
