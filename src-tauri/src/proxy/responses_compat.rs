//! OpenAI Responses compatibility negotiated from structured upstream errors.
//!
//! Some Responses-compatible gateways reject optional sampling parameters such as
//! `temperature` and `top_p`.  When the upstream names one of those fields in a
//! structured 400/422 response, retry the same route without that field and
//! remember the capability so later requests do not repeat the known failure.

use super::ProxyError;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

/// Optional parameters that can be omitted without changing the required request
/// contract. Keep this list intentionally narrow and protocol-based.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ResponsesOptionalParam {
    Temperature,
    TopP,
}

impl ResponsesOptionalParam {
    const ALL: [Self; 2] = [Self::Temperature, Self::TopP];

    fn from_error_param(param: &str) -> Option<Self> {
        match param {
            "temperature" => Some(Self::Temperature),
            "top_p" => Some(Self::TopP),
            _ => None,
        }
    }

    pub(crate) fn field_name(self) -> &'static str {
        match self {
            Self::Temperature => "temperature",
            Self::TopP => "top_p",
        }
    }

    fn remove_from(self, body: &mut Value) -> bool {
        body.as_object_mut()
            .and_then(|object| object.remove(self.field_name()))
            .is_some()
    }
}

/// Capability identity for a Responses route. A rejection is only reusable for
/// the same provider, endpoint, model, and wire format.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct ResponsesRouteKey {
    provider_id: String,
    endpoint: String,
    model: String,
    api_format: String,
}

impl ResponsesRouteKey {
    pub(crate) fn new(
        provider_id: impl Into<String>,
        endpoint: impl Into<String>,
        model: impl Into<String>,
        api_format: impl Into<String>,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            endpoint: endpoint.into(),
            model: model.into(),
            api_format: api_format.into(),
        }
    }
}

/// Process-local cache of optional parameters rejected by each Responses route.
#[derive(Default)]
pub(crate) struct ResponsesCompatibilityCache {
    omitted: Mutex<HashMap<ResponsesRouteKey, HashSet<ResponsesOptionalParam>>>,
}

impl ResponsesCompatibilityCache {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn omitted_for(&self, key: &ResponsesRouteKey) -> HashSet<ResponsesOptionalParam> {
        self.omitted
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(key)
            .cloned()
            .unwrap_or_default()
    }

    fn apply_omissions(&self, key: &ResponsesRouteKey, body: &mut Value) {
        for param in self.omitted_for(key) {
            param.remove_from(body);
        }
    }

    pub(crate) fn record(&self, key: &ResponsesRouteKey, param: ResponsesOptionalParam) {
        self.omitted
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(key.clone())
            .or_default()
            .insert(param);
    }

    #[cfg(test)]
    fn contains(&self, key: &ResponsesRouteKey, param: ResponsesOptionalParam) -> bool {
        self.omitted_for(key).contains(&param)
    }
}

fn process_cache() -> &'static ResponsesCompatibilityCache {
    static CACHE: OnceLock<ResponsesCompatibilityCache> = OnceLock::new();
    CACHE.get_or_init(ResponsesCompatibilityCache::new)
}

/// Returns the optional parameter named by a structured 400/422 error.
pub(crate) fn optional_param_from_error(error: &ProxyError) -> Option<ResponsesOptionalParam> {
    let ProxyError::UpstreamError { status, body } = error else {
        return None;
    };
    if !matches!(*status, 400 | 422) {
        return None;
    }

    let parsed: Value = serde_json::from_str(body.as_deref()?).ok()?;
    ResponsesOptionalParam::from_error_param(parsed.pointer("/error/param")?.as_str()?)
}

/// Sends an Anthropic→Responses request and performs only the bounded,
/// same-route fallbacks justified by structured upstream errors.
pub(crate) async fn with_optional_param_fallback<T, F, Fut>(
    body: Value,
    cache: &ResponsesCompatibilityCache,
    key: &ResponsesRouteKey,
    mut send: F,
) -> Result<T, ProxyError>
where
    F: FnMut(Value) -> Fut,
    Fut: std::future::Future<Output = Result<T, ProxyError>>,
{
    let mut request_body = body;
    cache.apply_omissions(key, &mut request_body);
    let mut retries = 0usize;

    loop {
        match send(request_body.clone()).await {
            Ok(response) => return Ok(response),
            Err(error) => {
                let Some(param) = optional_param_from_error(&error) else {
                    return Err(error);
                };
                if retries >= ResponsesOptionalParam::ALL.len()
                    || !param.remove_from(&mut request_body)
                {
                    return Err(error);
                }

                retries += 1;
                cache.record(key, param);
                log::debug!(
                    "[ResponsesCompat] Retrying same route without rejected optional parameter `{}`",
                    param.field_name()
                );
            }
        }
    }
}

pub(crate) fn process_compatibility_cache() -> &'static ResponsesCompatibilityCache {
    process_cache()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex as StdMutex};

    fn upstream_error(status: u16, param: &str) -> ProxyError {
        ProxyError::UpstreamError {
            status,
            body: Some(
                json!({
                    "error": {
                        "type": "invalid_request_error",
                        "param": param,
                        "message": format!("Unsupported parameter: {param}")
                    }
                })
                .to_string(),
            ),
        }
    }

    fn route_key() -> ResponsesRouteKey {
        ResponsesRouteKey::new("provider-1", "/v1/messages", "model-a", "openai_responses")
    }

    fn sent_bodies() -> Arc<StdMutex<Vec<Value>>> {
        Arc::new(StdMutex::new(Vec::new()))
    }

    #[test]
    fn cache_is_scoped_to_the_full_route_key() {
        let cache = ResponsesCompatibilityCache::new();
        let key = route_key();
        cache.record(&key, ResponsesOptionalParam::Temperature);

        assert!(cache.contains(&key, ResponsesOptionalParam::Temperature));
        for other in [
            ResponsesRouteKey::new("provider-2", "/v1/messages", "model-a", "openai_responses"),
            ResponsesRouteKey::new("provider-1", "/other", "model-a", "openai_responses"),
            ResponsesRouteKey::new("provider-1", "/v1/messages", "model-b", "openai_responses"),
            ResponsesRouteKey::new("provider-1", "/v1/messages", "model-a", "openai_chat"),
        ] {
            assert!(!cache.contains(&other, ResponsesOptionalParam::Temperature));
        }
    }

    #[test]
    fn recognizes_only_structured_whitelisted_400_and_422_params() {
        for status in [400, 422] {
            assert_eq!(
                optional_param_from_error(&upstream_error(status, "temperature")),
                Some(ResponsesOptionalParam::Temperature)
            );
            assert_eq!(
                optional_param_from_error(&upstream_error(status, "top_p")),
                Some(ResponsesOptionalParam::TopP)
            );
        }

        assert!(optional_param_from_error(&upstream_error(400, "messages")).is_none());
        assert!(optional_param_from_error(&upstream_error(500, "temperature")).is_none());
        assert!(optional_param_from_error(&ProxyError::UpstreamError {
            status: 400,
            body: Some(
                json!({"error": {"message": "Unsupported parameter: temperature"}}).to_string(),
            ),
        })
        .is_none());
        assert!(optional_param_from_error(&ProxyError::UpstreamError {
            status: 400,
            body: Some("Unsupported parameter: temperature".to_string()),
        })
        .is_none());
    }

    #[tokio::test]
    async fn retries_temperature_once_on_the_same_route() {
        let cache = ResponsesCompatibilityCache::new();
        let key = route_key();
        let bodies = sent_bodies();
        let captured = bodies.clone();

        let result = with_optional_param_fallback(
            json!({
                "model": "model-a",
                "temperature": 0.2,
                "top_p": 0.9,
                "messages": []
            }),
            &cache,
            &key,
            move |body| {
                let captured = captured.clone();
                async move {
                    let attempt = {
                        let mut captured = captured.lock().unwrap();
                        captured.push(body.clone());
                        captured.len()
                    };
                    if attempt == 1 {
                        Err(upstream_error(400, "temperature"))
                    } else {
                        Ok(())
                    }
                }
            },
        )
        .await;

        assert!(result.is_ok());
        let bodies = bodies.lock().unwrap();
        assert_eq!(bodies.len(), 2);
        assert_eq!(bodies[0]["temperature"], 0.2);
        assert_eq!(bodies[1].get("temperature"), None);
        assert_eq!(bodies[1]["top_p"], 0.9);
        assert!(cache.contains(&key, ResponsesOptionalParam::Temperature));
    }

    #[tokio::test]
    async fn retries_multiple_whitelisted_params_with_a_strict_bound() {
        let cache = ResponsesCompatibilityCache::new();
        let key = route_key();
        let bodies = sent_bodies();
        let captured = bodies.clone();

        let result = with_optional_param_fallback(
            json!({"model": "model-a", "temperature": 0.2, "top_p": 0.9}),
            &cache,
            &key,
            move |body| {
                let captured = captured.clone();
                async move {
                    captured.lock().unwrap().push(body.clone());
                    if body.get("temperature").is_some() {
                        Err(upstream_error(400, "temperature"))
                    } else if body.get("top_p").is_some() {
                        Err(upstream_error(422, "top_p"))
                    } else {
                        Ok(())
                    }
                }
            },
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(bodies.lock().unwrap().len(), 3);
        assert!(cache.contains(&key, ResponsesOptionalParam::Temperature));
        assert!(cache.contains(&key, ResponsesOptionalParam::TopP));
    }

    #[tokio::test]
    async fn repeated_same_error_does_not_loop() {
        let cache = ResponsesCompatibilityCache::new();
        let key = route_key();
        let attempts = Arc::new(StdMutex::new(0usize));
        let captured = attempts.clone();

        let error = with_optional_param_fallback(
            json!({"model": "model-a", "temperature": 0.2}),
            &cache,
            &key,
            move |_body| {
                let captured = captured.clone();
                async move {
                    *captured.lock().unwrap() += 1;
                    Err::<(), _>(upstream_error(400, "temperature"))
                }
            },
        )
        .await
        .expect_err("the repeated error must be returned after the bounded retry");

        assert!(matches!(
            error,
            ProxyError::UpstreamError { status: 400, body: Some(body) }
                if body.contains("temperature")
        ));
        assert_eq!(*attempts.lock().unwrap(), 2);
    }

    #[tokio::test]
    async fn cache_prevents_later_requests_from_repeating_known_failures() {
        let cache = ResponsesCompatibilityCache::new();
        let key = route_key();
        let first_bodies = sent_bodies();
        let first_captured = first_bodies.clone();

        with_optional_param_fallback(
            json!({"model": "model-a", "temperature": 0.2, "top_p": 0.9}),
            &cache,
            &key,
            move |body| {
                let captured = first_captured.clone();
                async move {
                    captured.lock().unwrap().push(body.clone());
                    if body.get("temperature").is_some() {
                        Err(upstream_error(400, "temperature"))
                    } else if body.get("top_p").is_some() {
                        Err(upstream_error(422, "top_p"))
                    } else {
                        Ok(())
                    }
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(first_bodies.lock().unwrap().len(), 3);

        let second_bodies = sent_bodies();
        let second_captured = second_bodies.clone();
        with_optional_param_fallback(
            json!({"model": "model-a", "temperature": 0.2, "top_p": 0.9}),
            &cache,
            &key,
            move |body| {
                let captured = second_captured.clone();
                async move {
                    captured.lock().unwrap().push(body);
                    Ok(())
                }
            },
        )
        .await
        .unwrap();

        let second_bodies = second_bodies.lock().unwrap();
        assert_eq!(second_bodies.len(), 1, "cached omissions avoid another 400");
        assert_eq!(second_bodies[0].get("temperature"), None);
        assert_eq!(second_bodies[0].get("top_p"), None);
    }

    #[tokio::test]
    async fn non_whitelisted_and_server_errors_are_returned_unchanged() {
        for error in [
            upstream_error(400, "max_output_tokens"),
            upstream_error(500, "temperature"),
        ] {
            let expected_status = match &error {
                ProxyError::UpstreamError { status, .. } => *status,
                _ => unreachable!(),
            };
            let expected_body = match &error {
                ProxyError::UpstreamError { body, .. } => body.clone(),
                _ => unreachable!(),
            };
            let retry_status = expected_status;
            let retry_body = expected_body.clone();
            let attempts = Arc::new(StdMutex::new(0usize));
            let captured = attempts.clone();

            let returned = with_optional_param_fallback(
                json!({"model": "model-a", "temperature": 0.2}),
                &ResponsesCompatibilityCache::new(),
                &route_key(),
                move |_body| {
                    let captured = captured.clone();
                    let retry_body = retry_body.clone();
                    async move {
                        *captured.lock().unwrap() += 1;
                        Err::<(), _>(ProxyError::UpstreamError {
                            status: retry_status,
                            body: retry_body,
                        })
                    }
                },
            )
            .await
            .expect_err("non-whitelisted errors must be preserved");

            assert!(matches!(
                returned,
                ProxyError::UpstreamError { status, body }
                    if status == expected_status && body == expected_body
            ));
            assert_eq!(*attempts.lock().unwrap(), 1);
        }
    }
}
