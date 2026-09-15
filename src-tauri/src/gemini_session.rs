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
//! 语义上有三处容易踩坑，都会直接影响 token 统计的准确性：
//!
//! - `$set.messages` 是**全量替换**而非追加。会话被 resume 或压缩后，整段历史
//!   会以快照形式重新写入，跳过这类行会丢掉快照里的全部消息。
//! - 独立消息行按 `id` **改写**已有消息：Gemini CLI 先写一条不带 `toolCalls`
//!   的消息，拿到工具调用后再用同一个 `id` 重写一次。按追加处理会让同一条消息
//!   出现两次。
//! - 快照替换的是**当前会话视图**，不是「这些请求花过多少 token」。上下文超限时
//!   Gemini CLI 会把最旧的一半裁掉，被裁掉的请求仍然留在文件里、也确实计过费。
//!   所以 [`GeminiSessionFile::messages`] 给会话管理器展示，
//!   [`GeminiSessionFile::usage_events`] 给用量导入器计费，两者不可互换。
//!
//! 更早的版本把整个会话写成单个 JSON 对象（顶层 `messages` 数组），这里一并兼容。

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

/// Gemini CLI 注入的会话引导块前缀，不作为会话标题
const SESSION_CONTEXT_PREFIX: &str = "<session_context>";

/// 一个 Gemini 会话文件回放后的结果
#[derive(Debug, Default)]
pub struct GeminiSessionFile {
    pub session_id: Option<String>,
    pub start_time: Option<Value>,
    pub last_updated: Option<Value>,
    /// 最终态消息列表：`$set.messages` 快照会整体替换它。供会话管理器展示。
    pub messages: Vec<Value>,
    /// 回放途中出现过的全部带 `tokens` 的消息，按 `id` 去重、后写覆盖。
    ///
    /// 与 [`Self::messages`] 的差别只在被快照截断掉的那部分：它们不再属于当前
    /// 会话，但对应的请求确实消耗过 token，计费不能漏。供用量导入器使用。
    pub usage_events: Vec<Value>,
    /// 回放时无法解析、被跳过的行数
    pub malformed_lines: usize,
    /// 其中位于文件中间（非末行）的数量。
    ///
    /// 末行损坏多半是写入被中断，下次追加会补全，可以容忍；中间坏行会让其后的
    /// 行回放到错误的状态上（丢掉一行 `$set.messages` 尤其致命），调用方应据此
    /// 避免推进同步游标，留待下次重试。
    pub malformed_interior_lines: usize,
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

/// 回放会话文件内容，得到消息列表的最终状态与全部计费事件。
///
/// 先整体按单个 JSON 对象解析（旧版格式，可能是缩进过的多行 JSON）；失败则
/// 按 JSONL 逐行回放。只有一行的新版文件两条路径结果一致。
pub fn parse(data: &str) -> GeminiSessionFile {
    let mut session = GeminiSessionFile::default();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut usage_index: HashMap<String, usize> = HashMap::new();

    match serde_json::from_str::<Value>(data) {
        Ok(value) if value.is_object() => session.apply(value, &mut index, &mut usage_index),
        _ => {
            let lines: Vec<&str> = data
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect();
            let last = lines.len().saturating_sub(1);

            for (pos, line) in lines.iter().enumerate() {
                match serde_json::from_str::<Value>(line) {
                    Ok(value) => session.apply(value, &mut index, &mut usage_index),
                    Err(_) => {
                        session.malformed_lines += 1;
                        if pos != last {
                            session.malformed_interior_lines += 1;
                        }
                    }
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

    fn apply(
        &mut self,
        value: Value,
        index: &mut HashMap<String, usize>,
        usage_index: &mut HashMap<String, usize>,
    ) {
        if let Some(patch) = value.get("$set") {
            if let Some(fields) = patch.as_object() {
                for (key, field) in fields {
                    match key.as_str() {
                        "messages" => {
                            if let Some(list) = field.as_array() {
                                self.replace_messages(list.clone(), index, usage_index);
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
                self.replace_messages(list.clone(), index, usage_index);
            }
            return;
        }

        self.upsert_message(value, index, usage_index);
    }

    /// 快照整体替换当前会话视图。
    ///
    /// `index` 记录消息在 `messages` 里的下标，替换后必须重建；`usage_index`
    /// 记录计费事件的位置，**不随快照清空** —— 被截断掉的消息只是不再属于当前
    /// 会话，它们对应的 token 已经花掉了。
    fn replace_messages(
        &mut self,
        messages: Vec<Value>,
        index: &mut HashMap<String, usize>,
        usage_index: &mut HashMap<String, usize>,
    ) {
        self.messages = messages;
        index.clear();
        for (pos, message) in self.messages.iter().enumerate() {
            if let Some(id) = message.get("id").and_then(Value::as_str) {
                index.insert(id.to_string(), pos);
            }
            record_usage(&mut self.usage_events, usage_index, message);
        }
    }

    fn upsert_message(
        &mut self,
        message: Value,
        index: &mut HashMap<String, usize>,
        usage_index: &mut HashMap<String, usize>,
    ) {
        // message 随后会被 move 进 messages，计费事件要在这之前记下来
        record_usage(&mut self.usage_events, usage_index, &message);

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

/// 把一条带 `tokens` 的消息记进计费事件列表：按 `id` 去重，后写覆盖。
///
/// 这里只做「值不重复」的去重，不判断消息是否真的会被计费（`type == "gemini"`
/// 等交给导入侧），保持回放层不解释业务语义。
fn record_usage(events: &mut Vec<Value>, index: &mut HashMap<String, usize>, message: &Value) {
    if !message.get("tokens").is_some_and(Value::is_object) {
        return;
    }

    match message.get("id").and_then(Value::as_str) {
        Some(id) => match index.get(id) {
            Some(&pos) => events[pos] = message.clone(),
            None => {
                index.insert(id.to_string(), events.len());
                events.push(message.clone());
            }
        },
        // 没有 id 就无法去重：全部保留，由导入侧的 request_id 兜底（它们都会
        // 落到 `unknown` 这一条上）。实测会话文件里没有这种消息。
        None => events.push(message.clone()),
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

    fn usage_ids(session: &GeminiSessionFile) -> Vec<&str> {
        session
            .usage_events
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
    fn usage_events_survive_snapshot_truncation() {
        // 上下文超限时 Gemini CLI 会把最旧的一半裁掉（快照只保留尾部）。最终态
        // 应随之收缩，但被裁掉的那次请求确实花过 token，计费事件不能跟着丢。
        let session = parse(concat!(
            r#"{"sessionId":"s-7"}"#,
            "\n",
            r#"{"id":"m1","type":"gemini","content":"a","tokens":{"input":10,"output":1}}"#,
            "\n",
            r#"{"id":"m2","type":"gemini","content":"b","tokens":{"input":20,"output":2}}"#,
            "\n",
            r#"{"id":"m3","type":"gemini","content":"c","tokens":{"input":30,"output":3}}"#,
            "\n",
            r#"{"$set":{"messages":[{"id":"m2","type":"gemini","content":"b","tokens":{"input":20,"output":2}},{"id":"m3","type":"gemini","content":"c","tokens":{"input":30,"output":3}}]}}"#,
            "\n",
        ));

        assert_eq!(ids(&session), vec!["m2", "m3"], "最终态应反映截断后的会话");
        assert_eq!(
            usage_ids(&session),
            vec!["m1", "m2", "m3"],
            "被截断的 m1 仍应计费，且 m2/m3 不因快照重叠重复出现"
        );
    }

    #[test]
    fn usage_events_keep_order_and_overwrite_in_place() {
        // 同 id 重写（补 toolCalls）只更新位置上的值，不追加、不打乱顺序
        let session = parse(concat!(
            r#"{"sessionId":"s-8"}"#,
            "\n",
            r#"{"id":"m1","type":"gemini","content":"a","tokens":{"input":10,"output":1}}"#,
            "\n",
            r#"{"id":"m2","type":"gemini","content":"b","tokens":{"input":20,"output":2}}"#,
            "\n",
            r#"{"id":"m1","type":"gemini","content":"a","tokens":{"input":10,"output":1},"toolCalls":[{"name":"read_file"}]}"#,
            "\n",
        ));

        assert_eq!(usage_ids(&session), vec!["m1", "m2"]);
        assert!(session.usage_events[0].get("toolCalls").is_some());
    }

    #[test]
    fn usage_events_ignore_messages_without_tokens() {
        let session = parse(concat!(
            r#"{"sessionId":"s-9"}"#,
            "\n",
            r#"{"id":"m1","type":"user","content":"hi"}"#,
            "\n",
            r#"{"id":"m2","type":"gemini","content":"ok"}"#,
            "\n",
        ));

        assert!(session.usage_events.is_empty());
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
        assert_eq!(session.malformed_lines, 1);
        assert_eq!(
            session.malformed_interior_lines, 1,
            "坏行之后还有内容，说明不是写入中断留下的半行"
        );
    }

    #[test]
    fn flags_incomplete_final_line_separately() {
        // 写入被中断留下的半行：可以容忍，但要能和中间坏行区分开
        let session = parse(concat!(
            r#"{"sessionId":"s-10"}"#,
            "\n",
            r#"{"id":"m1","type":"gemini","content":"ok"}"#,
            "\n",
            r#"{"id":"m2","type":"gem"#,
        ));

        assert_eq!(ids(&session), vec!["m1"]);
        assert_eq!(session.malformed_lines, 1);
        assert_eq!(session.malformed_interior_lines, 0);
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
