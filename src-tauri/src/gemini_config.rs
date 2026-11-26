use crate::config::{get_home_dir, write_text_file};
use crate::error::AppError;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// 获取 Gemini 配置目录路径（支持设置覆盖）
pub fn get_gemini_dir() -> PathBuf {
    if let Some(custom) = crate::settings::get_override_dir(&crate::app_config::AppType::Gemini) {
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
pub fn parse_env_file(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();

    for line in content.lines() {
        let line = line.trim();

        // 跳过空行和注释
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Gemini CLI loads .env with dotenv, which reads `export KEY=VALUE` as `KEY=VALUE`
        let line = line.strip_prefix("export ").unwrap_or(line);

        // 解析 KEY=VALUE
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_string();
            let value = env_value(value).to_string();

            // 验证 key 是否有效（不为空，只包含字母、数字和下划线）
            if !key.is_empty() && key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                map.insert(key, value);
            }
        }
    }

    map
}

/// The value dotenv reads from the text after `=`: an unquoted value ends at `#`, and a value in
/// `"`, `'` or `` ` `` loses the quotes (a comment may follow it).
///
/// CC Switch writes values back as `KEY=value`, so a quoted value that dotenv would read
/// differently without its quotes stays as written.
fn env_value(raw: &str) -> &str {
    let raw = raw.trim();
    let Some(quote) = raw.chars().next().filter(|c| matches!(c, '"' | '\'' | '`')) else {
        return raw.split('#').next().unwrap_or_default().trim_end();
    };
    let Some((inner, rest)) = raw[1..].split_once(quote) else {
        return raw;
    };
    let rest = rest.trim_start();
    let closed = rest.is_empty() || rest.starts_with('#');
    let needs_quotes = inner.contains('#')
        || inner != inner.trim()
        || inner.starts_with(['"', '\'', '`'])
        // `\"` may be an escaped quote, and dotenv expands `\n` and `\r` in double quotes
        || inner.ends_with('\\')
        || (quote == '"' && (inner.contains("\\n") || inner.contains("\\r")));
    if closed && !needs_quotes {
        inner
    } else {
        raw
    }
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

/// 从 .env 原文中按「键名 + 值」双匹配删除若干行，其余内容逐字保留
///
/// 不走 `parse_env_file` → `serialize_env_file` 的往返：那对函数会丢掉注释、空行、
/// 无法识别的行和重复定义，并按键名重排整个文件。全量投影时这无所谓（本来就要重写
/// 整份），但用来做**定向**清理就等于顺手把用户手写的东西一起删了。
///
/// 按值匹配而非按键名：只清掉扩散出去的那一份，用户自己写的同名不同值的行保留。
/// 同一个键有重复定义时也只删命中的那条，被它遮住的上一条会重新生效——这正是想要的
/// 结果，因为遮住它的恰恰是泄漏值。
///
/// 返回 `None` 表示没有任何一行命中，调用方据此跳过写盘。
pub fn remove_env_entries_preserving_layout(
    content: &str,
    doomed: &HashMap<String, String>,
) -> Option<String> {
    let mut removed = false;
    let mut kept: Vec<&str> = Vec::new();

    for line in content.split('\n') {
        let trimmed = line.trim();
        // 与 parse_env_file 同一口径：dotenv 把 `export KEY=VALUE` 读作 `KEY=VALUE`，
        // 漏掉前缀会让带 export 的泄漏行永远匹配不上、清不掉
        let entry = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        let hit = !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && entry.split_once('=').is_some_and(|(key, value)| {
                doomed
                    .get(key.trim())
                    .is_some_and(|doomed_value| doomed_value == value.trim())
            });

        if hit {
            removed = true;
        } else {
            kept.push(line);
        }
    }

    removed.then(|| kept.join("\n"))
}

/// 从 `~/.gemini/.env` 中定向删除「键=值」完全匹配的行，返回是否真的改了文件
pub fn remove_gemini_env_entries(doomed: &HashMap<String, String>) -> Result<bool, AppError> {
    let path = get_gemini_env_path();
    if !path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    match remove_env_entries_preserving_layout(&content, doomed) {
        Some(cleaned) => {
            write_gemini_env_text_atomic(&cleaned)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// 写入 Gemini .env 文件（原子操作，内容逐字落盘）
///
/// 与 `write_gemini_env_atomic` 共用目录/文件权限处理，区别只在于内容不经
/// `serialize_env_file` 归一化——供保序的定向删除使用。
pub fn write_gemini_env_text_atomic(content: &str) -> Result<(), AppError> {
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

    write_text_file(&path, content)?;

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

    // 如果 env 不为空，要有凭据：GEMINI_API_KEY，或者 Vertex 的 GOOGLE_API_KEY /
    // GOOGLE_GENAI_USE_VERTEXAI（服务账号凭据走 GOOGLE_APPLICATION_CREDENTIALS 或 ADC）。
    if ![
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "GOOGLE_GENAI_USE_VERTEXAI",
    ]
    .iter()
    .any(|key| env_map.contains_key(*key))
    {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_env_file() {
        let content = r#"
# Comment line
GOOGLE_GEMINI_BASE_URL=https://example.com
GEMINI_API_KEY=sk-test123
GEMINI_MODEL=gemini-3.5-flash

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
            Some(&"gemini-3.5-flash".to_string())
        );
    }

    #[test]
    fn test_parse_env_file_reads_export_lines() {
        let content = r#"# Gemini CLI
export GEMINI_API_KEY=AIza-user-key
export GEMINI_MODEL=gemini-2.5-pro
"#;

        let map = parse_env_file(content);

        assert_eq!(
            map.get("GEMINI_API_KEY"),
            Some(&"AIza-user-key".to_string())
        );
        assert_eq!(map.get("GEMINI_MODEL"), Some(&"gemini-2.5-pro".to_string()));
    }

    #[test]
    fn test_parse_env_file_strips_quotes_and_inline_comments() {
        let content = r#"GOOGLE_CLOUD_PROJECT="your-project-id"
export GEMINI_API_KEY='AIza-user-key'
GOOGLE_GEMINI_BASE_URL=`https://relay.example`
GEMINI_MODEL=gemini-2.5-pro # default model
GOOGLE_CLOUD_LOCATION="us-central1" # region
GEMINI_SANDBOX=""
"#;

        let map = parse_env_file(content);

        for (key, value) in [
            ("GOOGLE_CLOUD_PROJECT", "your-project-id"),
            ("GEMINI_API_KEY", "AIza-user-key"),
            ("GOOGLE_GEMINI_BASE_URL", "https://relay.example"),
            ("GEMINI_MODEL", "gemini-2.5-pro"),
            ("GOOGLE_CLOUD_LOCATION", "us-central1"),
            ("GEMINI_SANDBOX", ""),
        ] {
            assert_eq!(map.get(key).map(String::as_str), Some(value), "{key}");
        }
    }

    #[test]
    fn test_parse_env_file_keeps_values_that_need_their_quotes() {
        // Values are written back as `KEY=value`, so one that dotenv would read
        // differently without its quotes stays as written.
        for value in [
            r#""a#b""#,
            r#"" padded ""#,
            r#""line\nbreak""#,
            r#""'x'""#,
            r#""a\" # x""#,
            r#""a" b"#,
            r#""unterminated"#,
            "'it''s'",
        ] {
            let map = parse_env_file(&format!("GEMINI_CLI_CUSTOM_HEADERS={value}\n"));
            assert_eq!(
                map.get("GEMINI_CLI_CUSTOM_HEADERS").map(String::as_str),
                Some(value)
            );
        }
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
    fn test_parse_env_file_skips_invalid_lines() {
        let content = "VALID_KEY=value
INVALID LINE
KEY_WITH-DASH=value";

        // 宽松模式：跳过无效行，继续解析
        let lax_result = parse_env_file(content);
        assert_eq!(lax_result.len(), 1); // 只有 VALID_KEY
        assert_eq!(lax_result.get("VALID_KEY"), Some(&"value".to_string()));
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
                "GEMINI_MODEL": "gemini-3.5-flash"
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
                "GEMINI_MODEL": "gemini-3.5-flash"
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

    #[test]
    fn test_remove_env_entries_matches_export_prefixed_lines() {
        // dotenv（与 parse_env_file 一致）把 `export KEY=VALUE` 读作 KEY=VALUE；
        // 定向清理必须同样剥掉前缀，否则 export 形式的泄漏行永远清不掉
        let content = "# comment\nexport GEMINI_API_KEY=leaked\nGEMINI_API_KEY=keep_me\n";
        let mut doomed = HashMap::new();
        doomed.insert("GEMINI_API_KEY".to_string(), "leaked".to_string());

        let result = remove_env_entries_preserving_layout(content, &doomed).unwrap();

        assert_eq!(
            result, "# comment\nGEMINI_API_KEY=keep_me\n",
            "export 行应命中并删除，无前缀的同名不同值行保留"
        );
    }

    #[test]
    fn test_remove_env_entries_export_without_hit_is_none() {
        let content = "export GEMINI_API_KEY=other\n";
        let mut doomed = HashMap::new();
        doomed.insert("GEMINI_API_KEY".to_string(), "leaked".to_string());

        assert!(remove_env_entries_preserving_layout(content, &doomed).is_none());
    }
}
