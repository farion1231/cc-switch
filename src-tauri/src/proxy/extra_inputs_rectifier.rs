//! Extra Inputs 整流器
//!
//! 当上游 API 返回 "Extra inputs are not permitted" 错误时，自动从错误消息中
//! 提取不支持的字段名，缓存到内存中（1 小时过期），并在后续请求中预过滤这些字段。

use super::types::RectifierConfig;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// 缓存条目过期时间：1 小时
const CACHE_TTL: Duration = Duration::from_secs(3600);

/// Anthropic 独有的顶层字段，作为兜底候选：当 200+error 触发整流但无法从错误消息中提取字段名时，
/// 检查请求体是否包含这些字段并尝试剥离。
pub const ANTHROPIC_ONLY_FIELDS: &[&str] =
    &["context_management", "anthropic_beta", "output_config"];

/// Extra Inputs 整流结果
#[derive(Debug, Clone, Default)]
pub struct ExtraInputsRectifyResult {
    /// 是否应用了整流
    pub applied: bool,
    /// 实际从 body 中移除的字段名列表
    pub removed_fields: Vec<String>,
}

/// 缓存条目
#[derive(Debug, Clone)]
struct CacheEntry {
    /// 写入时间
    created_at: Instant,
}

impl CacheEntry {
    fn new() -> Self {
        Self {
            created_at: Instant::now(),
        }
    }

    fn is_expired(&self) -> bool {
        self.created_at.elapsed() > CACHE_TTL
    }
}

/// Extra Inputs 字段缓存
///
/// 缓存 key: `"provider_id:field_name"`
/// 线程安全，支持并发读写。
#[derive(Debug, Clone)]
pub struct ExtraInputsCache {
    entries: Arc<RwLock<HashMap<String, CacheEntry>>>,
}

impl ExtraInputsCache {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 缓存 key 格式
    fn make_key(provider_id: &str, field_name: &str) -> String {
        format!("{provider_id}:{field_name}")
    }

    /// 批量记录不支持的字段
    pub async fn insert_many(&self, provider_id: &str, field_names: &[String]) {
        let mut entries = self.entries.write().await;
        for field_name in field_names {
            let key = Self::make_key(provider_id, field_name);
            entries.insert(key, CacheEntry::new());
        }
    }

    /// 获取某个 provider 所有未过期的不支持字段
    pub async fn get_blocked_fields(&self, provider_id: &str) -> Vec<String> {
        let prefix = format!("{provider_id}:");
        let entries = self.entries.read().await;
        let mut expired_keys = Vec::new();
        let mut fields = Vec::new();
        for (k, v) in entries.iter() {
            if k.starts_with(&prefix) {
                if v.is_expired() {
                    expired_keys.push(k.clone());
                } else {
                    fields.push(k[prefix.len()..].to_string());
                }
            }
        }
        if !expired_keys.is_empty() {
            drop(entries);
            let mut entries = self.entries.write().await;
            for key in &expired_keys {
                entries.remove(key);
            }
        }
        fields
    }
}

impl Default for ExtraInputsCache {
    fn default() -> Self {
        Self::new()
    }
}

/// 检测是否需要触发 extra inputs 整流器
///
/// 返回 `true` 当错误消息包含 "extra inputs are not permitted" 且配置开关开启。
/// 注意：已被 thinking_rectifier 匹配的 `signature` 相关错误在此处也会匹配，
/// 但调用顺序保证 thinking_rectifier 优先处理。
pub fn should_rectify_extra_inputs(error_message: Option<&str>, config: &RectifierConfig) -> bool {
    if !config.enabled {
        return false;
    }
    if !config.request_extra_inputs_strip {
        return false;
    }

    let Some(msg) = error_message else {
        return false;
    };

    msg.to_lowercase()
        .contains("extra inputs are not permitted")
}

/// 从错误消息中提取不支持的顶层字段名
///
/// 支持的错误格式：
/// - `"context_management: Extra inputs are not permitted"`
/// - `"messages.0.content.1.signature: Extra inputs are not permitted"`
///   → 提取顶层字段 `context_management`；路径式字段 `signature` 不提取为顶层（因为嵌套字段由其他整流器处理）
/// - 多个错误用换行或 JSON 嵌套时，尝试匹配所有出现
///
/// 只提取**顶层字段**（不含 `.` 的路径前缀），嵌套路径（如 `messages.0.xxx`）跳过。
pub fn extract_extra_input_fields(error_message: &str) -> Vec<String> {
    let lower = error_message.to_lowercase();
    let mut fields = Vec::new();

    // 匹配模式：`field_name: Extra inputs are not permitted`
    // 或嵌套 JSON 中的同一模式
    for line in lower.lines() {
        extract_from_line(line, &mut fields);
    }

    fields.sort();
    fields.dedup();
    fields
}

/// 从一行文本中提取 `field: Extra inputs are not permitted` 的字段名
fn extract_from_line(text: &str, fields: &mut Vec<String>) {
    let pattern = "extra inputs are not permitted";
    let mut search_from = 0;

    while let Some(pos) = text[search_from..].find(pattern) {
        let abs_pos = search_from + pos;

        // 向前查找 ": " 或 ":" 分隔符
        if abs_pos >= 2 {
            let before = &text[..abs_pos];
            // 去掉尾部的 ": " 或 ":"
            let before = before.trim_end_matches(": ").trim_end_matches(':');

            // 取最后一个字段路径（可能前面有其他文本）
            // 查找最近的分隔符（空格、逗号、引号等）
            let field_path = before
                .rsplit([' ', ',', '"', '\'', '{'])
                .next()
                .unwrap_or("")
                .trim();

            if !field_path.is_empty() {
                // 只取顶层字段：不含 '.' 的路径视为顶层
                if !field_path.contains('.') {
                    // 排除纯数字（不是有效字段名）
                    if !field_path.chars().all(|c| c.is_ascii_digit()) {
                        fields.push(field_path.to_string());
                    }
                } else {
                    // 嵌套路径如 "messages.0.content.1.xxx"：提取第一段
                    // 仅当第一段不是 "messages" 等已知数组路径时才提取
                    let first_segment = field_path.split('.').next().unwrap_or("");
                    // 排除已知的消息/内容路径前缀，这些由其他整流器处理
                    if !matches!(
                        first_segment,
                        "messages" | "content" | "tools" | "tool_choice" | "metadata"
                    ) && !first_segment.is_empty()
                        && !first_segment.chars().all(|c| c.is_ascii_digit())
                    {
                        fields.push(first_segment.to_string());
                    }
                }
            }
        }

        search_from = abs_pos + pattern.len();
    }
}

/// 从请求体中移除指定的顶层字段
///
/// 只操作 JSON 对象的顶层 key，不做递归处理。
pub fn strip_fields(body: &mut Value, fields: &[String]) -> ExtraInputsRectifyResult {
    let mut result = ExtraInputsRectifyResult {
        applied: false,
        removed_fields: Vec::new(),
    };

    let Some(obj) = body.as_object_mut() else {
        return result;
    };

    for field in fields {
        if obj.remove(field).is_some() {
            result.removed_fields.push(field.clone());
            result.applied = true;
        }
    }

    result
}

/// 根据缓存预过滤请求体（发送前调用）
///
/// 从缓存中查找该 provider 已知不支持的字段，从 body 顶层移除。
pub fn pre_filter_from_cache(
    body: &mut Value,
    blocked_fields: &[String],
) -> ExtraInputsRectifyResult {
    let mut result = ExtraInputsRectifyResult::default();

    let Some(obj) = body.as_object_mut() else {
        return result;
    };

    for field in blocked_fields {
        if obj.remove(field).is_some() {
            result.removed_fields.push(field.clone());
            result.applied = true;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn enabled_config() -> RectifierConfig {
        RectifierConfig {
            enabled: true,
            request_thinking_signature: true,
            request_thinking_budget: true,
            request_extra_inputs_strip: true,
        }
    }

    fn disabled_config() -> RectifierConfig {
        RectifierConfig {
            enabled: true,
            request_thinking_signature: true,
            request_thinking_budget: true,
            request_extra_inputs_strip: false,
        }
    }

    fn master_disabled_config() -> RectifierConfig {
        RectifierConfig {
            enabled: false,
            request_thinking_signature: true,
            request_thinking_budget: true,
            request_extra_inputs_strip: true,
        }
    }

    // ==================== should_rectify_extra_inputs 测试 ====================

    #[test]
    fn test_detect_extra_inputs_error() {
        assert!(should_rectify_extra_inputs(
            Some("context_management: Extra inputs are not permitted"),
            &enabled_config()
        ));
    }

    #[test]
    fn test_detect_extra_inputs_nested_json() {
        let nested = r#"{"error":{"type":"invalid_request_error","message":"context_management: Extra inputs are not permitted"}}"#;
        assert!(should_rectify_extra_inputs(Some(nested), &enabled_config()));
    }

    #[test]
    fn test_no_trigger_for_unrelated_error() {
        assert!(!should_rectify_extra_inputs(
            Some("Request timeout"),
            &enabled_config()
        ));
        assert!(!should_rectify_extra_inputs(None, &enabled_config()));
    }

    #[test]
    fn test_disabled_config() {
        assert!(!should_rectify_extra_inputs(
            Some("context_management: Extra inputs are not permitted"),
            &disabled_config()
        ));
    }

    #[test]
    fn test_master_disabled() {
        assert!(!should_rectify_extra_inputs(
            Some("context_management: Extra inputs are not permitted"),
            &master_disabled_config()
        ));
    }

    // ==================== extract_extra_input_fields 测试 ====================

    #[test]
    fn test_extract_simple_field() {
        let fields =
            extract_extra_input_fields("context_management: Extra inputs are not permitted");
        assert_eq!(fields, vec!["context_management"]);
    }

    #[test]
    fn test_extract_from_nested_json() {
        let msg = r#"{"error":{"message":"context_management: Extra inputs are not permitted"}}"#;
        let fields = extract_extra_input_fields(msg);
        assert_eq!(fields, vec!["context_management"]);
    }

    #[test]
    fn test_extract_ignores_nested_path() {
        // messages.0.content.1.signature 这类嵌套路径不应提取为顶层字段
        let fields = extract_extra_input_fields(
            "messages.0.content.1.signature: Extra inputs are not permitted",
        );
        assert!(fields.is_empty());
    }

    #[test]
    fn test_extract_multiple_fields() {
        let msg = "foo: Extra inputs are not permitted, bar: Extra inputs are not permitted";
        let fields = extract_extra_input_fields(msg);
        assert!(fields.contains(&"foo".to_string()));
        assert!(fields.contains(&"bar".to_string()));
    }

    #[test]
    fn test_extract_no_match() {
        let fields = extract_extra_input_fields("Request timeout");
        assert!(fields.is_empty());
    }

    #[test]
    fn test_extract_non_messages_dotted_path() {
        // custom_config.option: Extra inputs → 提取 custom_config
        let fields =
            extract_extra_input_fields("custom_config.option: Extra inputs are not permitted");
        assert_eq!(fields, vec!["custom_config"]);
    }

    // ==================== strip_fields 测试 ====================

    #[test]
    fn test_strip_single_field() {
        let mut body = json!({
            "model": "claude-test",
            "context_management": { "enabled": true },
            "messages": []
        });

        let result = strip_fields(&mut body, &["context_management".to_string()]);

        assert!(result.applied);
        assert_eq!(result.removed_fields, vec!["context_management"]);
        assert!(body.get("context_management").is_none());
        assert!(body.get("model").is_some());
        assert!(body.get("messages").is_some());
    }

    #[test]
    fn test_strip_multiple_fields() {
        let mut body = json!({
            "model": "claude-test",
            "foo": 1,
            "bar": 2,
            "baz": 3
        });

        let result = strip_fields(&mut body, &["foo".to_string(), "bar".to_string()]);

        assert!(result.applied);
        assert_eq!(result.removed_fields.len(), 2);
        assert!(body.get("foo").is_none());
        assert!(body.get("bar").is_none());
        assert!(body.get("baz").is_some());
    }

    #[test]
    fn test_strip_nonexistent_field() {
        let mut body = json!({
            "model": "claude-test",
            "messages": []
        });

        let result = strip_fields(&mut body, &["nonexistent".to_string()]);

        assert!(!result.applied);
        assert!(result.removed_fields.is_empty());
    }

    // ==================== pre_filter_from_cache 测试 ====================

    #[test]
    fn test_pre_filter_removes_blocked_fields() {
        let mut body = json!({
            "model": "claude-test",
            "context_management": { "enabled": true },
            "messages": [{"role": "user", "content": "hello"}]
        });

        let result = pre_filter_from_cache(&mut body, &["context_management".to_string()]);

        assert_eq!(result.removed_fields, vec!["context_management"]);
        assert!(body.get("context_management").is_none());
        assert!(body.get("model").is_some());
    }

    #[test]
    fn test_pre_filter_no_blocked_fields() {
        let mut body = json!({
            "model": "claude-test",
            "messages": []
        });

        let result = pre_filter_from_cache(&mut body, &[]);

        assert!(result.removed_fields.is_empty());
    }

    // ==================== ExtraInputsCache 测试 ====================

    #[tokio::test]
    async fn test_cache_insert_and_get() {
        let cache = ExtraInputsCache::new();
        cache
            .insert_many(
                "provider_1",
                &["context_management".to_string(), "some_field".to_string()],
            )
            .await;
        cache
            .insert_many("provider_2", &["other_field".to_string()])
            .await;

        let fields = cache.get_blocked_fields("provider_1").await;
        assert!(fields.contains(&"context_management".to_string()));
        assert!(fields.contains(&"some_field".to_string()));
        assert!(!fields.contains(&"other_field".to_string()));

        let fields2 = cache.get_blocked_fields("provider_2").await;
        assert_eq!(fields2, vec!["other_field"]);
    }

    #[tokio::test]
    async fn test_cache_insert_many() {
        let cache = ExtraInputsCache::new();
        cache
            .insert_many("p1", &["field_a".to_string(), "field_b".to_string()])
            .await;

        let fields = cache.get_blocked_fields("p1").await;
        assert!(fields.contains(&"field_a".to_string()));
        assert!(fields.contains(&"field_b".to_string()));
    }

    #[tokio::test]
    async fn test_cache_empty_provider() {
        let cache = ExtraInputsCache::new();
        let fields = cache.get_blocked_fields("nonexistent").await;
        assert!(fields.is_empty());
    }
}
