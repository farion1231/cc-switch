use crate::provider::ProviderMeta;
use crate::proxy::ProxyError;
use axum::http::HeaderMap;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::sync::{LazyLock, RwLock};
use tauri::AppHandle;

pub trait ProxyHook: Send + Sync {
    fn matches_provider(&self, _meta: &ProviderMeta) -> bool {
        false
    }
    fn force_api_format(&self, _meta: &ProviderMeta) -> Option<String> {
        None
    }
    fn on_request_body(&self, _body: &mut Value, _meta: &ProviderMeta) {}

    fn on_auth_headers(
        &self,
        _meta: &ProviderMeta,
        _app_handle: Option<&AppHandle>,
        _body: Option<&Value>,
    ) -> Pin<Box<dyn Future<Output = Vec<(String, String)>> + Send>> {
        Box::pin(async { vec![] })
    }

    fn filter_outbound_headers(&self, _meta: &ProviderMeta, _headers: &mut HeaderMap) {}

    fn on_build_url(&self, _meta: &ProviderMeta, _url: &mut String) {}

    /// Select pooled transport when exact inbound header casing is unnecessary.
    fn prefer_pooled_client(&self, _meta: &ProviderMeta) -> bool {
        false
    }

    fn skip_tls_verify(&self, _meta: &ProviderMeta) -> bool {
        false
    }

    fn on_response_body(&self, _meta: &ProviderMeta, _body: &[u8]) -> Option<Vec<u8>> {
        None
    }

    fn transform_upstream_error(&self, _meta: &ProviderMeta, _body: &Value) -> Option<ProxyError> {
        None
    }

    fn on_takeover(&self, _meta: &ProviderMeta) -> Result<(), String> {
        Ok(())
    }

    fn on_restore(&self, _meta: &ProviderMeta) -> Result<(), String> {
        Ok(())
    }
}

pub struct HookRegistry {
    proxy_hooks: RwLock<Vec<Box<dyn ProxyHook>>>,
}

impl HookRegistry {
    pub(crate) fn new() -> Self {
        Self {
            proxy_hooks: RwLock::new(vec![]),
        }
    }

    pub fn register(&self, hook: Box<dyn ProxyHook>) {
        self.proxy_hooks.write().unwrap().push(hook);
    }

    /// Dispatch only hooks owned by this provider. Futures returned by the closure
    /// must own their inputs so callers can await them after releasing this lock.
    pub fn with_provider_hooks<F, R>(&self, meta: &ProviderMeta, f: F) -> R
    where
        F: FnOnce(&[&dyn ProxyHook]) -> R,
    {
        let hooks = self.proxy_hooks.read().unwrap();
        let matching: Vec<&dyn ProxyHook> = hooks
            .iter()
            .filter(|hook| hook.matches_provider(meta))
            .map(|hook| hook.as_ref())
            .collect();
        f(&matching)
    }

    pub fn global() -> &'static HookRegistry {
        static REGISTRY: LazyLock<HookRegistry> = LazyLock::new(HookRegistry::new);
        &REGISTRY
    }

    #[cfg(test)]
    pub fn clear(&self) {
        self.proxy_hooks.write().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHook;
    impl ProxyHook for TestHook {
        fn matches_provider(&self, meta: &ProviderMeta) -> bool {
            meta.provider_type.as_deref() == Some("test-extension")
        }
        fn on_request_body(&self, body: &mut Value, _: &ProviderMeta) {
            body["matched"] = Value::Bool(true);
        }
        fn on_auth_headers(
            &self,
            _: &ProviderMeta,
            _: Option<&AppHandle>,
            _: Option<&Value>,
        ) -> Pin<Box<dyn Future<Output = Vec<(String, String)>> + Send>> {
            Box::pin(async { vec![("authorization".into(), "test-token".into())] })
        }
    }

    #[tokio::test]
    async fn dispatch_is_scoped_and_auth_futures_release_registry_lock() {
        let registry = HookRegistry::new();
        registry.register(Box::new(TestHook));
        for kind in [None, Some("ordinary"), Some("test-extension")] {
            let mut meta = ProviderMeta::default();
            meta.provider_type = kind.map(str::to_owned);
            let mut body = serde_json::json!({});
            let futures = registry.with_provider_hooks(&meta, |hooks| {
                for hook in hooks {
                    hook.on_request_body(&mut body, &meta);
                }
                hooks
                    .iter()
                    .map(|hook| hook.on_auth_headers(&meta, None, Some(&body)))
                    .collect::<Vec<_>>()
            });
            assert_eq!(
                body.get("matched").is_some(),
                kind == Some("test-extension")
            );
            assert_eq!(futures.len(), usize::from(kind == Some("test-extension")));
            // This write would deadlock if dispatch retained the read lock.
            registry.clear();
            for future in futures {
                assert_eq!(future.await[0].1, "test-token");
            }
            registry.register(Box::new(TestHook));
        }
    }
}
