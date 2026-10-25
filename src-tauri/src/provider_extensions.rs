//! Composition boundary for provider-specific policy. Shared flows depend only on
//! these capabilities, never on vendor identifiers or Tauri initialization order.
use crate::claude_desktop_config::ResolvedModelRoute;
use crate::error::AppError;
use crate::provider::Provider;

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ProviderCapabilities {
    pub managed_auth: bool,
    pub force_desktop_proxy: bool,
    pub streaming_only_chat: bool,
}

pub(crate) trait ProviderExtension: Sync {
    fn matches(&self, provider_type: &str) -> bool;
    fn capabilities(&self, provider_type: &str) -> ProviderCapabilities;
    fn proxy_provider_type(
        &self,
        _provider: &Provider,
    ) -> Option<crate::proxy::providers::ProviderType> {
        None
    }
    fn is_managed_upstream(&self, _host: &str, _path: &str) -> bool {
        false
    }
    fn desktop_default_routes(
        &self,
        _provider: &Provider,
    ) -> Option<Result<Vec<ResolvedModelRoute>, AppError>> {
        None
    }
}

// Add provider modules here; business flows do not need new vendor branches.
const EXTENSIONS: &[&dyn ProviderExtension] = &[&crate::huawei::extension::HUAWEI_EXTENSION];

pub(crate) fn for_provider(provider: &Provider) -> Option<&'static dyn ProviderExtension> {
    let provider_type = provider.meta.as_ref()?.provider_type.as_deref()?;
    EXTENSIONS
        .iter()
        .copied()
        .find(|extension| extension.matches(provider_type))
}

pub(crate) fn proxy_provider_type(
    provider: &Provider,
) -> Option<crate::proxy::providers::ProviderType> {
    for_provider(provider)?.proxy_provider_type(provider)
}

pub(crate) fn is_managed_upstream(host: &str, path: &str) -> bool {
    EXTENSIONS
        .iter()
        .any(|extension| extension.is_managed_upstream(host, path))
}

pub(crate) fn capabilities(provider: &Provider) -> ProviderCapabilities {
    capabilities_for_type(
        provider
            .meta
            .as_ref()
            .and_then(|meta| meta.provider_type.as_deref()),
    )
}

pub(crate) fn capabilities_for_type(provider_type: Option<&str>) -> ProviderCapabilities {
    provider_type
        .and_then(|provider_type| {
            EXTENSIONS
                .iter()
                .find(|extension| extension.matches(provider_type))
                .map(|extension| extension.capabilities(provider_type))
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unregistered_providers_keep_default_behavior_without_app_setup() {
        for provider_type in [
            None,
            Some("anthropic"),
            Some("codex_oauth"),
            Some("unknown"),
        ] {
            let caps = capabilities_for_type(provider_type);
            assert!(!caps.managed_auth);
            assert!(!caps.force_desktop_proxy);
            assert!(!caps.streaming_only_chat);
        }
    }
}
