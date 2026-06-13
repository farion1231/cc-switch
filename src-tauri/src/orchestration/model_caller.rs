use crate::orchestration::config::ModelConfig;
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

pub struct ModelCaller {
    client: Client,
    models: HashMap<String, ModelConfig>,
}

#[derive(Debug, Clone)]
pub struct ModelResponse {
    pub content: String,
    pub model: String,
    pub usage: TokenUsage,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl ModelCaller {
    pub fn new(models: HashMap<String, ModelConfig>) -> Result<Self, String> {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
        Ok(Self { client, models })
    }

    /// Call a model and return the full text response (non-streaming).
    /// Used by CASCADE/DEBATE/MoA orchestration strategies.
    pub async fn call(
        &self,
        model_key: &str,
        messages: Vec<Value>,
        tools: Option<Vec<Value>>,
        temperature: Option<f64>,
    ) -> Result<ModelResponse, String> {
        let config = self
            .models
            .get(model_key)
            .ok_or_else(|| format!("Model '{}' not found in configuration", model_key))?;

        let api_key = std::env::var(&config.api_key_env).map_err(|_| {
            format!(
                "API key env '{}' not set for model '{}'",
                config.api_key_env, model_key
            )
        })?;

        // Anthropic requires system messages as a top-level field, not in the messages array.
        let (messages, system_prompt) = if config.provider == "anthropic" {
            let (non_system, system_parts): (Vec<_>, Vec<_>) = messages
                .iter()
                .cloned()
                .partition(|m| m.get("role").and_then(|r| r.as_str()) != Some("system"));
            let system_text: Vec<String> = system_parts
                .iter()
                .filter_map(|m| {
                    m.get("content")
                        .and_then(|c| c.as_str())
                        .map(|s| s.to_string())
                })
                .collect();
            (
                non_system,
                if system_text.is_empty() {
                    None
                } else {
                    Some(system_text.join("\n"))
                },
            )
        } else {
            (messages, None)
        };

        let mut body = json!({
            "model": config.model,
            "messages": messages,
            "max_tokens": config.max_tokens,
            "stream": false,
        });

        if let Some(sys) = system_prompt {
            body["system"] = json!(sys);
        }

        if let Some(t) = temperature {
            body["temperature"] = json!(t);
        }
        if let Some(ref t) = tools {
            body["tools"] = json!(t);
        }

        let start = std::time::Instant::now();
        let url = Self::build_url(config)?;
        let mut req = self
            .client
            .post(&url)
            .header("Content-Type", "application/json");

        // Provider-specific auth headers
        match config.provider.as_str() {
            "anthropic" => {
                req = req
                    .header("x-api-key", &api_key)
                    .header("anthropic-version", "2023-06-01");
            }
            _ => {
                req = req.header("Authorization", format!("Bearer {}", api_key));
            }
        }

        let resp = req
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("HTTP error calling '{}': {}", model_key, e))?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_else(|e| format!("<could not read body: {}>", e));
            return Err(format!(
                "Model '{}' returned {}: {}",
                model_key, status, error_body
            ));
        }

        let resp_body: Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse response from '{}': {}", model_key, e))?;

        let latency_ms = start.elapsed().as_millis() as u64;

        let content = Self::extract_content(&resp_body);
        let usage = TokenUsage {
            input_tokens: resp_body
                .get("usage")
                .and_then(|u| u.get("input_tokens").or_else(|| u.get("prompt_tokens")))
                .and_then(|t| t.as_u64())
                .unwrap_or(0),
            output_tokens: resp_body
                .get("usage")
                .and_then(|u| u.get("output_tokens").or_else(|| u.get("completion_tokens")))
                .and_then(|t| t.as_u64())
                .unwrap_or(0),
        };

        Ok(ModelResponse {
            content,
            model: config.model.clone(),
            usage,
            latency_ms,
        })
    }

    /// Call with a single prompt (convenience wrapper for judge/verifier calls)
    pub async fn call_prompt(
        &self,
        model_key: &str,
        system: &str,
        user_prompt: &str,
        temperature: Option<f64>,
    ) -> Result<ModelResponse, String> {
        let messages = build_messages_for_prompt(system, user_prompt);
        self.call(model_key, messages, None, temperature).await
    }

    fn build_url(config: &ModelConfig) -> Result<String, String> {
        match config.provider.as_str() {
            "anthropic" => Ok("https://api.anthropic.com/v1/messages".to_string()),
            "openai" => Ok("https://api.openai.com/v1/chat/completions".to_string()),
            "deepseek" => Ok("https://api.deepseek.com/v1/chat/completions".to_string()),
            "qwen" => Ok(
                "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions".to_string(),
            ),
            "glm" => Ok("https://open.bigmodel.cn/api/paas/v4/chat/completions".to_string()),
            "kimi" => Ok("https://api.moonshot.cn/v1/chat/completions".to_string()),
            "doubao" => Ok(
                "https://ark.cn-beijing.volces.com/api/v3/chat/completions".to_string(),
            ),
            "yi" => Ok("https://api.lingyiwanwu.com/v1/chat/completions".to_string()),
            "baichuan" => Ok("https://api.baichuan-ai.com/v1/chat/completions".to_string()),
            "spark" => Ok(
                "https://spark-api-open.xf-yun.com/v1/chat/completions".to_string(),
            ),
            _ => {
                if let Some(ref url) = config.base_url {
                    return Ok(url.clone());
                }
                return Err(format!(
                    "Unknown provider '{}' and no base_url configured — cannot route request",
                    config.provider
                ));
            }
        }
    }

    fn extract_content(resp: &Value) -> String {
        // Anthropic format: content[0].text
        if let Some(content) = resp.get("content").and_then(|c| c.as_array()) {
            let texts: Vec<String> = content
                .iter()
                .filter_map(|block| {
                    if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                        block
                            .get("text")
                            .and_then(|t| t.as_str())
                            .map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .collect();
            if !texts.is_empty() {
                return texts.join("\n");
            }
        }
        // OpenAI format: choices[0].message.content
        if let Some(content) = resp
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
        {
            return content.to_string();
        }
        // Fallback: return raw JSON
        resp.to_string()
    }
}

/// Build the messages array for call_prompt. Extracted for testability.
pub fn build_messages_for_prompt(system: &str, user_prompt: &str) -> Vec<Value> {
    let mut messages = Vec::with_capacity(2);
    if !system.is_empty() {
        messages.push(json!({"role": "system", "content": system}));
    }
    messages.push(json!({"role": "user", "content": user_prompt}));
    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_anthropic_content() {
        let resp = json!({
            "content": [
                {"type": "text", "text": "Hello world"}
            ],
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });
        assert_eq!(ModelCaller::extract_content(&resp), "Hello world");
    }

    #[test]
    fn extract_openai_content() {
        let resp = json!({
            "choices": [{
                "message": {"content": "Hello from OpenAI"}
            }]
        });
        assert_eq!(ModelCaller::extract_content(&resp), "Hello from OpenAI");
    }

    #[test]
    fn extract_multi_block_anthropic() {
        let resp = json!({
            "content": [
                {"type": "text", "text": "Part 1"},
                {"type": "tool_use", "id": "t1", "name": "bash", "input": {}},
                {"type": "text", "text": "Part 2"}
            ]
        });
        assert_eq!(ModelCaller::extract_content(&resp), "Part 1\nPart 2");
    }

    #[test]
    fn call_prompt_builds_messages_with_system() {
        let messages = build_messages_for_prompt("You are a judge.", "Evaluate this.");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "You are a judge.");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "Evaluate this.");
    }

    #[test]
    fn call_prompt_builds_messages_without_system() {
        let messages = build_messages_for_prompt("", "Hello");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "Hello");
    }

    #[test]
    fn unknown_provider_uses_explicit_base_url() {
        let config = ModelConfig {
            provider: "minimax".to_string(),
            model: "MiniMax-Text-01".to_string(),
            api_key_env: "MINIMAX_API_KEY".to_string(),
            base_url: Some("https://example.com/v1/chat/completions".to_string()),
            max_tokens: 1024,
        };

        assert_eq!(
            ModelCaller::build_url(&config).unwrap(),
            "https://example.com/v1/chat/completions"
        );
    }

    #[test]
    fn unknown_provider_no_base_url_returns_error() {
        let config = ModelConfig {
            provider: "minimax".to_string(),
            model: "MiniMax-Text-01".to_string(),
            api_key_env: "MINIMAX_API_KEY".to_string(),
            base_url: None,
            max_tokens: 1024,
        };
        assert!(ModelCaller::build_url(&config).is_err());
    }
}
