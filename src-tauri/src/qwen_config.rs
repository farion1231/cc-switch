use crate::config::{get_home_dir, write_text_file};
use crate::error::AppError;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// 获取 Qwen 配置目录路径
pub fn get_qwen_dir() -> PathBuf {
    get_home_dir().join(".qwen")
}

/// 获取 Qwen .env 文件路径
pub fn get_qwen_env_path() -> PathBuf {
    get_qwen_dir().join(".env")
}

/// 解析 .env 文件内容为键值对
pub fn parse_env_file(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_string();
            let value = value.trim().to_string();
            if !key.is_empty() && key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                map.insert(key, value);
            }
        }
    }
    map
}

/// 将键值对序列化为 .env 文件内容
pub fn serialize_env_file(map: &HashMap<String, String>) -> String {
    let mut lines = Vec::new();
    for (key, value) in map {
        lines.push(format!("{}={}", key, value));
    }
    lines.join("\n")
}

/// 读取 Qwen .env 配置
pub fn read_qwen_env() -> Result<HashMap<String, String>, AppError> {
    let env_path = get_qwen_env_path();
    if !env_path.exists() {
        return Ok(HashMap::new());
    }
    let content = fs::read_to_string(&env_path)?;
    Ok(parse_env_file(&content))
}

/// 写入 Qwen .env 配置
pub fn write_qwen_env(env_map: &HashMap<String, String>) -> Result<(), AppError> {
    let env_path = get_qwen_env_path();
    if let Some(parent) = env_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serialize_env_file(env_map);
    fs::write(&env_path, content)?;
    Ok(())
}

/// 写入 Qwen live 配置（切换 provider 时使用）
pub fn write_qwen_live(
    openai_api_key: &str,
    openai_base_url: &str,
    openai_model: &str,
) -> Result<(), AppError> {
    let mut env_map = HashMap::new();
    env_map.insert("OPENAI_API_KEY".to_string(), openai_api_key.to_string());
    env_map.insert("OPENAI_BASE_URL".to_string(), openai_base_url.to_string());
    env_map.insert("OPENAI_MODEL".to_string(), openai_model.to_string());
    write_qwen_env(&env_map)
}

/// 清空 Qwen live 配置
pub fn clear_qwen_live() -> Result<(), AppError> {
    write_qwen_env(&HashMap::new())
}

/// 将环境变量转换为 JSON 格式
pub fn env_to_json(env_map: &HashMap<String, String>) -> Value {
    serde_json::to_value(env_map).unwrap_or_default()
}
