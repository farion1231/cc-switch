use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::{
    atomic_write, delete_file, get_codex_config_dir, sanitize_provider_name, write_json_file,
    write_text_file,
};
use crate::error::AppError;

pub fn get_codex_auth_path() -> PathBuf {
    get_codex_config_dir().join("auth.json")
}

pub fn get_codex_config_path() -> PathBuf {
    get_codex_config_dir().join("config.toml")
}

#[allow(dead_code)]
pub fn get_codex_provider_paths(
    provider_id: &str,
    provider_name: Option<&str>,
) -> (PathBuf, PathBuf) {
    let base_name = provider_name
        .map(sanitize_provider_name)
        .unwrap_or_else(|| sanitize_provider_name(provider_id));

    (
        get_codex_config_dir().join(format!("auth-{base_name}.json")),
        get_codex_config_dir().join(format!("config-{base_name}.toml")),
    )
}

#[allow(dead_code)]
pub fn delete_codex_provider_config(
    provider_id: &str,
    provider_name: &str,
) -> Result<(), AppError> {
    let (auth_path, config_path) = get_codex_provider_paths(provider_id, Some(provider_name));
    delete_file(&auth_path).ok();
    delete_file(&config_path).ok();
    Ok(())
}

pub fn write_codex_live_atomic(
    auth: &Value,
    config_text_opt: Option<&str>,
) -> Result<(), AppError> {
    let auth_path = get_codex_auth_path();
    let config_path = get_codex_config_path();

    if let Some(parent) = auth_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let old_auth = if auth_path.exists() {
        Some(fs::read(&auth_path).map_err(|e| AppError::io(&auth_path, e))?)
    } else {
        None
    };

    let config_text = config_text_opt.unwrap_or_default().to_string();
    validate_config_toml(&config_text)?;

    write_json_file(&auth_path, auth)?;

    if let Err(err) = write_text_file(&config_path, &config_text) {
        if let Some(bytes) = old_auth {
            let _ = atomic_write(&auth_path, &bytes);
        } else {
            let _ = delete_file(&auth_path);
        }
        return Err(err);
    }

    Ok(())
}

pub fn read_codex_config_text() -> Result<String, AppError> {
    let path = get_codex_config_path();
    if path.exists() {
        fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))
    } else {
        Ok(String::new())
    }
}

pub fn validate_config_toml(text: &str) -> Result<(), AppError> {
    if text.trim().is_empty() {
        return Ok(());
    }

    toml::from_str::<toml::Table>(text)
        .map(|_| ())
        .map_err(|e| AppError::toml(Path::new("config.toml"), e))
}

pub fn read_and_validate_codex_config_text() -> Result<String, AppError> {
    let text = read_codex_config_text()?;
    validate_config_toml(&text)?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use serial_test::serial;
    use tempfile::tempdir;

    #[test]
    #[serial]
    fn codex_live_write_round_trip() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        write_codex_live_atomic(
            &json!({ "OPENAI_API_KEY": "test-key" }),
            Some("model = \"gpt-5\"\n"),
        )?;

        let auth: Value = crate::config::read_json_file(&get_codex_auth_path())?;
        assert_eq!(
            auth.get("OPENAI_API_KEY").and_then(|item| item.as_str()),
            Some("test-key")
        );
        assert!(read_codex_config_text()?.contains("gpt-5"));

        Ok(())
    }
}
