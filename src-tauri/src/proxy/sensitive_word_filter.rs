//! 敏感词过滤模块
//!
//! 从本地 txt 文件加载敏感词列表，在请求转发前过滤请求体中的敏感词。
//!
//! ## 功能
//! - 从 txt 文件加载敏感词（每行一个，去空行和首尾空白）
//! - 遍历 Claude / Codex / Gemini 常见文本字段，将敏感词替换为 `***`
//! - 支持 content/input/contents/system/instructions 等字符串、数组（多模态）和对象格式

use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::SystemTime;

/// 缓存的敏感词列表及其来源文件信息
#[derive(Clone)]
struct WordCache {
    file_path: String,
    words: Vec<String>,
    modified_time: Option<SystemTime>,
}

/// 全局敏感词缓存（跨请求共享，避免每次都读文件）
static WORD_CACHE: Mutex<Option<WordCache>> = Mutex::new(None);

/// 从 txt 文件加载敏感词列表
///
/// 每行一个敏感词，自动去除空行和首尾空白。
fn read_sensitive_words(file_path: &str) -> Result<WordCache, String> {
    let path = Path::new(file_path);
    if !path.exists() {
        return Err(format!("敏感词文件不存在: {file_path}"));
    }

    let modified_time = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok();

    // 读取文件
    let content =
        fs::read_to_string(path).map_err(|e| format!("读取敏感词文件失败: {e}"))?;

    let words: Vec<String> = content
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    if words.is_empty() {
        return Err("敏感词文件为空".to_string());
    }

    Ok(WordCache {
        file_path: file_path.to_string(),
        words,
        modified_time,
    })
}

/// 将指定文件重新读取并写入全局缓存。
pub fn reload_sensitive_words(file_path: &str) -> Result<Vec<String>, String> {
    let cache_entry = read_sensitive_words(file_path)?;

    {
        let mut cache = WORD_CACHE.lock().unwrap();
        *cache = Some(cache_entry.clone());
    }

    log::info!(
        "[SensitiveWordFilter] 已重新加载 {} 个敏感词",
        cache_entry.words.len()
    );

    Ok(cache_entry.words)
}

/// 获取当前缓存的敏感词列表，仅在缓存来源与当前配置文件一致时返回。
pub fn get_cached_sensitive_words(file_path: &str) -> Option<Vec<String>> {
    let cache = WORD_CACHE.lock().unwrap();
    cache.as_ref().and_then(|cached| {
        if cached.file_path == file_path {
            Some(cached.words.clone())
        } else {
            None
        }
    })
}

/// 获取当前缓存详情。
pub fn get_sensitive_word_cache() -> Option<(String, Vec<String>, Option<SystemTime>)> {
    let cache = WORD_CACHE.lock().unwrap();
    cache.as_ref().map(|cached| {
        (
            cached.file_path.clone(),
            cached.words.clone(),
            cached.modified_time,
        )
    })
}

const ROOT_TEXTUAL_KEYS: &[&str] = &[
    "system",
    "messages",
    "input",
    "instructions",
    "contents",
    "system_instruction",
];

const NESTED_TEXTUAL_KEYS: &[&str] = &[
    "content",
    "contents",
    "parts",
    "text",
    "input_text",
    "output_text",
    "instructions",
];

/// 过滤请求体中的敏感词
///
/// 遍历 Claude / Codex / Gemini 常见文本字段，将文本中的敏感词替换为 `***`。
pub fn filter_sensitive_words(body: &mut Value, words: &[String]) {
    if words.is_empty() {
        return;
    }

    let word_set: HashSet<&str> = words.iter().map(|s| s.as_str()).collect();
    let mut filter_count = 0u32;

    for key in ROOT_TEXTUAL_KEYS {
        if let Some(value) = body.get_mut(*key) {
            filter_count += filter_value(value, &word_set);
        }
    }

    if filter_count > 0 {
        log::info!(
            "[SensitiveWordFilter] 已过滤 {} 处敏感词",
            filter_count
        );
    }
}

/// 过滤单个 JSON value 中的敏感词
fn filter_value(value: &mut Value, word_set: &HashSet<&str>) -> u32 {
    match value {
        Value::String(text) => {
            let filtered = replace_sensitive_words(text, word_set);
            if filtered != *text {
                *value = Value::String(filtered);
                1
            } else {
                0
            }
        }
        Value::Array(arr) => {
            let mut count = 0u32;
            for item in arr.iter_mut() {
                count += filter_value(item, word_set);
            }
            count
        }
        Value::Object(map) => {
            let mut count = 0u32;
            for key in NESTED_TEXTUAL_KEYS {
                if let Some(child) = map.get_mut(*key) {
                    count += filter_value(child, word_set);
                }
            }
            count
        }
        _ => 0,
    }
}

/// 替换文本中的敏感词为 `***`
fn replace_sensitive_words(text: &str, word_set: &HashSet<&str>) -> String {
    let mut result = text.to_string();
    for word in word_set {
        if word.is_empty() {
            continue;
        }
        result = result.replace(word, "***");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_replace_sensitive_words() {
        let words: HashSet<&str> = ["密码", "secret"].iter().copied().collect();
        assert_eq!(
            replace_sensitive_words("请输入密码登录", &words),
            "请输入***登录"
        );
        assert_eq!(
            replace_sensitive_words("this is a secret message", &words),
            "this is a *** message"
        );
    }

    #[test]
    fn test_replace_no_match() {
        let words: HashSet<&str> = ["密码"].iter().copied().collect();
        assert_eq!(replace_sensitive_words("hello world", &words), "hello world");
    }

    #[test]
    fn test_replace_empty_word_list() {
        let words: HashSet<&str> = HashSet::new();
        assert_eq!(replace_sensitive_words("hello", &words), "hello");
    }

    #[test]
    fn test_filter_string_content() {
        let mut body = json!({
            "model": "claude-3",
            "messages": [
                {"role": "user", "content": "请告诉我密码是什么"}
            ]
        });

        let words = vec!["密码".to_string()];
        filter_sensitive_words(&mut body, &words);

        let content = body["messages"][0]["content"].as_str().unwrap();
        assert_eq!(content, "请告诉我***是什么");
    }

    #[test]
    fn test_filter_multimodal_content() {
        let mut body = json!({
            "model": "claude-3",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "请输入密码"},
                        {"type": "image", "source": {"type": "base64", "data": "abc"}}
                    ]
                }
            ]
        });

        let words = vec!["密码".to_string()];
        filter_sensitive_words(&mut body, &words);

        let text = body["messages"][0]["content"][0]["text"].as_str().unwrap();
        assert_eq!(text, "请输入***");
    }

    #[test]
    fn test_filter_system_field() {
        let mut body = json!({
            "model": "claude-3",
            "system": "你是一个安全的助手，不要泄露密码",
            "messages": []
        });

        let words = vec!["密码".to_string()];
        filter_sensitive_words(&mut body, &words);

        let system = body["system"].as_str().unwrap();
        assert_eq!(system, "你是一个安全的助手，不要泄露***");
    }

    #[test]
    fn test_filter_empty_words() {
        let mut body = json!({
            "model": "claude-3",
            "messages": [{"role": "user", "content": "hello"}]
        });

        let words: Vec<String> = vec![];
        filter_sensitive_words(&mut body, &words);

        assert_eq!(
            body["messages"][0]["content"].as_str().unwrap(),
            "hello"
        );
    }

    #[test]
    fn test_filter_multiple_occurrences() {
        let mut body = json!({
            "model": "claude-3",
            "messages": [
                {"role": "user", "content": "密码很重要，请保护密码"}
            ]
        });

        let words = vec!["密码".to_string()];
        filter_sensitive_words(&mut body, &words);

        let content = body["messages"][0]["content"].as_str().unwrap();
        assert_eq!(content, "***很重要，请保护***");
    }

    #[test]
    fn test_filter_multiple_words() {
        let mut body = json!({
            "model": "claude-3",
            "messages": [
                {"role": "user", "content": "请输入密码和secret"}
            ]
        });

        let words = vec!["密码".to_string(), "secret".to_string()];
        filter_sensitive_words(&mut body, &words);

        let content = body["messages"][0]["content"].as_str().unwrap();
        assert_eq!(content, "请输入***和***");
    }

    #[test]
    fn test_filter_codex_responses_input_string() {
        let mut body = json!({
            "model": "gpt-5-codex",
            "input": "请输出密码和secret"
        });

        let words = vec!["密码".to_string(), "secret".to_string()];
        filter_sensitive_words(&mut body, &words);

        assert_eq!(body["input"].as_str().unwrap(), "请输出***和***");
    }

    #[test]
    fn test_filter_codex_responses_input_items() {
        let mut body = json!({
            "model": "gpt-5-codex",
            "input": [
                {
                    "role": "user",
                    "content": [
                        {"type": "input_text", "text": "我的密码是 123456"},
                        {"type": "input_image", "image_url": "https://example.com/a.png"}
                    ]
                }
            ],
            "instructions": "不要泄露密码"
        });

        let words = vec!["密码".to_string()];
        filter_sensitive_words(&mut body, &words);

        assert_eq!(
            body["input"][0]["content"][0]["text"].as_str().unwrap(),
            "我的***是 123456"
        );
        assert_eq!(body["instructions"].as_str().unwrap(), "不要泄露***");
    }

    #[test]
    fn test_filter_gemini_contents_and_system_instruction() {
        let mut body = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [
                        {"text": "这里有密码"},
                        {"inlineData": {"mimeType": "image/png", "data": "abc"}}
                    ]
                }
            ],
            "system_instruction": {
                "parts": [
                    {"text": "系统里也有密码"}
                ]
            }
        });

        let words = vec!["密码".to_string()];
        filter_sensitive_words(&mut body, &words);

        assert_eq!(
            body["contents"][0]["parts"][0]["text"].as_str().unwrap(),
            "这里有***"
        );
        assert_eq!(
            body["system_instruction"]["parts"][0]["text"].as_str().unwrap(),
            "系统里也有***"
        );
    }

    #[test]
    fn test_filter_does_not_touch_model_or_ids() {
        let mut body = json!({
            "model": "secret-model",
            "previous_response_id": "resp-secret-123",
            "input": "secret 在这里"
        });

        let words = vec!["secret".to_string()];
        filter_sensitive_words(&mut body, &words);

        assert_eq!(body["model"].as_str().unwrap(), "secret-model");
        assert_eq!(
            body["previous_response_id"].as_str().unwrap(),
            "resp-secret-123"
        );
        assert_eq!(body["input"].as_str().unwrap(), "*** 在这里");
    }
}
