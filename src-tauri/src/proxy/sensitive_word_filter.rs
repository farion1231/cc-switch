//! 敏感词过滤模块
//!
//! 从本地 txt 文件加载敏感词列表，在请求转发前过滤请求体中的敏感词。
//!
//! ## 功能
//! - 从 txt 文件加载敏感词（每行一个，去空行和首尾空白）
//! - 遍历 Claude / Codex / Gemini 常见文本字段，将敏感词替换为 `***`
//! - 支持 content/input/contents/system/instructions 等字符串、数组（多模态）和对象格式
//! - 使用 Aho-Corasick 自动机进行多模式匹配，支持最长匹配优先
//! - 自动机缓存在全局缓存中，避免每次请求重新构建

use aho_corasick::AhoCorasick;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::SystemTime;

/// 缓存的敏感词列表及其来源文件信息
struct WordCache {
    file_path: String,
    words: Vec<String>,
    matcher: AhoCorasick,
    modified_time: Option<SystemTime>,
}

/// 全局敏感词缓存（跨请求共享，避免每次都读文件和重建自动机）
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
    let content = content.trim_start_matches('\u{feff}');

    let words: Vec<String> = content
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    if words.is_empty() {
        return Err("敏感词文件为空".to_string());
    }

    // 构建 Aho-Corasick 自动机，使用最长匹配模式
    let matcher = AhoCorasick::builder()
        .match_kind(aho_corasick::MatchKind::LeftmostLongest)
        .build(&words)
        .map_err(|e| format!("构建敏感词匹配器失败: {e}"))?;

    Ok(WordCache {
        file_path: file_path.to_string(),
        words,
        matcher,
        modified_time,
    })
}

/// 将指定文件重新读取并写入全局缓存。
pub fn reload_sensitive_words(file_path: &str) -> Result<Vec<String>, String> {
    let cache_entry = read_sensitive_words(file_path)?;

    let words = cache_entry.words.clone();
    {
        let mut cache = WORD_CACHE.lock().unwrap();
        *cache = Some(cache_entry);
    }

    log::info!(
        "[SensitiveWordFilter] 已重新加载 {} 个敏感词",
        words.len()
    );

    Ok(words)
}

/// 获取当前缓存的敏感词列表，仅在缓存来源与当前配置文件一致时返回。
#[allow(dead_code)]
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

/// 过滤请求体中的敏感词（使用缓存的匹配器）
///
/// 遍历 Claude / Codex / Gemini 常见文本字段，将文本中的敏感词替换为 `***`。
/// 使用全局缓存的 Aho-Corasick 自动机，避免每次请求重新构建。
///
/// 返回被过滤的敏感词数量。
pub fn filter_sensitive_words_with_cache(body: &mut Value, file_path: &str) -> u32 {
    let matcher = {
        let cache = WORD_CACHE.lock().unwrap();
        match cache.as_ref() {
            Some(cached) if cached.file_path == file_path => cached.matcher.clone(),
            _ => {
                log::warn!(
                    "[SensitiveWordFilter] 缓存未命中或文件路径不匹配，跳过过滤: {}",
                    file_path
                );
                return 0;
            }
        }
    };

    let mut filter_count = 0u32;

    for key in ROOT_TEXTUAL_KEYS {
        if let Some(value) = body.get_mut(*key) {
            filter_count += filter_value(value, &matcher);
        }
    }

    if filter_count > 0 {
        log::info!(
            "[SensitiveWordFilter] 已过滤 {} 处敏感词",
            filter_count
        );
    }

    filter_count
}

/// 过滤请求体中的敏感词（传入敏感词列表，每次构建匹配器）
///
/// 遍历 Claude / Codex / Gemini 常见文本字段，将文本中的敏感词替换为 `***`。
///
/// 注意：此函数每次调用都会构建新的 Aho-Corasick 自动机。
/// 如果需要高性能，建议使用 `reload_sensitive_words` 预加载并使用 `filter_sensitive_words_with_cache`。
#[allow(dead_code)]
pub fn filter_sensitive_words(body: &mut Value, words: &[String]) {
    if words.is_empty() {
        return;
    }

    // 构建匹配器
    let matcher = AhoCorasick::builder()
        .match_kind(aho_corasick::MatchKind::LeftmostLongest)
        .build(words);

    let matcher = match matcher {
        Ok(m) => m,
        Err(e) => {
            log::error!("[SensitiveWordFilter] 构建匹配器失败: {}", e);
            return;
        }
    };

    let mut filter_count = 0u32;

    for key in ROOT_TEXTUAL_KEYS {
        if let Some(value) = body.get_mut(*key) {
            filter_count += filter_value(value, &matcher);
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
fn filter_value(value: &mut Value, matcher: &AhoCorasick) -> u32 {
    match value {
        Value::String(text) => {
            let filtered = replace_sensitive_words(text, matcher);
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
                count += filter_value(item, matcher);
            }
            count
        }
        Value::Object(map) => {
            let mut count = 0u32;
            for key in NESTED_TEXTUAL_KEYS {
                if let Some(child) = map.get_mut(*key) {
                    count += filter_value(child, matcher);
                }
            }
            count
        }
        _ => 0,
    }
}

/// 替换文本中的敏感词为 `***`
///
/// 使用 Aho-Corasick 自动机进行多模式匹配，采用最长匹配优先策略。
/// 这确保了当敏感词有重叠时（如 "sk-" 和 "sk-live-abc"），
/// 总是优先替换更长的匹配，避免短前缀干扰长词匹配的问题。
fn replace_sensitive_words(text: &str, matcher: &AhoCorasick) -> String {
    // 使用 replace_all_with 为所有匹配使用相同的替换字符串
    let mut result = String::with_capacity(text.len());
    matcher.replace_all_with(text, &mut result, |_mat, _matched_text, dst| {
        dst.push_str("***");
        true // 继续处理下一个匹配
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn build_matcher(words: &[&str]) -> Option<AhoCorasick> {
        if words.is_empty() {
            return None;
        }
        Some(
            AhoCorasick::builder()
                .match_kind(aho_corasick::MatchKind::LeftmostLongest)
                .build(words)
                .unwrap(),
        )
    }

    #[test]
    fn test_replace_sensitive_words() {
        let matcher = build_matcher(&["密码", "secret"]).unwrap();
        assert_eq!(
            replace_sensitive_words("请输入密码登录", &matcher),
            "请输入***登录"
        );
        assert_eq!(
            replace_sensitive_words("this is a secret message", &matcher),
            "this is a *** message"
        );
    }

    #[test]
    fn test_replace_no_match() {
        let matcher = build_matcher(&["密码"]).unwrap();
        assert_eq!(replace_sensitive_words("hello world", &matcher), "hello world");
    }

    #[test]
    fn test_replace_empty_word_list() {
        // 空词列表返回 None，表示不需要替换
        let matcher = build_matcher(&[]);
        assert!(matcher.is_none());
    }

    #[test]
    fn test_longest_match_priority() {
        // 测试最长匹配优先：当有重叠词时，应优先匹配更长的词
        let matcher = build_matcher(&["sk-", "sk-live-abc123"]).unwrap();
        // sk-live-abc123 应该被完整替换，而不是先替换 sk- 变成 ***live-abc123
        assert_eq!(
            replace_sensitive_words("token: sk-live-abc123 here", &matcher),
            "token: *** here"
        );

        // 单独的 sk- 也能被匹配
        assert_eq!(
            replace_sensitive_words("prefix sk- test", &matcher),
            "prefix *** test"
        );
    }

    #[test]
    fn test_multiple_overlapping_words() {
        // 测试多个重叠词的情况
        let matcher = build_matcher(&["api", "api-key", "api-key-secret"]).unwrap();
        assert_eq!(
            replace_sensitive_words("value: api-key-secret", &matcher),
            "value: ***"
        );
        assert_eq!(
            replace_sensitive_words("value: api-key", &matcher),
            "value: ***"
        );
        assert_eq!(
            replace_sensitive_words("value: api", &matcher),
            "value: ***"
        );
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

    #[test]
    fn test_read_sensitive_words_strips_utf8_bom() {
        let file_path = std::env::temp_dir().join(format!(
            "sensitive_words_bom_{}_{}.txt",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&file_path, "\u{feff}first-word\nsecond-word\n").unwrap();

        let cache = read_sensitive_words(file_path.to_str().unwrap()).unwrap();

        assert_eq!(
            cache.words,
            vec!["first-word".to_string(), "second-word".to_string()]
        );

        fs::remove_file(file_path).unwrap();
    }
}
