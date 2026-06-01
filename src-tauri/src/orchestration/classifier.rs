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
}

pub struct TaskClassifier;

impl TaskClassifier {
    pub fn classify(body: &serde_json::Value) -> TaskProfile {
        let messages = body.get("messages").and_then(|m| m.as_array());
        let tools = body.get("tools").and_then(|t| t.as_array());

        let content_text = extract_text_content(messages);
        let msg_count = messages.map(|m| m.len()).unwrap_or(0);
        let has_tools = tools.map(|t| !t.is_empty()).unwrap_or(false);
        let has_image = detect_image(messages);

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
        }
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
        let mut score = (token_estimate / 4000.0).min(0.5);
        if has_tools {
            score += 0.3;
        }
        if msg_count > 6 {
            score += 0.2;
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
}
