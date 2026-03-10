//! Type definitions for KeyMux

use serde::{Deserialize, Serialize};

/// Provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api_format: ApiFormat,
}

/// API format for different providers
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiFormat {
    OpenAI,     // POST /v1/chat/completions
    Anthropic,  // POST /v1/messages
    Google,     // POST /v1beta/models/{model}:generateContent
    OpenRouter, // OpenAI-compatible
}

/// Model identifier parsed from `/provider/model` syntax
#[derive(Debug, Clone)]
pub struct ModelId {
    pub provider: String,
    pub model: String,
}

impl ModelId {
    /// Parse model identifier from string (e.g., "anthropic/claude-3-5-sonnet")
    pub fn parse(s: &str) -> Option<Self> {
        let mut parts = s.splitn(2, '/');
        let provider = parts.next()?.trim().to_string();
        let model = parts.next()?.trim().to_string();

        if provider.is_empty() || model.is_empty() {
            return None;
        }

        Some(Self { provider, model })
    }
}

/// OpenAI-compatible chat completion request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
}

/// Message in chat completion request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

/// Tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

/// Function definition for tools
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: serde_json::Value,
}

/// Tool call in assistant message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionCall,
}

/// Function call details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// OpenAI-compatible chat completion response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

/// Choice in chat completion response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub index: u32,
    pub message: Message,
    pub finish_reason: Option<String>,
}

/// Usage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// API key with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: String,
    pub provider: String,
    pub key: String, // Encrypted
    pub quota_limit: Option<f64>,
    pub quota_used: f64,
    pub rate_limit_rpm: Option<u64>,
    pub is_active: bool,
    pub created_at: i64,
}

/// Rank context for intelligent routing
#[derive(Debug, Clone)]
pub struct RankContext {
    pub provider: String,
    pub model: String,
    pub key_id: String,
    pub observed_latency_ms: f64,
    pub cost_per_token: f64,
    pub capability_flags: CapabilityFlags,
    pub quota_remaining: f64,
    pub carrier_quality: CarrierMetrics,
}

/// Capability flags for models
#[derive(Debug, Clone, Copy, Default)]
pub struct CapabilityFlags {
    pub vision: bool,
    pub function_calling: bool,
    pub reasoning: bool,
    pub long_context: bool,
}

impl CapabilityFlags {
    /// Calculate match score (0.0 - 1.0)
    pub fn match_score(&self) -> f64 {
        let total = 4;
        let matched = [
            self.vision,
            self.function_calling,
            self.reasoning,
            self.long_context,
        ]
        .iter()
        .filter(|&&b| b)
        .count() as f64;
        matched / total
    }
}

/// Carrier/radio quality metrics (from LiteBike)
#[derive(Debug, Clone, Default)]
pub struct CarrierMetrics {
    pub signal_strength: f64, // dBm, higher is better (e.g., -50 > -100)
    pub latency_ms: f64,
    pub packet_loss: f64, // 0.0 - 1.0
    pub bandwidth_mbps: f64,
}

impl CarrierMetrics {
    /// Calculate normalized score (0.0 - 1.0)
    pub fn normalized_score(&self) -> f64 {
        // Simple weighted score
        let signal_score = ((self.signal_strength + 100.0) / 50.0).clamp(0.0, 1.0);
        let latency_score = 1.0 / (1.0 + self.latency_ms / 100.0);
        let loss_score = 1.0 - self.packet_loss;
        let bandwidth_score = (self.bandwidth_mbps / 100.0).clamp(0.0, 1.0);

        (signal_score * 0.3 + latency_score * 0.3 + loss_score * 0.2 + bandwidth_score * 0.2)
            .clamp(0.0, 1.0)
    }
}
