// unused imports removed
use std::path::PathBuf;

use crate::config::{
    atomic_write, delete_file, get_home_dir, sanitize_provider_name, write_json_file,
    write_text_file,
};
use crate::error::AppError;
use serde_json::Value;
use std::fs;
use std::path::Path;
use toml_edit::DocumentMut;

const PRESERVED_LIVE_TABLES: &[&str] = &["mcp_servers", "projects"];

/// 获取 Codex 配置目录路径
pub fn get_codex_config_dir() -> PathBuf {
    if let Some(custom) = crate::settings::get_codex_override_dir() {
        return custom;
    }

    get_home_dir().join(".codex")
}

/// 获取 Codex auth.json 路径
pub fn get_codex_auth_path() -> PathBuf {
    get_codex_config_dir().join("auth.json")
}

/// 获取 Codex config.toml 路径
pub fn get_codex_config_path() -> PathBuf {
    get_codex_config_dir().join("config.toml")
}

/// 获取 Codex 供应商配置文件路径
#[allow(dead_code)]
pub fn get_codex_provider_paths(
    provider_id: &str,
    provider_name: Option<&str>,
) -> (PathBuf, PathBuf) {
    let base_name = provider_name
        .map(sanitize_provider_name)
        .unwrap_or_else(|| sanitize_provider_name(provider_id));

    let auth_path = get_codex_config_dir().join(format!("auth-{base_name}.json"));
    let config_path = get_codex_config_dir().join(format!("config-{base_name}.toml"));

    (auth_path, config_path)
}

/// 删除 Codex 供应商配置文件
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

/// 原子写 Codex 的 `auth.json` 与 `config.toml`，在第二步失败时回滚第一步
pub fn write_codex_live_atomic(
    auth: &Value,
    config_text_opt: Option<&str>,
) -> Result<(), AppError> {
    let auth_path = get_codex_auth_path();
    let config_path = get_codex_config_path();

    if let Some(parent) = auth_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    // 读取旧内容用于回滚
    let old_auth = if auth_path.exists() {
        Some(fs::read(&auth_path).map_err(|e| AppError::io(&auth_path, e))?)
    } else {
        None
    };
    let _old_config = if config_path.exists() {
        Some(fs::read(&config_path).map_err(|e| AppError::io(&config_path, e))?)
    } else {
        None
    };

    // 准备写入内容
    let cfg_text = match config_text_opt {
        Some(s) => s.to_string(),
        None => String::new(),
    };
    if !cfg_text.trim().is_empty() {
        toml::from_str::<toml::Table>(&cfg_text).map_err(|e| AppError::toml(&config_path, e))?;
    }

    // 第一步：写 auth.json
    write_json_file(&auth_path, auth)?;

    // 第二步：写 config.toml（失败则回滚 auth.json）
    if let Err(e) = write_text_file(&config_path, &cfg_text) {
        // 回滚 auth.json
        if let Some(bytes) = old_auth {
            let _ = atomic_write(&auth_path, &bytes);
        } else {
            let _ = delete_file(&auth_path);
        }
        return Err(e);
    }

    Ok(())
}

/// Merge provider-owned Codex config with the current live config while preserving
/// user-managed tables such as `mcp_servers` and `projects`.
pub fn merge_codex_live_config(
    provider_config_text: &str,
    existing_live_text: Option<&str>,
) -> Result<String, AppError> {
    let config_path = get_codex_config_path();
    let mut provider_doc = if provider_config_text.trim().is_empty() {
        DocumentMut::new()
    } else {
        provider_config_text.parse::<DocumentMut>().map_err(|e| {
            AppError::Config(format!(
                "解析 Codex provider config.toml 失败 ({}): {e}",
                config_path.display()
            ))
        })?
    };

    let Some(existing_live_text) = existing_live_text else {
        return Ok(provider_doc.to_string());
    };

    if existing_live_text.trim().is_empty() {
        return Ok(provider_doc.to_string());
    }

    let existing_doc = match existing_live_text.parse::<DocumentMut>() {
        Ok(doc) => doc,
        Err(e) => {
            log::warn!(
                "解析现有 Codex config.toml 失败 ({}): {e}; 将跳过保留的 live 表并继续写入 provider 配置",
                config_path.display()
            );
            return Ok(provider_doc.to_string());
        }
    };

    for key in PRESERVED_LIVE_TABLES {
        if let Some(item) = existing_doc.get(key) {
            provider_doc[key] = item.clone();
        }
    }

    Ok(provider_doc.to_string())
}

/// 读取 `~/.codex/config.toml`，若不存在返回空字符串
pub fn read_codex_config_text() -> Result<String, AppError> {
    let path = get_codex_config_path();
    if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))
    } else {
        Ok(String::new())
    }
}

/// 对非空的 TOML 文本进行语法校验
pub fn validate_config_toml(text: &str) -> Result<(), AppError> {
    if text.trim().is_empty() {
        return Ok(());
    }
    toml::from_str::<toml::Table>(text)
        .map(|_| ())
        .map_err(|e| AppError::toml(Path::new("config.toml"), e))
}

/// 读取并校验 `~/.codex/config.toml`，返回文本（可能为空）
pub fn read_and_validate_codex_config_text() -> Result<String, AppError> {
    let s = read_codex_config_text()?;
    validate_config_toml(&s)?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_codex_live_config_preserves_mcp_servers_and_projects() {
        let provider = r#"model_provider = "custom"
model = "gpt-5"

[model_providers.custom]
base_url = "https://provider-b.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#;

        let existing = r#"model_provider = "custom"
model = "gpt-4o"

[model_providers.custom]
base_url = "http://127.0.0.1:5000/v1"
wire_api = "responses"
requires_openai_auth = true

[mcp_servers.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[projects."/tmp/demo"]
trust_level = "trusted"
"#;

        let merged = merge_codex_live_config(provider, Some(existing)).expect("merge config");

        assert!(merged.contains("base_url = \"https://provider-b.example/v1\""));
        assert!(merged.contains("[mcp_servers.context7]"));
        assert!(merged.contains("@upstash/context7-mcp"));
        assert!(merged.contains("[projects.\"/tmp/demo\"]"));
    }

    #[test]
    fn merge_codex_live_config_falls_back_when_existing_live_is_malformed() {
        let provider = r#"model_provider = "custom"
model = "gpt-5"

[model_providers.custom]
base_url = "https://provider-b.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#;

        let malformed_existing = r#"model_provider = "custom"

[mcp_servers.context7
command = "npx"
"#;

        let merged =
            merge_codex_live_config(provider, Some(malformed_existing)).expect("merge config");

        assert!(merged.contains("base_url = \"https://provider-b.example/v1\""));
        assert!(!merged.contains("[mcp_servers.context7]"));
    }
}
