use crate::orchestration::executor::ExecutionResult;
use serde_json::{json, Value};

/// Convert an orchestration `ExecutionResult` into the format expected by the calling client.
pub struct ResponseAdapter;

impl ResponseAdapter {
    /// Produce an Anthropic Messages API-compatible response body.
    pub fn to_anthropic(result: &ExecutionResult, model: &str) -> Value {
        json!({
            "id": format!("msg_orch_{}", uuid::Uuid::new_v4()),
            "type": "message",
            "role": "assistant",
            "model": model,
            "content": [{
                "type": "text",
                "text": result.content,
            }],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": result.total_input_tokens,
                "output_tokens": result.total_output_tokens,
            },
            "omniagent": {
                "strategy": result.strategy,
                "model_used": result.model_used,
                "verified": result.verified,
                "judge_score": result.judge_score,
                "cascade_attempts": result.cascade_attempts,
                "total_latency_ms": result.total_latency_ms,
            }
        })
    }

    /// Produce an OpenAI Chat Completions API-compatible response body.
    pub fn to_openai(result: &ExecutionResult, model: &str) -> Value {
        json!({
            "id": format!("chatcmpl-orch-{}", uuid::Uuid::new_v4()),
            "object": "chat.completion",
            "created": chrono::Utc::now().timestamp(),
            "model": model,
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": result.content,
                },
                "finish_reason": "stop",
            }],
            "usage": {
                "prompt_tokens": result.total_input_tokens,
                "completion_tokens": result.total_output_tokens,
                "total_tokens": result.total_input_tokens + result.total_output_tokens,
            },
            "omniagent": {
                "strategy": result.strategy,
                "model_used": result.model_used,
                "verified": result.verified,
                "judge_score": result.judge_score,
                "cascade_attempts": result.cascade_attempts,
                "total_latency_ms": result.total_latency_ms,
            }
        })
    }

    /// Convert ExecutionResult into the appropriate response format based on client type.
    pub fn to_response(result: &ExecutionResult, model: &str, client: &str) -> Value {
        match client {
            "claude_code" | "claude" => Self::to_anthropic(result, model),
            "codex" | "opencode" | "gemini" => Self::to_openai(result, model),
            _ => Self::to_openai(result, model),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_response_includes_omniagent_metadata() {
        let result = ExecutionResult {
            content: "Hello from orchestrator".to_string(),
            model_used: "deepseek-chat".to_string(),
            strategy: "debate".to_string(),
            total_latency_ms: 1500,
            total_input_tokens: 100,
            total_output_tokens: 50,
            cascade_attempts: 2,
            verified: true,
            judge_score: Some(0.85),
        };
        let response = ResponseAdapter::to_anthropic(&result, "claude-sonnet-4");
        assert_eq!(response["type"], "message");
        assert_eq!(response["content"][0]["text"], "Hello from orchestrator");
        assert_eq!(response["omniagent"]["strategy"], "debate");
        assert_eq!(response["omniagent"]["judge_score"], 0.85);
    }

    #[test]
    fn openai_response_includes_usage() {
        let result = ExecutionResult {
            content: "Synthesized answer".to_string(),
            model_used: "glm-4-flash".to_string(),
            strategy: "moa".to_string(),
            total_latency_ms: 2300,
            total_input_tokens: 200,
            total_output_tokens: 80,
            cascade_attempts: 3,
            verified: true,
            judge_score: Some(0.92),
        };
        let response = ResponseAdapter::to_openai(&result, "gpt-4o");
        assert_eq!(
            response["choices"][0]["message"]["content"],
            "Synthesized answer"
        );
        assert_eq!(response["usage"]["prompt_tokens"], 200);
        assert_eq!(response["omniagent"]["strategy"], "moa");
    }
}
