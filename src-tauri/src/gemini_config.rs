use crate::config::write_text_file;
use crate::error::AppError;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// 获取用户主目录，带回退和日志
fn get_home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| {
        log::warn!("无法获取用户主目录，回退到当前目录");
        PathBuf::from(".")
    })
}

/// 获取 Gemini 配置目录路径（支持设置覆盖）
pub fn get_gemini_dir() -> PathBuf {
    if let Some(custom) = crate::settings::get_gemini_override_dir() {
        return custom;
    }

    get_home_dir().join(".gemini")
}

/// 获取 Gemini .env 文件路径
pub fn get_gemini_env_path() -> PathBuf {
    get_gemini_dir().join(".env")
}

/// 解析 .env 文件内容为键值对
///
/// 此函数宽松地解析 .env 文件，跳过无效行。
/// 对于需要严格验证的场景，请使用 `parse_env_file_strict`。
pub fn parse_env_file(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();

    for line in content.lines() {
        let line = line.trim();

        // 跳过空行和注释
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // 解析 KEY=VALUE
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_string();
            let value = value.trim().to_string();

            // 验证 key 是否有效（不为空，只包含字母、数字和下划线）
            if !key.is_empty() && key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                map.insert(key, value);
            }
        }
    }

    map
}

/// 严格解析 .env 文件内容，返回详细的错误信息
///
/// 与 `parse_env_file` 不同，此函数在遇到无效行时会返回错误，
/// 包含行号和详细的错误信息。
///
/// # 错误
///
/// 返回 `AppError` 如果遇到以下情况：
/// - 行不包含 `=` 分隔符
/// - Key 为空或包含无效字符
/// - Key 不符合环境变量命名规范
///
/// # 使用场景
///
/// 此函数为未来的严格验证场景预留，当前运行时使用宽松的 `parse_env_file`。
/// 可用于：
/// - 配置导入验证
/// - CLI 工具的严格模式
/// - 配置文件错误诊断
///
/// 已有完整的测试覆盖，可直接使用。
#[allow(dead_code)]
pub fn parse_env_file_strict(content: &str) -> Result<HashMap<String, String>, AppError> {
    let mut map = HashMap::new();

    for (line_num, line) in content.lines().enumerate() {
        let line = line.trim();
        let line_number = line_num + 1; // 行号从 1 开始

        // 跳过空行和注释
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // 检查是否包含 =
        if !line.contains('=') {
            return Err(AppError::localized(
                "gemini.env.parse_error.no_equals",
                format!("Gemini .env 文件格式错误（第 {line_number} 行）：缺少 '=' 分隔符\n行内容: {line}"),
                format!("Invalid Gemini .env format (line {line_number}): missing '=' separator\nLine: {line}"),
            ));
        }

        // 解析 KEY=VALUE
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim();

            // 验证 key 不为空
            if key.is_empty() {
                return Err(AppError::localized(
                    "gemini.env.parse_error.empty_key",
                    format!("Gemini .env 文件格式错误（第 {line_number} 行）：环境变量名不能为空\n行内容: {line}"),
                    format!("Invalid Gemini .env format (line {line_number}): variable name cannot be empty\nLine: {line}"),
                ));
            }

            // 验证 key 只包含字母、数字和下划线
            if !key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return Err(AppError::localized(
                    "gemini.env.parse_error.invalid_key",
                    format!("Gemini .env 文件格式错误（第 {line_number} 行）：环境变量名只能包含字母、数字和下划线\n变量名: {key}"),
                    format!("Invalid Gemini .env format (line {line_number}): variable name can only contain letters, numbers, and underscores\nVariable: {key}"),
                ));
            }

            map.insert(key.to_string(), value.to_string());
        }
    }

    Ok(map)
}

/// 将键值对序列化为 .env 格式
pub fn serialize_env_file(map: &HashMap<String, String>) -> String {
    let mut lines = Vec::new();

    // 按键排序以保证输出稳定
    let mut keys: Vec<_> = map.keys().collect();
    keys.sort();

    for key in keys {
        if let Some(value) = map.get(key) {
            lines.push(format!("{key}={value}"));
        }
    }

    lines.join("\n")
}

/// 读取 Gemini .env 文件
pub fn read_gemini_env() -> Result<HashMap<String, String>, AppError> {
    let path = get_gemini_env_path();

    if !path.exists() {
        return Ok(HashMap::new());
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;

    Ok(parse_env_file(&content))
}

/// 写入 Gemini .env 文件（原子操作）
pub fn write_gemini_env_atomic(map: &HashMap<String, String>) -> Result<(), AppError> {
    let path = get_gemini_env_path();

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

/// 从 .env 格式转换为 Provider.settings_config (JSON Value)
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

/// 验证 Gemini 配置的基本结构
///
/// 此函数只验证配置的基本格式，不强制要求 GEMINI_API_KEY。
/// 这允许用户先创建供应商配置，稍后再填写 API Key。
///
/// API Key 的验证会在切换供应商时进行（通过 `validate_gemini_settings_strict`）。
pub fn validate_gemini_settings(settings: &Value) -> Result<(), AppError> {
    // 只验证基本结构，不强制要求 GEMINI_API_KEY
    // 如果有 env 字段，验证它是一个对象
    if let Some(env) = settings.get("env") {
        if !env.is_object() {
            return Err(AppError::localized(
                "gemini.validation.invalid_env",
                "Gemini 配置格式错误: env 必须是对象",
                "Gemini config invalid: env must be an object",
            ));
        }
    }

    // 如果有 config 字段，验证它是对象或 null
    if let Some(config) = settings.get("config") {
        if !(config.is_object() || config.is_null()) {
            return Err(AppError::localized(
                "gemini.validation.invalid_config",
                "Gemini 配置格式错误: config 必须是对象",
                "Gemini config invalid: config must be an object",
            ));
        }
    }

    Ok(())
}

/// 严格验证 Gemini 配置（要求必需字段）
///
/// 此函数在切换供应商时使用，确保配置包含所有必需的字段。
/// 对于需要 API Key 的供应商（如 PackyCode），会验证 GEMINI_API_KEY 字段。
pub fn validate_gemini_settings_strict(settings: &Value) -> Result<(), AppError> {
    // 先做基础格式验证（包含 env/config 类型）
    validate_gemini_settings(settings)?;

    let env_map = json_to_env(settings)?;

    // 如果 env 为空，表示使用 OAuth（如 Google 官方），跳过验证
    if env_map.is_empty() {
        return Ok(());
    }

    // 如果 env 不为空，检查必需字段 GEMINI_API_KEY
    if !env_map.contains_key("GEMINI_API_KEY") {
        return Err(AppError::localized(
            "gemini.validation.missing_api_key",
            "Gemini 配置缺少必需字段: GEMINI_API_KEY",
            "Gemini config missing required field: GEMINI_API_KEY",
        ));
    }

    Ok(())
}

/// 获取 Gemini settings.json 文件路径
///
/// 返回路径：`~/.gemini/settings.json`（与 `.env` 文件同级）
pub fn get_gemini_settings_path() -> PathBuf {
    get_gemini_dir().join("settings.json")
}

/// 更新 Gemini 目录 settings.json 中的 security.auth.selectedType 字段
///
/// 此函数会：
/// 1. 读取现有的 settings.json（如果存在）
/// 2. 只更新 `security.auth.selectedType` 字段，保留其他所有字段
/// 3. 原子性写入文件
///
/// # 参数
/// - `selected_type`: 要设置的 selectedType 值（如 "gemini-api-key" 或 "oauth-personal"）
fn update_selected_type(selected_type: &str) -> Result<(), AppError> {
    let settings_path = get_gemini_settings_path();

    // 确保目录存在
    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    // 读取现有的 settings.json（如果存在）
    let mut settings_content = if settings_path.exists() {
        let content =
            fs::read_to_string(&settings_path).map_err(|e| AppError::io(&settings_path, e))?;
        serde_json::from_str::<Value>(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    // 只更新 security.auth.selectedType 字段
    if let Some(obj) = settings_content.as_object_mut() {
        let security = obj
            .entry("security")
            .or_insert_with(|| serde_json::json!({}));

        if let Some(security_obj) = security.as_object_mut() {
            let auth = security_obj
                .entry("auth")
                .or_insert_with(|| serde_json::json!({}));

            if let Some(auth_obj) = auth.as_object_mut() {
                auth_obj.insert(
                    "selectedType".to_string(),
                    Value::String(selected_type.to_string()),
                );
            }
        }
    }

    // 写入文件
    crate::config::write_json_file(&settings_path, &settings_content)?;

    Ok(())
}

/// 为 Packycode Gemini 供应商写入 settings.json
///
/// 设置 `~/.gemini/settings.json` 中的：
/// ```json
/// {
///   "security": {
///     "auth": {
///       "selectedType": "gemini-api-key"
///     }
///   }
/// }
/// ```
///
/// 保留文件中的其他所有字段。
pub fn write_packycode_settings() -> Result<(), AppError> {
    update_selected_type("gemini-api-key")
}

/// 为 Google 官方 Gemini 供应商写入 settings.json（OAuth 模式）
///
/// 设置 `~/.gemini/settings.json` 中的：
/// ```json
/// {
///   "security": {
///     "auth": {
///       "selectedType": "oauth-personal"
///     }
///   }
/// }
/// ```
///
/// 保留文件中的其他所有字段。
pub fn write_google_oauth_settings() -> Result<(), AppError> {
    update_selected_type("oauth-personal")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_env_file() {
        let content = r#"
# Comment line
GOOGLE_GEMINI_BASE_URL=https://example.com
GEMINI_API_KEY=sk-test123
GEMINI_MODEL=gemini-3-pro-preview

# Another comment
"#;

        let map = parse_env_file(content);

        assert_eq!(map.len(), 3);
        assert_eq!(
            map.get("GOOGLE_GEMINI_BASE_URL"),
            Some(&"https://example.com".to_string())
        );
        assert_eq!(map.get("GEMINI_API_KEY"), Some(&"sk-test123".to_string()));
        assert_eq!(
            map.get("GEMINI_MODEL"),
            Some(&"gemini-3-pro-preview".to_string())
        );
    }

    #[test]
    fn test_serialize_env_file() {
        let mut map = HashMap::new();
        map.insert("GEMINI_API_KEY".to_string(), "sk-test".to_string());
        map.insert(
            "GEMINI_MODEL".to_string(),
            "gemini-3-pro-preview".to_string(),
        );

        let content = serialize_env_file(&map);

        assert!(content.contains("GEMINI_API_KEY=sk-test"));
        assert!(content.contains("GEMINI_MODEL=gemini-3-pro-preview"));
    }

    #[test]
    fn test_env_json_conversion() {
        let mut env_map = HashMap::new();
        env_map.insert("GEMINI_API_KEY".to_string(), "test-key".to_string());

        let json = env_to_json(&env_map);
        let converted = json_to_env(&json).unwrap();

        assert_eq!(
            converted.get("GEMINI_API_KEY"),
            Some(&"test-key".to_string())
        );
    }

    #[test]
    fn test_parse_env_file_strict_success() {
        // 测试严格模式下正常解析
        let content = r#"
# Comment line
GOOGLE_GEMINI_BASE_URL=https://example.com
GEMINI_API_KEY=sk-test123
GEMINI_MODEL=gemini-3-pro-preview

# Another comment
"#;

        let result = parse_env_file_strict(content);
        assert!(result.is_ok());

        let map = result.unwrap();
        assert_eq!(map.len(), 3);
        assert_eq!(
            map.get("GOOGLE_GEMINI_BASE_URL"),
            Some(&"https://example.com".to_string())
        );
        assert_eq!(map.get("GEMINI_API_KEY"), Some(&"sk-test123".to_string()));
        assert_eq!(
            map.get("GEMINI_MODEL"),
            Some(&"gemini-3-pro-preview".to_string())
        );
    }

    #[test]
    fn test_parse_env_file_strict_missing_equals() {
        // 测试严格模式下检测缺少 = 的行
        let content = "GOOGLE_GEMINI_BASE_URL=https://example.com
INVALID_LINE_WITHOUT_EQUALS
GEMINI_API_KEY=sk-test123";

        let result = parse_env_file_strict(content);
        assert!(result.is_err());

        let err = result.unwrap_err();
        let err_msg = format!("{err:?}");
        assert!(err_msg.contains("第 2 行") || err_msg.contains("line 2"));
        assert!(err_msg.contains("INVALID_LINE_WITHOUT_EQUALS"));
    }

    #[test]
    fn test_parse_env_file_strict_empty_key() {
        // 测试严格模式下检测空 key
        let content = "GOOGLE_GEMINI_BASE_URL=https://example.com
=value_without_key
GEMINI_API_KEY=sk-test123";

        let result = parse_env_file_strict(content);
        assert!(result.is_err());

        let err = result.unwrap_err();
        let err_msg = format!("{err:?}");
        assert!(err_msg.contains("第 2 行") || err_msg.contains("line 2"));
        assert!(err_msg.contains("empty") || err_msg.contains("空"));
    }

    #[test]
    fn test_parse_env_file_strict_invalid_key_characters() {
        // 测试严格模式下检测无效字符（如空格、特殊符号）
        let content = "GOOGLE_GEMINI_BASE_URL=https://example.com
INVALID KEY WITH SPACES=value
GEMINI_API_KEY=sk-test123";

        let result = parse_env_file_strict(content);
        assert!(result.is_err());

        let err = result.unwrap_err();
        let err_msg = format!("{err:?}");
        assert!(err_msg.contains("第 2 行") || err_msg.contains("line 2"));
        assert!(err_msg.contains("INVALID KEY WITH SPACES"));
    }

    #[test]
    fn test_parse_env_file_lax_vs_strict() {
        // 测试宽松模式和严格模式的差异
        let content = "VALID_KEY=value
INVALID LINE
KEY_WITH-DASH=value";

        // 宽松模式：跳过无效行，继续解析
        let lax_result = parse_env_file(content);
        assert_eq!(lax_result.len(), 1); // 只有 VALID_KEY
        assert_eq!(lax_result.get("VALID_KEY"), Some(&"value".to_string()));

        // 严格模式：遇到无效行立即返回错误
        let strict_result = parse_env_file_strict(content);
        assert!(strict_result.is_err());
    }

    #[test]
    fn test_packycode_settings_structure() {
        // 验证 Packycode settings.json 的结构正确
        let settings_content = serde_json::json!({
            "security": {
                "auth": {
                    "selectedType": "gemini-api-key"
                }
            }
        });

        assert_eq!(
            settings_content["security"]["auth"]["selectedType"],
            "gemini-api-key"
        );
    }

    #[test]
    fn test_packycode_settings_merge() {
        // 测试合并逻辑：应该保留其他字段
        let mut existing_settings = serde_json::json!({
            "otherField": "should-be-kept",
            "security": {
                "otherSetting": "also-kept",
                "auth": {
                    "otherAuth": "preserved"
                }
            }
        });

        // 模拟更新 selectedType
        if let Some(obj) = existing_settings.as_object_mut() {
            let security = obj
                .entry("security")
                .or_insert_with(|| serde_json::json!({}));

            if let Some(security_obj) = security.as_object_mut() {
                let auth = security_obj
                    .entry("auth")
                    .or_insert_with(|| serde_json::json!({}));

                if let Some(auth_obj) = auth.as_object_mut() {
                    auth_obj.insert(
                        "selectedType".to_string(),
                        Value::String("gemini-api-key".to_string()),
                    );
                }
            }
        }

        // 验证所有字段都被保留
        assert_eq!(existing_settings["otherField"], "should-be-kept");
        assert_eq!(existing_settings["security"]["otherSetting"], "also-kept");
        assert_eq!(
            existing_settings["security"]["auth"]["otherAuth"],
            "preserved"
        );
        assert_eq!(
            existing_settings["security"]["auth"]["selectedType"],
            "gemini-api-key"
        );
    }

    #[test]
    fn test_google_oauth_settings_structure() {
        // 验证 Google OAuth settings.json 的结构正确
        let settings_content = serde_json::json!({
            "security": {
                "auth": {
                    "selectedType": "oauth-personal"
                }
            }
        });

        assert_eq!(
            settings_content["security"]["auth"]["selectedType"],
            "oauth-personal"
        );
    }

    #[test]
    fn test_validate_empty_env_for_oauth() {
        // 测试空 env（Google 官方 OAuth）可以通过基本验证
        let settings = serde_json::json!({
            "env": {}
        });

        assert!(validate_gemini_settings(&settings).is_ok());
        // 严格验证也应该通过（空 env 表示 OAuth）
        assert!(validate_gemini_settings_strict(&settings).is_ok());
    }

    #[test]
    fn test_validate_env_with_api_key() {
        // 测试有 API Key 的配置可以通过验证
        let settings = serde_json::json!({
            "env": {
                "GEMINI_API_KEY": "sk-test123",
                "GEMINI_MODEL": "gemini-3-pro-preview"
            }
        });

        assert!(validate_gemini_settings(&settings).is_ok());
        assert!(validate_gemini_settings_strict(&settings).is_ok());
    }

    #[test]
    fn test_validate_env_without_api_key_relaxed() {
        // 测试缺少 API Key 的非空配置在基本验证中可以通过（用户稍后填写）
        let settings = serde_json::json!({
            "env": {
                "GEMINI_MODEL": "gemini-3-pro-preview"
            }
        });

        // 基本验证应该通过（允许稍后填写 API Key）
        assert!(validate_gemini_settings(&settings).is_ok());
        // 严格验证应该失败（切换时要求完整配置）
        assert!(validate_gemini_settings_strict(&settings).is_err());
    }

    #[test]
    fn test_validate_invalid_env_type() {
        // 测试 env 不是对象时会失败
        let settings = serde_json::json!({
            "env": "invalid_string"
        });

        assert!(validate_gemini_settings(&settings).is_err());
    }
}
