use crate::huawei::claude_config;
use crate::huawei::proxy_processor;
use crate::provider::ProviderMeta;
use crate::provider_extensions::ProviderExtension;
use crate::proxy::hooks::ProxyHook;
use crate::proxy::ProxyError;
use axum::http::HeaderMap;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use tauri::AppHandle;

#[derive(Default)]
pub struct HuaweiCodeAgentProxyHook;

impl HuaweiCodeAgentProxyHook {
    pub fn new() -> Self {
        Self
    }
}

impl ProxyHook for HuaweiCodeAgentProxyHook {
    fn matches_provider(&self, meta: &ProviderMeta) -> bool {
        meta.provider_type
            .as_deref()
            .is_some_and(|kind| super::extension::HUAWEI_EXTENSION.matches(kind))
    }

    fn force_api_format(&self, meta: &ProviderMeta) -> Option<String> {
        if self.matches_provider(meta) {
            Some("openai_chat".to_string())
        } else {
            None
        }
    }

    fn on_request_body(&self, body: &mut Value, meta: &ProviderMeta) {
        proxy_processor::apply_fingerprint_replacements_to_body(body, &meta.extra);
        // SnapEngine's CodeAgent 3 routes can reject stream:false as "model not
        // registered". The response handler aggregates SSE for JSON clients.
        if crate::provider_extensions::capabilities_for_type(meta.provider_type.as_deref())
            .streaming_only_chat
        {
            body["stream"] = Value::Bool(true);
        }
    }

    fn on_auth_headers(
        &self,
        meta: &ProviderMeta,
        app_handle: Option<&AppHandle>,
        body: Option<&Value>,
    ) -> Pin<Box<dyn Future<Output = Vec<(String, String)>> + Send>> {
        let extra = meta.extra.clone();
        let app_handle = app_handle.cloned();
        let body = body.cloned();
        Box::pin(async move {
            proxy_processor::resolve_extra_auth_headers(&extra, app_handle.as_ref(), body.as_ref())
                .await
                .unwrap_or_default()
        })
    }

    fn filter_outbound_headers(&self, meta: &ProviderMeta, headers: &mut HeaderMap) {
        let header_mode = proxy_processor::get_header_mode(&meta.extra)
            .unwrap_or_else(|| "passthrough".to_string());
        if header_mode != "whitelist" {
            return;
        }
        let allowed = proxy_processor::get_allowed_headers(&meta.extra).unwrap_or_default();
        if allowed.is_empty() {
            return;
        }
        let mut to_remove = vec![];
        for name in headers.keys() {
            let name_str = name.as_str();
            if !allowed.iter().any(|a| name_str.eq_ignore_ascii_case(a)) {
                to_remove.push(name.clone());
            }
        }
        for name in to_remove {
            headers.remove(name);
        }
    }

    fn on_build_url(&self, meta: &ProviderMeta, url: &mut String) {
        if let Some(prefix) = proxy_processor::get_strip_path_prefix(&meta.extra) {
            let prefix_with_slash = format!("{}/", prefix);
            if url.contains(&prefix_with_slash) {
                *url = url.replace(&prefix_with_slash, "/");
            }
        }
    }

    fn prefer_pooled_client(&self, meta: &ProviderMeta) -> bool {
        proxy_processor::get_header_mode(&meta.extra).as_deref() == Some("whitelist")
    }

    fn skip_tls_verify(&self, meta: &ProviderMeta) -> bool {
        proxy_processor::get_skip_tls_verify(&meta.extra)
    }

    fn on_response_body(&self, meta: &ProviderMeta, body: &[u8]) -> Option<Vec<u8>> {
        proxy_processor::apply_fingerprint_replacements_to_response(body, &meta.extra)
    }

    fn transform_upstream_error(&self, meta: &ProviderMeta, body: &Value) -> Option<ProxyError> {
        let error_format = proxy_processor::get_error_format(&meta.extra)
            .unwrap_or_else(|| "passthrough".to_string());
        if error_format != "to_anthropic" {
            return None;
        }
        if body.get("choices").is_some() {
            return None;
        }
        if let Some(error_code) = body.get("error_code").and_then(|v| v.as_str()) {
            let error_msg = body.get("error_msg").and_then(|v| v.as_str()).unwrap_or("");
            let status = if error_code.contains("429") {
                429
            } else if error_code.starts_with("4")
                || error_code.contains("400")
                || error_code.contains("401")
                || error_code.contains("403")
            {
                400
            } else {
                502
            };
            log::warn!("[Transform] 上游返回非标准错误响应 (error_code={error_code}): {error_msg}");
            return Some(ProxyError::UpstreamError {
                status,
                body: Some(format!(
                    "{{\"error_code\":\"{error_code}\",\"error_msg\":\"{error_msg}\"}}"
                )),
            });
        }
        if let Some(error_obj) = body.get("error") {
            let error_str = error_obj.to_string();
            let status = if error_str.contains("429") || error_str.contains("rate_limit") {
                429
            } else {
                502
            };
            log::warn!("[Transform] 上游返回 error 对象: {error_str}");
            return Some(ProxyError::UpstreamError {
                status,
                body: Some(error_str),
            });
        }
        None
    }

    fn on_takeover(&self, meta: &ProviderMeta) -> Result<(), String> {
        if meta.provider_type.as_deref() != Some("huawei_codeagent_3") {
            return Ok(());
        }

        // Desktop 使用独立的认证流程，不修改 Claude Code 的批准列表。
        let is_desktop = meta
            .extra
            .get("appTarget")
            .and_then(|v| v.as_str())
            .is_some_and(|s| s == "claude_desktop");
        if is_desktop {
            return Ok(());
        }

        claude_config::approve_proxy_managed_key()
            .map(|_| ())
            .map_err(|e| format!("写入 PROXY_MANAGED 到 ~/.claude.json 失败: {e}"))
    }

    fn on_restore(&self, meta: &ProviderMeta) -> Result<(), String> {
        if meta.provider_type.as_deref() != Some("huawei_codeagent_3") {
            return Ok(());
        }

        // Desktop 从未修改 Claude Code 的批准列表。
        let is_desktop = meta
            .extra
            .get("appTarget")
            .and_then(|v| v.as_str())
            .is_some_and(|s| s == "claude_desktop");
        if is_desktop {
            return Ok(());
        }

        claude_config::remove_proxy_managed_key()
            .map_err(|e| format!("从 ~/.claude.json 移除 PROXY_MANAGED 失败: {e}"))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderMeta;
    use serde_json::json;
    use std::env;
    use tempfile::TempDir;

    #[test]
    fn codeagent_3_forces_upstream_streaming_only_for_its_own_provider() {
        let hook = HuaweiCodeAgentProxyHook::new();
        for stream in [None, Some(false), Some(true)] {
            let mut body = json!({"model": "maas-glm-5.2-tencent-codeagent", "messages": []});
            if let Some(stream) = stream {
                body["stream"] = json!(stream);
            }
            let original = body.clone();
            let mut meta = ProviderMeta::default();
            meta.provider_type = Some("huawei_codeagent_3".into());
            hook.on_request_body(&mut body, &meta);
            assert_eq!(body["stream"], true);
            assert_eq!(body["model"], original["model"]);
            for other in ["huawei_codeagent_2", "anthropic", "xai_oauth"] {
                meta.provider_type = Some(other.into());
                let mut body = original.clone();
                hook.on_request_body(&mut body, &meta);
                assert_eq!(body, original);
            }
        }
    }

    #[test]
    fn registry_applies_transport_and_header_policy_only_to_owned_aliases() {
        use crate::proxy::hooks::HookRegistry;
        let registry = HookRegistry::new();
        registry.register(Box::new(HuaweiCodeAgentProxyHook::new()));
        for kind in [
            "huawei_codeagent_2",
            "huawei_codeagent_3",
            "huawei_w3",
            "huawei_idea",
            "anthropic",
            "unknown",
        ] {
            let mut meta = ProviderMeta::default();
            meta.provider_type = Some(kind.into());
            meta.extra.insert("headerMode".into(), json!("whitelist"));
            meta.extra.insert("skipTlsVerify".into(), json!(true));
            let expected = kind.starts_with("huawei_");
            registry.with_provider_hooks(&meta, |hooks| {
                assert_eq!(hooks.len(), usize::from(expected));
                assert_eq!(
                    hooks.iter().any(|hook| hook.skip_tls_verify(&meta)),
                    expected
                );
                assert_eq!(
                    hooks.iter().any(|hook| hook.prefer_pooled_client(&meta)),
                    expected
                );
                assert_eq!(
                    hooks.iter().find_map(|hook| hook.force_api_format(&meta)),
                    expected.then(|| "openai_chat".into())
                );
            });
        }
    }

    #[tokio::test]
    async fn matched_hooks_filter_headers_and_resolve_auth_per_request() {
        use crate::proxy::hooks::HookRegistry;
        let registry = HookRegistry::new();
        registry.register(Box::new(HuaweiCodeAgentProxyHook::new()));
        for kind in ["huawei_codeagent_2", "anthropic"] {
            let mut meta = ProviderMeta::default();
            meta.provider_type = Some(kind.into());
            meta.extra.insert("headerMode".into(), json!("whitelist"));
            meta.extra
                .insert("allowedHeaders".into(), json!(["X-Keep"]));
            meta.extra.insert(
                "extraAuthHeaders".into(),
                json!([{"name":"x-auth-token", "value":"request-token"}]),
            );
            let mut headers = HeaderMap::new();
            headers.append("x-keep", "first".parse().unwrap());
            headers.append("x-keep", "second".parse().unwrap());
            headers.insert("x-drop", "private".parse().unwrap());
            let futures = registry.with_provider_hooks(&meta, |hooks| {
                for hook in hooks {
                    hook.filter_outbound_headers(&meta, &mut headers);
                }
                hooks
                    .iter()
                    .map(|hook| hook.on_auth_headers(&meta, None, None))
                    .collect::<Vec<_>>()
            });
            assert_eq!(headers.get_all("x-keep").iter().count(), 2);
            assert_eq!(headers.contains_key("x-drop"), kind == "anthropic");
            assert_eq!(futures.len(), usize::from(kind != "anthropic"));
            for future in futures {
                assert_eq!(
                    future.await,
                    vec![("x-auth-token".to_string(), "request-token".to_string())]
                );
            }
        }
    }

    /// Isolate ~/.claude.json writes in tests.
    struct TempHome {
        #[allow(dead_code)]
        dir: TempDir,
        original_test_home: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("failed to create temp home");
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            Self {
                dir,
                original_test_home,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match &self.original_test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn meta_with_app_target(provider_type: &str, app_target: &str) -> ProviderMeta {
        let mut meta = ProviderMeta::default();
        meta.provider_type = Some(provider_type.to_string());
        meta.extra
            .insert("appTarget".to_string(), json!(app_target));
        meta
    }

    #[test]
    fn desktop_does_not_modify_claude_code_approval() {
        let _home = TempHome::new();
        let hook = HuaweiCodeAgentProxyHook::new();
        let meta = meta_with_app_target("huawei_codeagent_3", "claude_desktop");

        hook.on_takeover(&meta).expect("desktop takeover");
        hook.on_restore(&meta).expect("desktop restore");

        assert!(!crate::config::get_claude_mcp_path().exists());
    }

    #[test]
    fn cli_keeps_proxy_managed_approval_without_connectivity_proxy() {
        let _home = TempHome::new();
        let hook = HuaweiCodeAgentProxyHook::new();
        let meta = meta_with_app_target("huawei_codeagent_3", "claude");

        hook.on_takeover(&meta).expect("CLI takeover");
        let path = crate::config::get_claude_mcp_path();
        let doc: Value = serde_json::from_slice(&std::fs::read(&path).expect("claude.json"))
            .expect("valid JSON");
        assert!(doc["customApiKeyResponses"]["approved"]
            .as_array()
            .expect("approved keys")
            .contains(&json!("PROXY_MANAGED")));

        hook.on_restore(&meta).expect("CLI restore");
        let doc: Value = serde_json::from_slice(&std::fs::read(&path).expect("claude.json"))
            .expect("valid JSON");
        assert!(!doc["customApiKeyResponses"]["approved"]
            .as_array()
            .expect("approved keys")
            .contains(&json!("PROXY_MANAGED")));
    }
}
