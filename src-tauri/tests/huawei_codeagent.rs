use cc_switch_lib::{ExtraAuthHeader, ProviderType};
use serde_json::json;
use std::collections::HashMap;
use std::str::FromStr;

mod support;

#[test]
fn test_provider_type_from_str_huawei_codeagent_2() {
    let pt = ProviderType::from_str("huawei_codeagent_2").unwrap();
    assert_eq!(pt, ProviderType::HuaweiCodeAgent2);
}

#[test]
fn test_provider_type_from_str_huawei_codeagent_3() {
    let pt = ProviderType::from_str("huawei_codeagent_3").unwrap();
    assert_eq!(pt, ProviderType::HuaweiCodeAgent3);
}

#[test]
fn test_provider_type_from_str_huawei_w3_backward_compat() {
    let pt = ProviderType::from_str("huawei_w3").unwrap();
    assert_eq!(pt, ProviderType::HuaweiCodeAgent2);
}

#[test]
fn test_provider_type_from_str_huawei_idea_backward_compat() {
    let pt = ProviderType::from_str("huawei_idea").unwrap();
    assert_eq!(pt, ProviderType::HuaweiCodeAgent2);
}

#[test]
fn test_provider_type_as_str_huawei_codeagent_2() {
    assert_eq!(
        ProviderType::HuaweiCodeAgent2.as_str(),
        "huawei_codeagent_2"
    );
}

#[test]
fn test_provider_type_as_str_huawei_codeagent_3() {
    assert_eq!(
        ProviderType::HuaweiCodeAgent3.as_str(),
        "huawei_codeagent_3"
    );
}

#[test]
fn test_extra_auth_header_deserialize_with_value_prefix() {
    let json_str =
        r#"{"name":"Authorization","tokenSource":"huawei_codeagent_2","valuePrefix":"Bearer "}"#;
    let header: ExtraAuthHeader = serde_json::from_str(json_str).unwrap();
    assert_eq!(header.name, "Authorization");
    assert_eq!(header.token_source.as_deref(), Some("huawei_codeagent_2"));
    assert_eq!(header.value_prefix.as_deref(), Some("Bearer "));
    assert!(header.value.is_none());
}

#[test]
fn test_extra_auth_header_deserialize_without_value_prefix() {
    let json_str = r#"{"name":"app-id","value":"CodeAgent2.0"}"#;
    let header: ExtraAuthHeader = serde_json::from_str(json_str).unwrap();
    assert_eq!(header.name, "app-id");
    assert_eq!(header.value.as_deref(), Some("CodeAgent2.0"));
    assert!(header.token_source.is_none());
    assert!(header.value_prefix.is_none());
}

#[test]
fn test_resolve_extra_auth_headers_value_prefix() {
    let _lock = support::ensure_test_home();
    let mut extra = HashMap::new();
    extra.insert(
        "extraAuthHeaders".to_string(),
        json!([
            {"name": "Authorization", "value": "my-token", "valuePrefix": "Bearer "}
        ]),
    );

    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt
        .block_on(cc_switch_lib::resolve_extra_auth_headers(
            &extra, None, None,
        ))
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "Authorization");
    assert_eq!(result[0].1, "Bearer my-token");
}

#[test]
fn test_resolve_extra_auth_headers_request_model() {
    let _lock = support::ensure_test_home();
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
        .block_on(cc_switch_lib::resolve_extra_auth_headers(
            &extra,
            None,
            Some(&body),
        ))
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "Model-Id");
    assert_eq!(result[0].1, "maas-glm-5.2-aliyun");
}

#[test]
fn test_resolve_extra_auth_headers_request_model_non_ascii() {
    let _lock = support::ensure_test_home();
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
        .block_on(cc_switch_lib::resolve_extra_auth_headers(
            &extra,
            None,
            Some(&body),
        ))
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "Model-Id");
    assert_eq!(result[0].1, "");
}

#[test]
fn test_resolve_extra_auth_headers_request_model_body_none() {
    let _lock = support::ensure_test_home();
    let mut extra = HashMap::new();
    extra.insert(
        "extraAuthHeaders".to_string(),
        json!([
            {"name": "Model-Id", "tokenSource": "request_model"}
        ]),
    );

    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt
        .block_on(cc_switch_lib::resolve_extra_auth_headers(
            &extra, None, None,
        ))
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "Model-Id");
    assert_eq!(result[0].1, "");
}

#[test]
fn test_provider_type_needs_transform_huawei_codeagent_2() {
    assert!(ProviderType::HuaweiCodeAgent2.needs_transform());
}

#[test]
fn test_provider_type_needs_transform_huawei_codeagent_3() {
    assert!(ProviderType::HuaweiCodeAgent3.needs_transform());
}

#[test]
fn test_provider_type_default_endpoint_huawei_codeagent_2() {
    assert_eq!(
        ProviderType::HuaweiCodeAgent2.default_endpoint(),
        "https://codeagentcli.rnd.huawei.com"
    );
}

#[test]
fn test_provider_type_default_endpoint_huawei_codeagent_3() {
    assert_eq!(
        ProviderType::HuaweiCodeAgent3.default_endpoint(),
        "https://codeagentcli.rnd.huawei.com"
    );
}

#[test]
fn test_token_file_fallback_loads_from_legacy_w3() {
    let _lock = support::ensure_test_home();
    let config_dir = std::env::temp_dir().join("cc-switch-test-token-fallback-w3");
    let _ = std::fs::remove_dir_all(&config_dir);
    std::fs::create_dir_all(&config_dir).unwrap();

    let legacy_path = config_dir.join("huawei_w3_tokens.json");
    let token_json = r#"{
        "idea_test_user": {
            "account_id": "idea_test_user",
            "user": "testuser",
            "token": "fake_token_123",
            "expires_at": 9999999999,
            "is_default": true,
            "encrypted_password": null
        }
    }"#;
    std::fs::write(&legacy_path, token_json).unwrap();

    let manager = cc_switch_lib::HuaweiCodeAgentAuthManager::new(config_dir.clone());
    manager.load_tokens_sync().unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    let status = rt.block_on(manager.get_status());
    assert!(status.authenticated);
    assert_eq!(status.user.as_deref(), Some("testuser"));

    let _ = std::fs::remove_dir_all(&config_dir);
}

#[test]
fn test_encrypt_decrypt_password_roundtrip() {
    let _lock = support::ensure_test_home();
    let password = "test_password_456";
    let encrypted = cc_switch_lib::encrypt_password(password).unwrap();
    let decrypted = cc_switch_lib::decrypt_password(&encrypted).unwrap();
    assert_eq!(decrypted, password);
}
