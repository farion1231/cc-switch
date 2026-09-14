//! Gemini CLI 会话文件读取（兼容旧版单对象 JSON 与新版增量 JSONL）
//!
//! Gemini CLI 0.46 起把会话写成追加式 JSONL（`session-<id>.jsonl`），一个文件
//! 由三类行组成：
//!
//! ```text
//! {"sessionId":…,"projectHash":…,"startTime":…,"lastUpdated":…,"kind":"main"}
//! {"$set":{"messages":[…],"lastUpdated":…}}
//! {"id":…,"type":"gemini","model":…,"tokens":{…},"toolCalls":[…]}
//! ```
//!
//! 语义上有两处容易踩坑，都会直接影响 token 统计的准确性：
//!
//! - `$set.messages` 是**全量替换**而非追加。会话被 resume 或压缩后，整段历史
//!   会以快照形式重新写入，跳过这类行会丢掉快照里的全部消息。
//! - 独立消息行按 `id` **改写**已有消息：Gemini CLI 先写一条不带 `toolCalls`
//!   的消息，拿到工具调用后再用同一个 `id` 重写一次。按追加处理会让同一条消息
//!   出现两次。
//!
//! 更早的版本把整个会话写成单个 JSON 对象（顶层 `messages` 数组），这里一并兼容。

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

/// Gemini CLI 注入的会话引导块前缀，不作为会话标题
const SESSION_CONTEXT_PREFIX: &str = "<session_context>";

/// 一个 Gemini 会话文件回放后的最终状态
#[derive(Debug, Default)]
pub struct GeminiSessionFile {
    pub session_id: Option<String>,
    pub start_time: Option<Value>,
    pub last_updated: Option<Value>,
    pub messages: Vec<Value>,
}

/// 是否是 Gemini 会话文件的扩展名（旧版 `.json` / 新版 `.jsonl`）
pub fn has_session_extension(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("json") | Some("jsonl")
    )
}

/// 读取并回放一个会话文件
pub fn load(path: &Path) -> Result<GeminiSessionFile, String> {
    let data = std::fs::read_to_string(path).map_err(|e| format!("Failed to read session: {e}"))?;
    Ok(parse(&data))
}

/// 回放会话文件内容，得到消息列表的最终状态。
///
/// 先整体按单个 JSON 对象解析（旧版格式，可能是缩进过的多行 JSON）；失败则
/// 按 JSONL 逐行回放。只有一行的新版文件两条路径结果一致。
pub fn parse(data: &str) -> GeminiSessionFile {
    let mut session = GeminiSessionFile::default();
    let mut index: HashMap<String, usize> = HashMap::new();

    match serde_json::from_str::<Value>(data) {
        Ok(value) if value.is_object() => session.apply(value, &mut index),
        _ => {
            for line in data.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Ok(value) = serde_json::from_str::<Value>(line) {
                    session.apply(value, &mut index);
                }
            }
        }
    }

    session
}

impl GeminiSessionFile {
    /// 取第一条真实用户消息的文本，用作会话标题。
    ///
    /// Gemini CLI 每个会话的首条 user 消息都是 `<session_context>` 引导块（注入
    /// 日期、系统、工作区目录树），拿它当标题会让所有会话长得一模一样，故跳过。
    pub fn first_user_text(&self) -> Option<String> {
        self.messages
            .iter()
            .filter(|m| m.get("type").and_then(Value::as_str) == Some("user"))
            .filter_map(message_text)
            .map(|text| text.trim().to_string())
            .find(|text| !text.is_empty() && !text.starts_with(SESSION_CONTEXT_PREFIX))
    }

    fn apply(&mut self, value: Value, index: &mut HashMap<String, usize>) {
        if let Some(patch) = value.get("$set") {
            if let Some(fields) = patch.as_object() {
                for (key, field) in fields {
                    match key.as_str() {
                        "messages" => {
                            if let Some(list) = field.as_array() {
                                self.replace_messages(list.clone(), index);
                            }
                        }
                        "sessionId" => {
                            if let Some(sid) = field.as_str() {
                                self.session_id = Some(sid.to_string());
                            }
                        }
                        "startTime" => self.start_time = Some(field.clone()),
                        "lastUpdated" => self.last_updated = Some(field.clone()),
                        _ => {}
                    }
                }
            }
            return;
        }

        // 没有 type 的记录是元数据：新版首行，或旧版的整个会话对象
        if value.get("type").is_none() {
            if let Some(sid) = value.get("sessionId").and_then(Value::as_str) {
                self.session_id = Some(sid.to_string());
            }
            if let Some(start) = value.get("startTime") {
                self.start_time = Some(start.clone());
            }
            if let Some(updated) = value.get("lastUpdated") {
                self.last_updated = Some(updated.clone());
            }
            if let Some(list) = value.get("messages").and_then(Value::as_array) {
                self.replace_messages(list.clone(), index);
            }
            return;
        }

        self.upsert_message(value, index);
    }

    fn replace_messages(&mut self, messages: Vec<Value>, index: &mut HashMap<String, usize>) {
        self.messages = messages;
        index.clear();
        for (pos, message) in self.messages.iter().enumerate() {
            if let Some(id) = message.get("id").and_then(Value::as_str) {
                index.insert(id.to_string(), pos);
            }
        }
    }

    fn upsert_message(&mut self, message: Value, index: &mut HashMap<String, usize>) {
        if let Some(id) = message.get("id").and_then(Value::as_str) {
            if let Some(&pos) = index.get(id) {
                self.messages[pos] = message;
                return;
            }
            index.insert(id.to_string(), self.messages.len());
        }
        self.messages.push(message);
    }
}

/// Gemini 的 `content` 可能是纯字符串，也可能是 `{text: …}` 数组
pub fn message_text(message: &Value) -> Option<String> {
    match message.get("content") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Array(items)) => Some(
            items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ids(session: &GeminiSessionFile) -> Vec<&str> {
        session
            .messages
            .iter()
            .filter_map(|m| m.get("id").and_then(Value::as_str))
            .collect()
    }

    #[test]
    fn has_session_extension_accepts_both_formats() {
        assert!(has_session_extension(Path::new("session-a.json")));
        assert!(has_session_extension(Path::new("session-a.jsonl")));
        assert!(!has_session_extension(Path::new("logs.txt")));
        assert!(!has_session_extension(Path::new("session-a")));
    }

    #[test]
    fn parses_legacy_single_object_json() {
        // 旧版：整个文件是一个缩进过的 JSON 对象
        let session = parse(
            r#"{
              "sessionId": "legacy-1",
              "startTime": "2026-03-06T10:17:58.000Z",
              "lastUpdated": "2026-03-06T10:20:00.000Z",
              "messages": [
                {"id": "m1", "type": "user", "content": "hello"},
                {"id": "m2", "type": "gemini", "content": "hi"}
              ]
            }"#,
        );

        assert_eq!(session.session_id.as_deref(), Some("legacy-1"));
        assert_eq!(ids(&session), vec!["m1", "m2"]);
        assert_eq!(
            session.last_updated,
            Some(json!("2026-03-06T10:20:00.000Z"))
        );
    }

    #[test]
    fn parses_incremental_jsonl() {
        let session = parse(concat!(
            r#"{"sessionId":"s-1","startTime":"2026-09-14T03:16:00.000Z","lastUpdated":"2026-09-14T03:16:00.000Z","kind":"main"}"#,
            "\n",
            r#"{"$set":{"messages":[{"id":"m1","type":"user","content":"hello"}],"lastUpdated":"2026-09-14T03:16:10.000Z"}}"#,
            "\n",
            r#"{"id":"m2","type":"gemini","content":"hi","tokens":{"input":10,"output":2}}"#,
            "\n",
            r#"{"$set":{"lastUpdated":"2026-09-14T03:16:20.000Z"}}"#,
            "\n",
        ));

        assert_eq!(session.session_id.as_deref(), Some("s-1"));
        assert_eq!(ids(&session), vec!["m1", "m2"]);
        assert_eq!(
            session.last_updated,
            Some(json!("2026-09-14T03:16:20.000Z"))
        );
    }

    #[test]
    fn set_messages_replaces_instead_of_appending() {
        // 会话被 resume/压缩后整段历史以快照重写，快照必须替换掉此前累积的消息，
        // 否则重叠部分会被重复计入 token 统计。
        let session = parse(concat!(
            r#"{"sessionId":"s-2"}"#,
            "\n",
            r#"{"id":"m1","type":"gemini","content":"first","tokens":{"input":10,"output":1}}"#,
            "\n",
            r#"{"$set":{"messages":[{"id":"m1","type":"gemini","content":"first","tokens":{"input":10,"output":1}},{"id":"m2","type":"gemini","content":"second","tokens":{"input":20,"output":2}}]}}"#,
            "\n",
        ));

        assert_eq!(ids(&session), vec!["m1", "m2"]);
    }

    #[test]
    fn repeated_id_rewrites_message_in_place() {
        // Gemini CLI 先写一条不带 toolCalls 的消息，拿到工具调用后用同一个 id
        // 重写；按追加处理会让同一条消息出现两次。
        let session = parse(concat!(
            r#"{"sessionId":"s-3"}"#,
            "\n",
            r#"{"id":"m1","type":"gemini","content":"x","tokens":{"input":10,"output":1}}"#,
            "\n",
            r#"{"id":"m1","type":"gemini","content":"x","tokens":{"input":10,"output":1},"toolCalls":[{"name":"read_file"}]}"#,
            "\n",
        ));

        assert_eq!(ids(&session), vec!["m1"]);
        assert!(session.messages[0].get("toolCalls").is_some());
    }

    #[test]
    fn parses_jsonl_without_header_line() {
        // 少数会话文件没有首行元数据，消息仍应被解析出来
        let session = parse(concat!(
            r#"{"id":"m1","type":"user","content":"hello"}"#,
            "\n",
            r#"{"$set":{"lastUpdated":"2026-09-04T09:07:00.000Z"}}"#,
            "\n",
            r#"{"id":"m2","type":"gemini","content":"hi","tokens":{"input":5,"output":1}}"#,
            "\n",
        ));

        assert!(session.session_id.is_none());
        assert_eq!(ids(&session), vec!["m1", "m2"]);
    }

    #[test]
    fn first_user_text_skips_session_context_bootstrap() {
        let session = parse(concat!(
            r#"{"sessionId":"s-4"}"#,
            "\n",
            r#"{"id":"m1","type":"user","content":[{"text":"<session_context>\nThis is the Gemini CLI.\n</session_context>"}]}"#,
            "\n",
            r#"{"id":"m2","type":"user","content":[{"text":"帮我连一下 vpn"}]}"#,
            "\n",
        ));

        assert_eq!(session.first_user_text().as_deref(), Some("帮我连一下 vpn"));
    }

    #[test]
    fn first_user_text_returns_none_when_only_bootstrap() {
        let session = parse(concat!(
            r#"{"sessionId":"s-5"}"#,
            "\n",
            r#"{"id":"m1","type":"user","content":[{"text":"<session_context>ctx</session_context>"}]}"#,
            "\n",
        ));

        assert!(session.first_user_text().is_none());
    }

    #[test]
    fn skips_malformed_lines() {
        let session = parse(concat!(
            r#"{"sessionId":"s-6"}"#,
            "\n",
            "not json\n",
            "\n",
            r#"{"id":"m1","type":"gemini","content":"ok"}"#,
            "\n",
        ));

        assert_eq!(session.session_id.as_deref(), Some("s-6"));
        assert_eq!(ids(&session), vec!["m1"]);
    }

    #[test]
    fn message_text_handles_string_and_array_content() {
        assert_eq!(
            message_text(&json!({"content": "plain"})).as_deref(),
            Some("plain")
        );
        assert_eq!(
            message_text(&json!({"content": [{"text": "a"}, {"text": "b"}]})).as_deref(),
            Some("a\nb")
        );
        assert!(message_text(&json!({})).is_none());
    }
}
