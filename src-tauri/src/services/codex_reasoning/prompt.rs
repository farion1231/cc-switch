//! Provider-level Codex system prompt replacement + identity correction.
//!
//! Rewrite runs on the outbound body **before** protocol conversion
//! (Chat / Anthropic transformers receive the already-rewritten body).
//! Fingerprints the final effective system text only — never returns prompt text.

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn default_true() -> bool {
    true
}

fn default_max_rounds() -> u8 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CodexSystemPromptConfig {
    pub enabled: bool,
    pub replacement: String,
    #[serde(default = "default_true")]
    pub correct_model_identity: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexReasoningContinuationConfig {
    pub enabled: bool,
    #[serde(default = "default_max_rounds")]
    pub max_rounds: u8,
}

impl Default for CodexReasoningContinuationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_rounds: default_max_rounds(),
        }
    }
}

impl CodexReasoningContinuationConfig {
    /// Clamp max_rounds to `0..=3`.
    pub fn clamped(mut self) -> Self {
        self.max_rounds = self.max_rounds.min(3);
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptRewriteMetadata {
    pub replaced: bool,
    pub identity_corrected: bool,
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // ChatCompletions reserved for non-Responses path
pub enum CodexRequestProtocol {
    Responses,
    ChatCompletions,
}

/// Apply identity correction to the replacement string only.
/// Replaces common "You are ChatGPT / OpenAI" phrasing with the selected model.
fn apply_identity_correction(text: &str, model: &str) -> (String, bool) {
    let mut out = text.to_string();
    let mut corrected = false;

    let patterns = [
        ("You are ChatGPT", format!("You are {model}")),
        (
            "You are a large language model trained by OpenAI",
            format!("You are {model}"),
        ),
        ("You are OpenAI", format!("You are {model}")),
        ("ChatGPT", model.to_string()),
    ];

    for (from, to) in patterns {
        if out.contains(from) {
            out = out.replace(from, &to);
            corrected = true;
        }
    }
    (out, corrected)
}

fn fingerprint_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let dig = hasher.finalize();
    // 32 hex chars (128 bits) — enough for log correlation, not full prompt.
    format!("{:x}", dig)[..32].to_string()
}

fn role_of(item: &Value) -> Option<&str> {
    item.get("role").and_then(|r| r.as_str())
}

fn is_system_or_developer(role: &str) -> bool {
    role == "system" || role == "developer"
}

/// Rewrite Codex system prompt layers on the request body.
///
/// - Responses: set `instructions` to replacement; drop system/developer input items.
/// - Chat: remove system/developer messages; insert exactly one system message at index 0.
/// - User content is never modified.
/// - Fingerprint only; metadata never contains prompt text.
pub fn rewrite_codex_system_prompt(
    request: &mut Value,
    model: &str,
    config: Option<&CodexSystemPromptConfig>,
    protocol: CodexRequestProtocol,
) -> Result<PromptRewriteMetadata, AppError> {
    let Some(cfg) = config else {
        return Ok(PromptRewriteMetadata::default());
    };
    if !cfg.enabled {
        return Ok(PromptRewriteMetadata::default());
    }

    let mut effective = cfg.replacement.clone();
    let mut identity_corrected = false;
    if cfg.correct_model_identity {
        let (corrected, did) = apply_identity_correction(&effective, model);
        effective = corrected;
        identity_corrected = did;
    }

    match protocol {
        CodexRequestProtocol::Responses => {
            request["instructions"] = Value::String(effective.clone());
            if let Some(input) = request.get_mut("input").and_then(|v| v.as_array_mut()) {
                input.retain(|item| {
                    role_of(item)
                        .map(|r| !is_system_or_developer(r))
                        .unwrap_or(true)
                });
            }
        }
        CodexRequestProtocol::ChatCompletions => {
            if let Some(messages) = request.get_mut("messages").and_then(|v| v.as_array_mut()) {
                messages.retain(|item| {
                    role_of(item)
                        .map(|r| !is_system_or_developer(r))
                        .unwrap_or(true)
                });
                messages.insert(
                    0,
                    json!({
                        "role": "system",
                        "content": effective.clone(),
                    }),
                );
            } else {
                request["messages"] = json!([
                    {
                        "role": "system",
                        "content": effective.clone(),
                    }
                ]);
            }
        }
    }

    Ok(PromptRewriteMetadata {
        replaced: true,
        identity_corrected,
        fingerprint: Some(fingerprint_text(&effective)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn enabled_cfg(replacement: &str, identity: bool) -> CodexSystemPromptConfig {
        CodexSystemPromptConfig {
            enabled: true,
            replacement: replacement.into(),
            correct_model_identity: identity,
        }
    }

    #[test]
    fn responses_replaces_only_system_layers_and_corrects_identity() -> Result<(), AppError> {
        let mut request = json!({
            "model": "gpt-5.4",
            "instructions": "old system",
            "input": [
                {"role": "developer", "content": "dev layer"},
                {"role": "user", "content": "hello user"},
                {"role": "system", "content": "sys layer"},
            ]
        });
        let cfg = enabled_cfg("You are ChatGPT, a helpful assistant.", true);
        let meta = rewrite_codex_system_prompt(
            &mut request,
            "gpt-5.4",
            Some(&cfg),
            CodexRequestProtocol::Responses,
        )?;
        assert!(meta.replaced);
        assert!(meta.identity_corrected);
        assert_eq!(
            request["instructions"].as_str().unwrap(),
            "You are gpt-5.4, a helpful assistant."
        );
        let input = request["input"].as_array().unwrap();
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"], "hello user");
        assert_eq!(meta.fingerprint.as_ref().unwrap().len(), 32);
        Ok(())
    }

    #[test]
    fn chat_inserts_single_system_message_at_index_zero() -> Result<(), AppError> {
        let mut request = json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "old"},
                {"role": "developer", "content": "dev"},
                {"role": "user", "content": "hi"},
            ]
        });
        let cfg = enabled_cfg("NEW SYSTEM", false);
        let meta = rewrite_codex_system_prompt(
            &mut request,
            "gpt-4o",
            Some(&cfg),
            CodexRequestProtocol::ChatCompletions,
        )?;
        assert!(meta.replaced);
        assert!(!meta.identity_corrected);
        let msgs = request["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "NEW SYSTEM");
        assert_eq!(msgs[1]["role"], "user");
        Ok(())
    }

    #[test]
    fn missing_or_disabled_config_is_noop() -> Result<(), AppError> {
        let mut request = json!({
            "instructions": "keep",
            "input": [{"role": "user", "content": "u"}]
        });
        let meta =
            rewrite_codex_system_prompt(&mut request, "m", None, CodexRequestProtocol::Responses)?;
        assert!(!meta.replaced);
        assert_eq!(request["instructions"], "keep");

        let disabled = CodexSystemPromptConfig {
            enabled: false,
            replacement: "x".into(),
            correct_model_identity: true,
        };
        let meta2 = rewrite_codex_system_prompt(
            &mut request,
            "m",
            Some(&disabled),
            CodexRequestProtocol::Responses,
        )?;
        assert!(!meta2.replaced);
        assert_eq!(request["instructions"], "keep");
        Ok(())
    }

    #[test]
    fn user_content_is_never_modified() -> Result<(), AppError> {
        let mut request = json!({
            "instructions": "old",
            "input": [
                {"role": "user", "content": "You are ChatGPT please help"},
            ]
        });
        let cfg = enabled_cfg("replacement only", true);
        let _ = rewrite_codex_system_prompt(
            &mut request,
            "gpt-x",
            Some(&cfg),
            CodexRequestProtocol::Responses,
        )?;
        assert_eq!(
            request["input"][0]["content"],
            "You are ChatGPT please help"
        );
        Ok(())
    }

    #[test]
    fn fingerprint_is_stable_and_has_no_prompt_text() -> Result<(), AppError> {
        let mut a = json!({"instructions": "x", "input": []});
        let mut b = json!({"instructions": "x", "input": []});
        let cfg = enabled_cfg("same text", false);
        let m1 =
            rewrite_codex_system_prompt(&mut a, "m", Some(&cfg), CodexRequestProtocol::Responses)?;
        let m2 =
            rewrite_codex_system_prompt(&mut b, "m", Some(&cfg), CodexRequestProtocol::Responses)?;
        assert_eq!(m1.fingerprint, m2.fingerprint);
        let fp = m1.fingerprint.unwrap();
        assert_eq!(fp.len(), 32);
        assert!(!fp.contains("same"));
        Ok(())
    }

    #[test]
    fn continuation_and_prompt_toggles_are_independent() {
        let prompt = CodexSystemPromptConfig {
            enabled: false,
            replacement: String::new(),
            correct_model_identity: true,
        };
        let cont = CodexReasoningContinuationConfig {
            enabled: true,
            max_rounds: 3,
        };
        assert!(!prompt.enabled);
        assert!(cont.enabled);
        assert_eq!(cont.clamped().max_rounds, 3);
        assert_eq!(
            CodexReasoningContinuationConfig {
                enabled: true,
                max_rounds: 9
            }
            .clamped()
            .max_rounds,
            3
        );
    }
}
