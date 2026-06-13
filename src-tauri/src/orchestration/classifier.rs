use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    Coding,
    Summary,
    Architecture,
    Image,
    Chat,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProfile {
    pub task_type: TaskType,
    pub complexity: f64,
    pub risk: RiskLevel,
    pub verifiability: f64,
    pub has_image: bool,
    pub need_code: bool,
    pub has_audio: bool,
    pub has_tools: bool,
    pub is_streaming: bool,
    pub requires_exact_format: bool,
    pub eligible_for_orchestration: bool,
    pub ineligibility_reason: Option<String>,
}

pub struct TaskClassifier;

impl TaskClassifier {
    pub fn classify(body: &serde_json::Value) -> TaskProfile {
        let messages = body.get("messages").and_then(|m| m.as_array());
        let tools = body.get("tools").and_then(|t| t.as_array());

        let content_text = extract_text_content(messages);
        let msg_count = messages.map(|m| m.len()).unwrap_or(0);
        let has_image = detect_image(messages);

        let has_tools = Self::has_tools(body);
        let is_streaming = Self::is_streaming(body);
        let has_audio = Self::has_audio(body);
        let requires_exact_format = Self::requires_exact_format(body);
        let (eligible_for_orchestration, ineligibility_reason) =
            Self::orchestration_eligibility(is_streaming, has_tools, has_audio);

        let task_type = Self::detect_task_type(&content_text, has_tools, has_image);
        let complexity = Self::calc_complexity(&content_text, msg_count, has_tools);
        let risk = Self::detect_risk(&content_text);
        let verifiability = Self::calc_verifiability(has_tools);
        let need_code = Self::detect_need_code(&content_text);

        TaskProfile {
            task_type,
            complexity,
            risk,
            verifiability,
            has_image,
            need_code,
            has_audio,
            has_tools,
            is_streaming,
            requires_exact_format,
            eligible_for_orchestration,
            ineligibility_reason,
        }
    }

    fn has_tools(body: &serde_json::Value) -> bool {
        body.get("tools")
            .and_then(|v| v.as_array())
            .map(|tools| !tools.is_empty())
            .unwrap_or(false)
    }

    fn is_streaming(body: &serde_json::Value) -> bool {
        body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false)
    }

    fn has_audio(body: &serde_json::Value) -> bool {
        let Some(messages) = body.get("messages").and_then(|v| v.as_array()) else {
            return false;
        };

        messages.iter().any(|message| {
            let Some(content) = message.get("content") else {
                return false;
            };
            if let Some(items) = content.as_array() {
                return items.iter().any(|item| {
                    matches!(
                        item.get("type").and_then(|v| v.as_str()),
                        Some("input_audio") | Some("audio") | Some("audio_url")
                    ) || item.get("audio").is_some()
                });
            }
            false
        })
    }

    fn requires_exact_format(body: &serde_json::Value) -> bool {
        let text = Self::extract_text(body).to_ascii_lowercase();
        text.contains("return json")
            || text.contains("valid json")
            || text.contains("exact format")
            || text.contains("do not include anything else")
    }

    fn orchestration_eligibility(
        is_streaming: bool,
        has_tools: bool,
        has_audio: bool,
    ) -> (bool, Option<String>) {
        if is_streaming || has_tools || has_audio {
            return (false, Some("streaming_or_tools_or_audio".to_string()));
        }
        (true, None)
    }

    fn extract_text(body: &serde_json::Value) -> String {
        let Some(messages) = body.get("messages").and_then(|v| v.as_array()) else {
            return String::new();
        };

        messages
            .iter()
            .filter_map(|message| message.get("content"))
            .flat_map(|content| {
                if let Some(s) = content.as_str() {
                    vec![s.to_string()]
                } else if let Some(items) = content.as_array() {
                    items
                        .iter()
                        .filter_map(|item| {
                            item.get("text")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                        })
                        .collect()
                } else {
                    Vec::new()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn detect_task_type(text: &str, has_tools: bool, has_image: bool) -> TaskType {
        if has_image {
            return TaskType::Image;
        }
        let lower = text.to_lowercase();
        if lower.contains("implement")
            || lower.contains("write")
            || lower.contains("create function")
            || lower.contains("```")
        {
            return TaskType::Coding;
        }
        if lower.contains("architecture")
            || lower.contains("design")
            || lower.contains("refactor")
            || lower.contains("migrate")
        {
            return TaskType::Architecture;
        }
        if lower.contains("summarize") || lower.contains("explain") {
            return TaskType::Summary;
        }
        if has_tools {
            return TaskType::Coding;
        }
        TaskType::Chat
    }

    fn calc_complexity(text: &str, msg_count: usize, has_tools: bool) -> f64 {
        let token_estimate = text.len() as f64 / 4.0;
        // Text length contributes up to 0.6 (not capped at 0.5, so long non-tool
        // messages can still reach high complexity).
        let mut score = (token_estimate / 3000.0).min(0.6);
        if has_tools {
            score += 0.25;
        }
        if msg_count > 6 {
            score += 0.15;
        }
        // Detect multi-file / multi-task complexity from markdown headers
        let header_count = text.matches("\n#").count();
        if header_count > 3 {
            score += 0.1;
        }
        score.min(1.0)
    }

    fn detect_risk(text: &str) -> RiskLevel {
        let lower = text.to_lowercase();
        if lower.contains("delete")
            || lower.contains("remove")
            || lower.contains("drop")
            || lower.contains("truncate")
        {
            return RiskLevel::Critical;
        }
        if lower.contains("refactor") || lower.contains("migrate") {
            return RiskLevel::High;
        }
        if lower.contains("fix") || lower.contains("update") {
            return RiskLevel::Medium;
        }
        RiskLevel::Low
    }

    fn calc_verifiability(has_tools: bool) -> f64 {
        if has_tools {
            0.9
        } else {
            0.1
        }
    }

    fn detect_need_code(text: &str) -> bool {
        let lower = text.to_lowercase();
        lower.contains("implement")
            || lower.contains("write")
            || lower.contains("create function")
            || lower.contains("```")
            || lower.contains("code")
    }
}

fn extract_text_content(messages: Option<&Vec<serde_json::Value>>) -> String {
    let Some(messages) = messages else {
        return String::new();
    };
    let mut text = String::new();
    for msg in messages {
        if let Some(content) = msg.get("content") {
            if let Some(s) = content.as_str() {
                text.push_str(s);
                text.push(' ');
            } else if let Some(arr) = content.as_array() {
                for block in arr {
                    if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                            text.push_str(t);
                            text.push(' ');
                        }
                    }
                }
            }
        }
    }
    text
}

fn detect_image(messages: Option<&Vec<serde_json::Value>>) -> bool {
    let Some(messages) = messages else {
        return false;
    };
    for msg in messages {
        if let Some(content) = msg.get("content") {
            if let Some(arr) = content.as_array() {
                for block in arr {
                    let block_type = block.get("type").and_then(|t| t.as_str());
                    if block_type == Some("image") || block_type == Some("image_url") {
                        return true;
                    }
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classify_coding_task() {
        let body = json!({
            "messages": [{"role": "user", "content": "implement a binary search"}],
            "model": "claude-opus-4"
        });
        let profile = TaskClassifier::classify(&body);
        assert_eq!(profile.task_type, TaskType::Coding);
        assert!(profile.need_code);
    }

    #[test]
    fn classify_simple_question() {
        let body = json!({
            "messages": [{"role": "user", "content": "what is 2+2?"}],
            "model": "claude-opus-4"
        });
        let profile = TaskClassifier::classify(&body);
        assert_eq!(profile.task_type, TaskType::Chat);
        assert!(!profile.need_code);
        assert!(profile.complexity < 0.4);
    }

    #[test]
    fn classify_critical_risk() {
        let body = json!({
            "messages": [{"role": "user", "content": "delete all temp files from /tmp"}],
            "model": "claude-opus-4"
        });
        let profile = TaskClassifier::classify(&body);
        assert_eq!(profile.risk, RiskLevel::Critical);
    }

    #[test]
    fn classify_with_tools() {
        let body = json!({
            "messages": [{"role": "user", "content": "fix the bug"}],
            "tools": [{"name": "bash", "type": "function"}],
            "model": "claude-opus-4"
        });
        let profile = TaskClassifier::classify(&body);
        assert_eq!(profile.task_type, TaskType::Coding);
        assert!(profile.verifiability > 0.5);
    }

    #[test]
    fn classify_image_content() {
        let body = json!({
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "describe this image"},
                    {"type": "image", "source": {"type": "base64", "data": "..."}}
                ]
            }],
            "model": "claude-opus-4"
        });
        let profile = TaskClassifier::classify(&body);
        assert_eq!(profile.task_type, TaskType::Image);
        assert!(profile.has_image);
    }

    #[test]
    fn classify_detects_streaming_tools_and_audio() {
        let body = json!({
            "stream": true,
            "tools": [{"name": "shell"}],
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "input_audio", "audio": {"data": "abc", "format": "wav"}},
                    {"type": "text", "text": "transcribe this"}
                ]
            }]
        });

        let profile = TaskClassifier::classify(&body);

        assert!(profile.is_streaming);
        assert!(profile.has_tools);
        assert!(profile.has_audio);
        assert!(!profile.eligible_for_orchestration);
        assert_eq!(
            profile.ineligibility_reason.as_deref(),
            Some("streaming_or_tools_or_audio")
        );
    }

    #[test]
    fn classify_text_request_is_eligible() {
        let body = json!({
            "stream": false,
            "messages": [{"role": "user", "content": "explain merge sort"}]
        });

        let profile = TaskClassifier::classify(&body);

        assert!(!profile.is_streaming);
        assert!(!profile.has_tools);
        assert!(!profile.has_audio);
        assert!(profile.eligible_for_orchestration);
        assert_eq!(profile.ineligibility_reason, None);
    }
}
