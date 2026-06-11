use serde_json::{json, Value};
use std::path::PathBuf;

use crate::error::AppError;

pub const ANTIGRAVITY_KEYTAR_SERVICE: &str = "gemini:antigravity";
pub const ANTIGRAVITY_KEYTAR_ACCOUNT: &str = "antigravity";

pub fn get_antigravity_dir() -> PathBuf {
    if let Some(custom) = crate::settings::get_antigravity_override_dir() {
        return custom;
    }

    crate::config::get_home_dir().join(".gemini")
}

fn read_antigravity_credential_helper() -> Result<Value, AppError> {
    if let Ok(password) = read_password() {
        if let Ok(credential) = parse_credential(&password) {
            return Ok(credential);
        }
    }

    // 回退到读取 ~/.gemini/oauth_creds.json
    let oauth_path = get_antigravity_dir().join("oauth_creds.json");
    if oauth_path.is_file() {
        if let Ok(content) = std::fs::read_to_string(&oauth_path) {
            if let Ok(credential) = serde_json::from_str::<Value>(&content) {
                if credential.is_object() && !credential.as_object().unwrap().is_empty() {
                    return Ok(credential);
                }
            }
        }
    }

    Err(credential_not_found())
}

pub fn read_antigravity_live_settings() -> Result<Value, AppError> {
    let credential = read_antigravity_credential_helper()?;
    Ok(json!({ "credential": credential }))
}

pub fn capture_antigravity_credential(settings: &mut Value) -> Result<(), AppError> {
    let credential = read_antigravity_credential_helper()?;
    merge_credential(settings, credential)
}

pub fn merge_live_settings(stored_settings: &Value, live_settings: &Value) -> Value {
    let mut merged = stored_settings.clone();
    if let Some(credential) = live_settings.get("credential") {
        if let Some(settings) = merged.as_object_mut() {
            settings.insert("credential".to_string(), credential.clone());
        }
    }
    merged
}

fn merge_credential(settings: &mut Value, credential: Value) -> Result<(), AppError> {
    let settings = settings.as_object_mut().ok_or_else(|| {
        AppError::localized(
            "provider.antigravity.settings.not_object",
            "Antigravity 2.0 配置必须是 JSON 对象",
            "Antigravity 2.0 settings must be a JSON object",
        )
    })?;
    settings.insert("credential".to_string(), credential);
    Ok(())
}

pub fn write_antigravity_live_settings(settings: &Value) -> Result<(), AppError> {
    validate_antigravity_settings(settings)?;
    let Some(credential) = settings.get("credential") else {
        return delete_password();
    };
    let credential = credential.as_object().expect("validated credential object");

    let password =
        serde_json::to_string(credential).map_err(|source| AppError::JsonSerialize { source })?;
    write_password(&password)
}

pub fn validate_antigravity_settings(settings: &Value) -> Result<(), AppError> {
    let settings = settings.as_object().ok_or_else(|| {
        AppError::localized(
            "provider.antigravity.settings.not_object",
            "Antigravity 2.0 配置必须是 JSON 对象",
            "Antigravity 2.0 settings must be a JSON object",
        )
    })?;
    let Some(credential) = settings.get("credential") else {
        return Ok(());
    };

    let Some(credential) = credential.as_object() else {
        return Err(AppError::localized(
            "provider.antigravity.credential.not_object",
            "Antigravity 2.0 credential 必须是 JSON 对象",
            "Antigravity 2.0 credential must be a JSON object",
        ));
    };
    if credential.is_empty() {
        return Err(AppError::localized(
            "provider.antigravity.credential.empty",
            "Antigravity 2.0 credential 不能为空",
            "Antigravity 2.0 credential cannot be empty",
        ));
    }

    Ok(())
}

fn parse_credential(password: &str) -> Result<Value, AppError> {
    if password.trim().is_empty() {
        return Err(AppError::localized(
            "antigravity.credential.empty",
            "Antigravity 2.0 系统凭证为空",
            "The Antigravity 2.0 system credential is empty",
        ));
    }

    let credential = serde_json::from_str::<Value>(password).map_err(|source| AppError::Json {
        path: format!(
            "keytar:{}/{}",
            ANTIGRAVITY_KEYTAR_SERVICE, ANTIGRAVITY_KEYTAR_ACCOUNT
        ),
        source,
    })?;

    if credential.as_object().is_none_or(serde_json::Map::is_empty) {
        return Err(AppError::localized(
            "antigravity.credential.not_object",
            "Antigravity 2.0 系统凭证不是有效的 JSON 对象",
            "The Antigravity 2.0 system credential is not a valid JSON object",
        ));
    }

    Ok(credential)
}

#[cfg(target_os = "windows")]
fn read_password() -> Result<String, AppError> {
    use std::ptr;
    use std::slice;

    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND};
    use windows_sys::Win32::Security::Credentials::{
        CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC,
    };

    let target = wide_null(&windows_keytar_target_name());
    let mut credential_ptr: *mut CREDENTIALW = ptr::null_mut();
    let success = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential_ptr) };

    if success == 0 {
        let error = unsafe { GetLastError() };
        if error == ERROR_NOT_FOUND {
            return Err(credential_not_found());
        }
        return Err(AppError::Message(format!(
            "Failed to read Antigravity 2.0 credential from Windows Credential Manager: {error}"
        )));
    }

    let result = unsafe {
        let credential = &*credential_ptr;
        if credential.UserName.is_null() {
            Err(AppError::Message(
                "Antigravity 2.0 credential has no account name".to_string(),
            ))
        } else {
            let user_name = wide_ptr_to_string(credential.UserName);
            if user_name != ANTIGRAVITY_KEYTAR_ACCOUNT {
                Err(credential_not_found())
            } else if credential.CredentialBlobSize == 0 {
                Ok(String::new())
            } else if credential.CredentialBlob.is_null() {
                Err(AppError::Message(
                    "Antigravity 2.0 credential has no password data".to_string(),
                ))
            } else {
                let bytes = slice::from_raw_parts(
                    credential.CredentialBlob,
                    credential.CredentialBlobSize as usize,
                );
                String::from_utf8(bytes.to_vec()).map_err(|error| {
                    AppError::Message(format!(
                        "Antigravity 2.0 credential is not valid UTF-8: {error}"
                    ))
                })
            }
        }
    };

    unsafe { CredFree(credential_ptr.cast()) };
    result
}

#[cfg(target_os = "windows")]
fn write_password(password: &str) -> Result<(), AppError> {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Security::Credentials::{
        CredWriteW, CREDENTIALW, CRED_PERSIST_ENTERPRISE, CRED_TYPE_GENERIC,
    };

    let mut target = wide_null(&windows_keytar_target_name());
    let mut user_name = wide_null(ANTIGRAVITY_KEYTAR_ACCOUNT);
    let mut password_bytes = password.as_bytes().to_vec();

    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_mut_ptr(),
        CredentialBlobSize: password_bytes.len() as u32,
        CredentialBlob: password_bytes.as_mut_ptr(),
        Persist: CRED_PERSIST_ENTERPRISE,
        UserName: user_name.as_mut_ptr(),
        ..Default::default()
    };

    let success = unsafe { CredWriteW(&credential, 0) };
    password_bytes.fill(0);

    if success == 0 {
        let error = unsafe { GetLastError() };
        return Err(AppError::Message(format!(
            "Failed to write Antigravity 2.0 credential to Windows Credential Manager: {error}"
        )));
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn delete_password() -> Result<(), AppError> {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND};
    use windows_sys::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};

    let target = wide_null(&windows_keytar_target_name());
    let success = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
    if success == 0 {
        let error = unsafe { GetLastError() };
        if error != ERROR_NOT_FOUND {
            return Err(AppError::Message(format!(
                "Failed to delete Antigravity 2.0 credential from Windows Credential Manager: {error}"
            )));
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_keytar_target_name() -> String {
    // keytar uses the service as TargetName and stores the account in UserName.
    ANTIGRAVITY_KEYTAR_SERVICE.to_string()
}

#[cfg(target_os = "windows")]
fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
fn wide_ptr_to_string(value: *const u16) -> String {
    let mut len = 0usize;
    unsafe {
        while *value.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(value, len))
    }
}

#[cfg(target_os = "macos")]
fn read_password() -> Result<String, AppError> {
    let output = std::process::Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            ANTIGRAVITY_KEYTAR_SERVICE,
            "-a",
            ANTIGRAVITY_KEYTAR_ACCOUNT,
            "-w",
        ])
        .output()
        .map_err(|source| AppError::IoContext {
            context: "Failed to execute macOS security command".to_string(),
            source,
        })?;

    if !output.status.success() {
        return Err(credential_not_found());
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end_matches(['\r', '\n'])
        .to_string())
}

#[cfg(target_os = "macos")]
fn write_password(password: &str) -> Result<(), AppError> {
    let status = std::process::Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-s",
            ANTIGRAVITY_KEYTAR_SERVICE,
            "-a",
            ANTIGRAVITY_KEYTAR_ACCOUNT,
            "-w",
            password,
        ])
        .status()
        .map_err(|source| AppError::IoContext {
            context: "Failed to execute macOS security command".to_string(),
            source,
        })?;

    if !status.success() {
        return Err(AppError::Message(
            "Failed to write Antigravity 2.0 credential to macOS Keychain".to_string(),
        ));
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn delete_password() -> Result<(), AppError> {
    if read_password().is_err() {
        return Ok(());
    }

    let status = std::process::Command::new("security")
        .args([
            "delete-generic-password",
            "-s",
            ANTIGRAVITY_KEYTAR_SERVICE,
            "-a",
            ANTIGRAVITY_KEYTAR_ACCOUNT,
        ])
        .status()
        .map_err(|source| AppError::IoContext {
            context: "Failed to execute macOS security command".to_string(),
            source,
        })?;
    if !status.success() {
        return Err(AppError::Message(
            "Failed to delete Antigravity 2.0 credential from macOS Keychain".to_string(),
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn read_password() -> Result<String, AppError> {
    let output = std::process::Command::new("secret-tool")
        .args([
            "lookup",
            "service",
            ANTIGRAVITY_KEYTAR_SERVICE,
            "account",
            ANTIGRAVITY_KEYTAR_ACCOUNT,
        ])
        .output()
        .map_err(|source| AppError::IoContext {
            context: "Failed to execute secret-tool".to_string(),
            source,
        })?;

    if !output.status.success() {
        return Err(credential_not_found());
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end_matches(['\r', '\n'])
        .to_string())
}

#[cfg(target_os = "linux")]
fn write_password(password: &str) -> Result<(), AppError> {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = std::process::Command::new("secret-tool")
        .args([
            "store",
            "--label",
            "Antigravity",
            "service",
            ANTIGRAVITY_KEYTAR_SERVICE,
            "account",
            ANTIGRAVITY_KEYTAR_ACCOUNT,
        ])
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|source| AppError::IoContext {
            context: "Failed to execute secret-tool".to_string(),
            source,
        })?;

    child
        .stdin
        .take()
        .ok_or_else(|| AppError::Message("Failed to open secret-tool stdin".to_string()))?
        .write_all(password.as_bytes())
        .map_err(|source| AppError::IoContext {
            context: "Failed to write Antigravity 2.0 credential to secret-tool".to_string(),
            source,
        })?;

    let status = child.wait().map_err(|source| AppError::IoContext {
        context: "Failed to wait for secret-tool".to_string(),
        source,
    })?;
    if !status.success() {
        return Err(AppError::Message(
            "Failed to write Antigravity 2.0 credential to the system keyring".to_string(),
        ));
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn delete_password() -> Result<(), AppError> {
    if read_password().is_err() {
        return Ok(());
    }

    let status = std::process::Command::new("secret-tool")
        .args([
            "clear",
            "service",
            ANTIGRAVITY_KEYTAR_SERVICE,
            "account",
            ANTIGRAVITY_KEYTAR_ACCOUNT,
        ])
        .status()
        .map_err(|source| AppError::IoContext {
            context: "Failed to execute secret-tool".to_string(),
            source,
        })?;
    if !status.success() {
        return Err(AppError::Message(
            "Failed to delete Antigravity 2.0 credential from the system keyring".to_string(),
        ));
    }
    Ok(())
}

fn credential_not_found() -> AppError {
    AppError::localized(
        "antigravity.credential.not_found",
        "未找到 Antigravity 2.0 登录凭证，请先在 Antigravity 2.0 中登录账号",
        "Antigravity 2.0 login credential was not found. Sign in to Antigravity 2.0 first",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_credential_preserves_unknown_fields() {
        let credential = parse_credential(
            r#"{"token":{"access_token":"access","refresh_token":"refresh"},"auth_method":"oauth","future":1}"#,
        )
        .unwrap();

        assert_eq!(credential["token"]["access_token"], "access");
        assert_eq!(credential["future"], 1);
    }

    #[test]
    fn validate_settings_accepts_unbound_provider() {
        validate_antigravity_settings(&json!({ "env": {}, "config": {} })).unwrap();
    }

    #[test]
    fn validate_settings_rejects_empty_credential() {
        let error = validate_antigravity_settings(&json!({ "credential": {} })).unwrap_err();
        assert!(error.to_string().contains("credential"));
    }

    #[test]
    fn merge_credential_preserves_gemini_style_settings() {
        let mut settings = json!({
            "env": {
                "GOOGLE_GEMINI_BASE_URL": "https://example.com",
                "GEMINI_API_KEY": "secret"
            },
            "config": {
                "timeout": 30000
            }
        });

        merge_credential(&mut settings, json!({ "account": "user@example.com" })).unwrap();

        assert_eq!(
            settings["env"]["GOOGLE_GEMINI_BASE_URL"],
            "https://example.com"
        );
        assert_eq!(settings["config"]["timeout"], 30000);
        assert_eq!(settings["credential"]["account"], "user@example.com");
    }

    #[test]
    fn merge_live_settings_preserves_stored_configuration() {
        let stored = json!({
            "env": { "GOOGLE_GEMINI_BASE_URL": "https://example.com" },
            "config": { "timeout": 30000 }
        });
        let live = json!({
            "credential": { "account": "second@example.com" }
        });

        let merged = merge_live_settings(&stored, &live);

        assert_eq!(
            merged["env"]["GOOGLE_GEMINI_BASE_URL"],
            "https://example.com"
        );
        assert_eq!(merged["config"]["timeout"], 30000);
        assert_eq!(merged["credential"]["account"], "second@example.com");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_target_name_matches_keytar_encoding() {
        assert_eq!(windows_keytar_target_name(), "gemini:antigravity");
    }
}
