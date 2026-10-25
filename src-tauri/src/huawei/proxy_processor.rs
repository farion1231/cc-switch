use serde_json::Value;
use std::collections::HashMap;
use tauri::Manager;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FingerprintReplacement {
    pub from: String,
    #[serde(rename = "to")]
    pub to_value: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExtraAuthHeader {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(rename = "tokenSource", skip_serializing_if = "Option::is_none")]
    pub token_source: Option<String>,
    #[serde(
        rename = "valuePrefix",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub value_prefix: Option<String>,
}

pub fn get_fingerprint_replacements(
    extra: &HashMap<String, Value>,
) -> Option<Vec<FingerprintReplacement>> {
    extra
        .get("fingerprintReplacements")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
}

pub fn get_extra_auth_headers(extra: &HashMap<String, Value>) -> Option<Vec<ExtraAuthHeader>> {
    extra
        .get("extraAuthHeaders")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
}

pub fn get_skip_tls_verify(extra: &HashMap<String, Value>) -> bool {
    extra
        .get("skipTlsVerify")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

pub fn get_strip_path_prefix(extra: &HashMap<String, Value>) -> Option<String> {
    extra
        .get("stripPathPrefix")
        .and_then(|v| v.as_str())
        .map(String::from)
}

pub fn get_header_mode(extra: &HashMap<String, Value>) -> Option<String> {
    extra
        .get("headerMode")
        .and_then(|v| v.as_str())
        .map(String::from)
}

pub fn get_allowed_headers(extra: &HashMap<String, Value>) -> Option<Vec<String>> {
    extra
        .get("allowedHeaders")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
}

pub fn get_error_format(extra: &HashMap<String, Value>) -> Option<String> {
    extra
        .get("errorFormat")
        .and_then(|v| v.as_str())
        .map(String::from)
}

pub fn apply_fingerprint_replacements_to_body(body: &mut Value, extra: &HashMap<String, Value>) {
    let replacements = get_fingerprint_replacements(extra)
        .map(|reps| {
            reps.iter()
                .map(|r| (r.from.clone(), r.to_value.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !replacements.is_empty() {
        super::fingerprint::apply_fingerprint_replacements(body, &replacements);
        super::fingerprint::strip_billing_header_in_value(body);
    }
}

pub fn apply_fingerprint_replacements_to_response(
    body_bytes: &[u8],
    extra: &HashMap<String, Value>,
) -> Option<Vec<u8>> {
    let replacements = get_fingerprint_replacements(extra)
        .map(|reps| {
            reps.iter()
                .map(|r| (r.from.clone(), r.to_value.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if replacements.is_empty() {
        return None;
    }
    let mut json_value = serde_json::from_slice::<Value>(body_bytes).ok()?;
    super::fingerprint::apply_fingerprint_replacements(&mut json_value, &replacements);
    Some(serde_json::to_vec(&json_value).unwrap_or_else(|_| body_bytes.to_vec()))
}

pub async fn resolve_extra_auth_headers(
    extra: &HashMap<String, Value>,
    app_handle: Option<&tauri::AppHandle>,
    body: Option<&Value>,
) -> Result<Vec<(String, String)>, String> {
    let Some(headers) = get_extra_auth_headers(extra) else {
        return Ok(vec![]);
    };

    let selected_account_id: Option<String> = extra
        .get("selectedAccountId")
        .and_then(|v| v.as_str())
        .map(String::from);

    let mut result = vec![];
    for eh in headers.iter() {
        let raw_value = if let Some(ref src) = eh.token_source {
            match src.as_str() {
                "huawei_w3" | "huawei_codeagent_2" => {
                    let Some(handle) = app_handle else {
                        continue;
                    };
                    let state = handle.state::<super::auth::commands::HuaweiCodeAgentAuthState>();

                    let account_id = if let Some(ref id) = selected_account_id {
                        id.clone()
                    } else {
                        let status = state.0.get_status().await;
                        status.account_id.ok_or("Huawei IDEA 未登录")?
                    };

                    state
                        .0
                        .get_token_with_refresh(&account_id)
                        .await
                        .map_err(|e| e.to_string())?
                }
                "uuid" => uuid::Uuid::new_v4().to_string(),
                "request_model" => body
                    .and_then(|b| b.get("model"))
                    .and_then(|v| v.as_str())
                    .filter(|s| s.is_ascii())
                    .map(String::from)
                    .unwrap_or_default(),
                _ => continue,
            }
        } else {
            eh.value.clone().unwrap_or_default()
        };

        let value = if let Some(ref prefix) = eh.value_prefix {
            format!("{}{}", prefix, raw_value)
        } else {
            raw_value
        };

        log::debug!(
            "[ProxyProcessor] 额外认证头: {} = {}",
            eh.name,
            if eh.name.eq_ignore_ascii_case("x-auth-token")
                || eh.name.eq_ignore_ascii_case("authorization")
            {
                "<token>".to_string()
            } else {
                value.clone()
            }
        );
        result.push((eh.name.clone(), value));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extra_auth_header_deserialize_with_value_prefix() {
        let json_str =
            r#"{"name":"Authorization","tokenSource":"huawei_w3","valuePrefix":"Bearer "}"#;
        let header: ExtraAuthHeader = serde_json::from_str(json_str).unwrap();
        assert_eq!(header.name, "Authorization");
        assert_eq!(header.token_source.as_deref(), Some("huawei_w3"));
        assert_eq!(header.value_prefix.as_deref(), Some("Bearer "));
        assert!(header.value.is_none());
    }

    #[test]
    fn test_extra_auth_header_deserialize_without_value_prefix() {
        let json_str = r#"{"name":"app-id","value":"com.huawei.devmind.codebot.apibot"}"#;
        let header: ExtraAuthHeader = serde_json::from_str(json_str).unwrap();
        assert_eq!(header.name, "app-id");
        assert_eq!(
            header.value.as_deref(),
            Some("com.huawei.devmind.codebot.apibot")
        );
        assert!(header.token_source.is_none());
        assert!(header.value_prefix.is_none());
    }

    #[test]
    fn test_extra_auth_header_serialize_value_prefix() {
        let header = ExtraAuthHeader {
            name: "Authorization".to_string(),
            value: None,
            token_source: Some("huawei_w3".to_string()),
            value_prefix: Some("Bearer ".to_string()),
        };
        let json = serde_json::to_value(&header).unwrap();
        assert_eq!(json["valuePrefix"], "Bearer ");
        assert_eq!(json["tokenSource"], "huawei_w3");
    }

    #[test]
    fn test_value_prefix_applied_to_static_value() {
        let mut extra = HashMap::new();
        extra.insert(
            "extraAuthHeaders".to_string(),
            json!([
                {"name": "Authorization", "value": "my-token", "valuePrefix": "Bearer "}
            ]),
        );

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(resolve_extra_auth_headers(&extra, None, None))
            .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "Authorization");
        assert_eq!(result[0].1, "Bearer my-token");
    }

    #[test]
    fn test_value_prefix_not_applied_when_absent() {
        let mut extra = HashMap::new();
        extra.insert(
            "extraAuthHeaders".to_string(),
            json!([
                {"name": "app-id", "value": "com.huawei.devmind.codebot.apibot"}
            ]),
        );

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(resolve_extra_auth_headers(&extra, None, None))
            .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "app-id");
        assert_eq!(result[0].1, "com.huawei.devmind.codebot.apibot");
    }

    #[test]
    fn test_request_model_extracts_model_from_body() {
        let mut extra = HashMap::new();
        extra.insert(
            "extraAuthHeaders".to_string(),
            json!([
                {"name": "Model-Id", "tokenSource": "request_model"}
            ]),
        );
        let body = json!({"model": "maas-glm-5.2-aliyun", "messages": []});

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(resolve_extra_auth_headers(&extra, None, Some(&body)))
            .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "Model-Id");
        assert_eq!(result[0].1, "maas-glm-5.2-aliyun");
    }

    #[test]
    fn test_request_model_returns_empty_when_body_is_none() {
        let mut extra = HashMap::new();
        extra.insert(
            "extraAuthHeaders".to_string(),
            json!([
                {"name": "Model-Id", "tokenSource": "request_model"}
            ]),
        );

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(resolve_extra_auth_headers(&extra, None, None))
            .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "Model-Id");
        assert_eq!(result[0].1, "");
    }

    #[test]
    fn test_request_model_returns_empty_for_non_ascii_model() {
        let mut extra = HashMap::new();
        extra.insert(
            "extraAuthHeaders".to_string(),
            json!([
                {"name": "Model-Id", "tokenSource": "request_model"}
            ]),
        );
        let body = json!({"model": "模型-中文"});

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(resolve_extra_auth_headers(&extra, None, Some(&body)))
            .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "Model-Id");
        assert_eq!(result[0].1, "");
    }

    #[test]
    fn test_request_model_returns_empty_when_model_field_missing() {
        let mut extra = HashMap::new();
        extra.insert(
            "extraAuthHeaders".to_string(),
            json!([
                {"name": "Model-Id", "tokenSource": "request_model"}
            ]),
        );
        let body = json!({"messages": []});

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(resolve_extra_auth_headers(&extra, None, Some(&body)))
            .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "Model-Id");
        assert_eq!(result[0].1, "");
    }

    #[test]
    fn test_value_prefix_with_request_model() {
        let mut extra = HashMap::new();
        extra.insert(
            "extraAuthHeaders".to_string(),
            json!([
                {"name": "X-Model", "tokenSource": "request_model", "valuePrefix": "model:"}
            ]),
        );
        let body = json!({"model": "glm-5.2"});

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(resolve_extra_auth_headers(&extra, None, Some(&body)))
            .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "X-Model");
        assert_eq!(result[0].1, "model:glm-5.2");
    }

    #[test]
    fn test_no_extra_auth_headers_returns_empty() {
        let extra = HashMap::new();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(resolve_extra_auth_headers(&extra, None, None))
            .unwrap();

        assert!(result.is_empty());
    }

    #[test]
    fn test_multiple_headers_mixed() {
        let mut extra = HashMap::new();
        extra.insert(
            "extraAuthHeaders".to_string(),
            json!([
                {"name": "app-id", "value": "test-app"},
                {"name": "Model-Id", "tokenSource": "request_model"},
                {"name": "Authorization", "value": "tok123", "valuePrefix": "Bearer "}
            ]),
        );
        let body = json!({"model": "maas-glm-5.2-aliyun"});

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(resolve_extra_auth_headers(&extra, None, Some(&body)))
            .unwrap();

        assert_eq!(result.len(), 3);
        assert_eq!(result[0], ("app-id".to_string(), "test-app".to_string()));
        assert_eq!(
            result[1],
            ("Model-Id".to_string(), "maas-glm-5.2-aliyun".to_string())
        );
        assert_eq!(
            result[2],
            ("Authorization".to_string(), "Bearer tok123".to_string())
        );
    }
}
