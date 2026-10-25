use crate::claude_desktop_config::{
    ResolvedModelRoute, DEFAULT_PROXY_ROUTES, ONE_M_CONTEXT_MARKER,
};
use crate::error::AppError;
use crate::provider::Provider;
use crate::provider_extensions::{ProviderCapabilities, ProviderExtension};
use serde_json::Value;

pub(crate) struct HuaweiExtension;
pub(crate) const HUAWEI_EXTENSION: HuaweiExtension = HuaweiExtension;

impl ProviderExtension for HuaweiExtension {
    fn matches(&self, provider_type: &str) -> bool {
        matches!(
            provider_type,
            "huawei_codeagent_2" | "huawei_codeagent_3" | "huawei_w3" | "huawei_idea"
        )
    }

    fn capabilities(&self, provider_type: &str) -> ProviderCapabilities {
        ProviderCapabilities {
            managed_auth: true,
            force_desktop_proxy: true,
            streaming_only_chat: provider_type == "huawei_codeagent_3",
        }
    }

    fn proxy_provider_type(
        &self,
        provider: &Provider,
    ) -> Option<crate::proxy::providers::ProviderType> {
        use crate::proxy::providers::ProviderType;
        Some(
            if provider.meta.as_ref()?.provider_type.as_deref()? == "huawei_codeagent_3" {
                ProviderType::HuaweiCodeAgent3
            } else {
                ProviderType::HuaweiCodeAgent2
            },
        )
    }

    fn is_managed_upstream(&self, host: &str, _path: &str) -> bool {
        host.ends_with(".rnd.huawei.com")
    }

    fn desktop_default_routes(
        &self,
        provider: &Provider,
    ) -> Option<Result<Vec<ResolvedModelRoute>, AppError>> {
        Some(default_routes_from_env(provider))
    }
}

/// 从 provider 的 env.ANTHROPIC_DEFAULT_*_MODEL 生成 Claude-safe 路由。
/// 复用 DEFAULT_PROXY_ROUTES 的 (route_id, env_key, supports_1m) 模板，
/// 用于华为 preset（不填 claudeDesktopModelRoutes）。剥掉 [1M] 后缀：
/// Claude Desktop schema 不接受，supports_1m 字段承载 1M 能力。
fn default_routes_from_env(provider: &Provider) -> Result<Vec<ResolvedModelRoute>, AppError> {
    let env = provider
        .settings_config
        .get("env")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            AppError::localized(
                "claude_desktop.provider.env_missing",
                "Claude Desktop 本地路由供应商缺少 env 配置",
                "Claude Desktop proxy provider is missing env configuration",
            )
        })?;
    let mut result = Vec::new();
    for default in DEFAULT_PROXY_ROUTES {
        let upstream_model = env
            .get(default.env_key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let Some(upstream_model) = upstream_model else {
            continue;
        };
        // 去掉 [1M] / [1m] 后缀，Claude Desktop schema 不接受。
        let marker = ONE_M_CONTEXT_MARKER.as_bytes();
        let bytes = upstream_model.as_bytes();
        let upstream_model_clean = if bytes.len() >= marker.len()
            && bytes[bytes.len() - marker.len()..].eq_ignore_ascii_case(marker)
        {
            upstream_model[..upstream_model.len() - marker.len()]
                .trim_end()
                .to_string()
        } else {
            upstream_model
        };
        result.push(ResolvedModelRoute {
            route_id: default.route_id.to_string(),
            upstream_model: upstream_model_clean,
            label_override: None,
            supports_1m: default.supports_1m,
        });
    }
    if result.is_empty() {
        return Err(AppError::localized(
            "claude_desktop.provider.huawei_routes_empty",
            "华为 Claude 供应商缺少 ANTHROPIC_DEFAULT_*_MODEL 模型配置",
            "Huawei Claude provider is missing ANTHROPIC_DEFAULT_*_MODEL env configuration",
        ));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use crate::provider_extensions::capabilities_for_type;

    #[test]
    fn legacy_aliases_keep_managed_auth_but_only_v3_requires_streaming() {
        for kind in [
            "huawei_codeagent_2",
            "huawei_codeagent_3",
            "huawei_w3",
            "huawei_idea",
        ] {
            let caps = capabilities_for_type(Some(kind));
            assert!(caps.managed_auth);
            assert!(caps.force_desktop_proxy);
            assert_eq!(caps.streaming_only_chat, kind == "huawei_codeagent_3");
        }
    }
}

#[cfg(test)]
mod proxy_tests {
    use super::*;
    use crate::proxy::providers::AuthStrategy;
    use crate::proxy::providers::ClaudeAdapter;
    use crate::proxy::providers::{get_adapter_for_provider_type, ProviderAdapter, ProviderType};

    #[test]
    fn aliases_keep_managed_auth_without_static_credentials() {
        for kind in [
            "huawei_codeagent_2",
            "huawei_codeagent_3",
            "huawei_w3",
            "huawei_idea",
        ] {
            let mut provider =
                Provider::with_id("test".into(), "test".into(), serde_json::json!({}), None);
            let mut meta = crate::provider::ProviderMeta::default();
            meta.provider_type = Some(kind.into());
            provider.meta = Some(meta);
            let adapter = ClaudeAdapter::new();
            let auth = adapter
                .extract_auth(&provider)
                .expect("managed auth needs no static key");
            assert_eq!(auth.strategy, AuthStrategy::ManagedExtension);
            assert!(adapter.get_auth_headers(&auth).unwrap().is_empty());
            let detected = crate::provider_extensions::proxy_provider_type(&provider).unwrap();
            assert_eq!(
                ProviderType::from_app_type_and_config(
                    &crate::app_config::AppType::Claude,
                    &provider
                ),
                Some(detected)
            );
            assert_eq!(
                detected,
                if kind.ends_with("_3") {
                    ProviderType::HuaweiCodeAgent3
                } else {
                    ProviderType::HuaweiCodeAgent2
                }
            );
            assert_eq!(get_adapter_for_provider_type(&detected).name(), "Claude");
        }
    }

    #[test]
    fn managed_host_matching_preserves_domain_boundaries() {
        for host in ["codeagentcli.rnd.huawei.com", "other.rnd.huawei.com"] {
            assert!(crate::provider_extensions::is_managed_upstream(host, "/"));
        }
        for host in [
            "evilrnd.huawei.com",
            "rnd.huawei.com.evil.test",
            "example.com",
        ] {
            assert!(!crate::provider_extensions::is_managed_upstream(host, "/"));
        }
    }
}
