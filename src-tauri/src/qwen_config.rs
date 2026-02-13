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
///
/// 支持的格式：
/// - KEY=value
/// - KEY="value with spaces"
/// - KEY='value with spaces'
/// - # 注释行
pub fn parse_env_file(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_string();
            let value = value.trim();
            // 处理带引号的值
            let value = if (value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\''))
            {
                if value.len() >= 2 {
                    &value[1..value.len() - 1]
                } else {
                    value
                }
            } else {
                value
            }
            .to_string();
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
        lines.push(format!("{key}={value}"));
    }
    lines.join("\n")
}

/// 读取 Qwen .env 配置
pub fn read_qwen_env() -> Result<HashMap<String, String>, AppError> {
    let env_path = get_qwen_env_path();
    if !env_path.exists() {
        return Ok(HashMap::new());
    }
    let content = fs::read_to_string(&env_path).map_err(|e| AppError::Io {
        path: env_path.display().to_string(),
        source: e,
    })?;
    Ok(parse_env_file(&content))
}

/// 写入 Qwen .env 配置
pub fn write_qwen_env(env_map: &HashMap<String, String>) -> Result<(), AppError> {
    write_qwen_env_atomic(env_map)
}

/// 写入 Qwen live 配置（切换 provider 时使用）
///
/// 保留现有 .env 文件中的其他配置，仅更新指定的三个字段
pub fn write_qwen_live(
    openai_api_key: &str,
    openai_base_url: &str,
    openai_model: &str,
) -> Result<(), AppError> {
    // 先读取现有配置，保留用户可能添加的其他环境变量
    let mut env_map = read_qwen_env().unwrap_or_default();
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
    let mut json_map = serde_json::Map::new();

    for (key, value) in env_map {
        json_map.insert(key.clone(), Value::String(value.clone()));
    }

    serde_json::json!({ "env": json_map })
}

/// 从 Provider.settings_config (JSON Value) 提取 .env 格式
pub fn json_to_env(settings: &Value) -> Result<HashMap<String, String>, AppError> {
    let mut env_map = HashMap::new();

    if let Some(env_obj) = settings.get("env").and_then(|v| v.as_object()) {
        for (key, value) in env_obj {
            if let Some(val_str) = value.as_str() {
                env_map.insert(key.clone(), val_str.to_string());
            }
        }
    }

    Ok(env_map)
}

/// 写入 Qwen .env 文件（原子操作）
pub fn write_qwen_env_atomic(map: &HashMap<String, String>) -> Result<(), AppError> {
    let path = get_qwen_env_path();

    // 确保目录存在
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;

        // 设置目录权限为 700（仅所有者可读写执行）
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(parent)
                .map_err(|e| AppError::io(parent, e))?
                .permissions();
            perms.set_mode(0o700);
            fs::set_permissions(parent, perms).map_err(|e| AppError::io(parent, e))?;
        }
    }

    let content = serialize_env_file(map);
    write_text_file(&path, &content)?;

    // 设置文件权限为 600（仅所有者可读写）
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path)
            .map_err(|e| AppError::io(&path, e))?
            .permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&path, perms).map_err(|e| AppError::io(&path, e))?;
    }

    Ok(())
}
