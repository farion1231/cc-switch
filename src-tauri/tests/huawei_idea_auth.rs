use cc_switch_lib::{
    decrypt_password, encrypt_password, HuaweiCodeAgentAuthManager, HuaweiCodeAgentToken,
};

mod support;

#[test]
fn test_huawei_idea_auth_manager_new() {
    let _lock = support::ensure_test_home();
    let config_dir = std::env::temp_dir().join("cc-switch-test-idea-auth");
    let _ = std::fs::remove_dir_all(&config_dir);
    std::fs::create_dir_all(&config_dir).unwrap();

    let manager = HuaweiCodeAgentAuthManager::new(config_dir.clone());
    let rt = tokio::runtime::Runtime::new().unwrap();
    let status = rt.block_on(manager.get_status());
    assert!(!status.authenticated);
    assert!(status.user.is_none());
}

#[test]
fn test_huawei_idea_auth_manager_logout() {
    let _lock = support::ensure_test_home();
    let config_dir = std::env::temp_dir().join("cc-switch-test-idea-auth-logout");
    let _ = std::fs::remove_dir_all(&config_dir);
    std::fs::create_dir_all(&config_dir).unwrap();

    let manager = HuaweiCodeAgentAuthManager::new(config_dir.clone());
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(manager.logout());
    assert!(result.is_ok());
}

#[test]
fn test_encrypt_decrypt_password_roundtrip() {
    let password = "test_password_123";
    let encrypted = encrypt_password(password).unwrap();
    let decrypted = decrypt_password(&encrypted).unwrap();
    assert_eq!(decrypted, password);
}

#[test]
fn test_encrypt_produces_different_ciphertext() {
    let password = "same_password";
    let enc1 = encrypt_password(password).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1));
    let enc2 = encrypt_password(password).unwrap();
    assert_ne!(enc1, enc2);
}

#[test]
fn test_decrypt_invalid_data_fails() {
    let result = decrypt_password("not_valid_base64!!!");
    assert!(result.is_err());
}

#[test]
fn test_huawei_idea_token_backward_compat() {
    let json = r#"{"account_id":"idea_test","user":"test","token":"abc","expires_at":1000}"#;
    let token: HuaweiCodeAgentToken = serde_json::from_str(json).unwrap();
    assert_eq!(token.account_id, "idea_test");
    assert!(token.encrypted_password.is_none());
}
