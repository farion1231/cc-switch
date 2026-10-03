//! 客户端契约：代理模式下写进客户端文件的内容。
//!
//! 契约由路由供应商和本地代理地址决定。代理内切换时，新路由的契约和当前的摘要相同，
//! 客户端文件就不读也不写，只换代理的上游；不同（独有字段、模型别名的 1M 标记、显示名
//! 不同，或代理地址变了）时，在同一个操作里先改写客户端，再发布路由。

use crate::live::engine::sha256_hex;
use serde_json::{json, Map, Value};

use crate::live::project::claude::ClaudeProjection;
use crate::live::project::gemini::GeminiProjection;
use crate::live::project::grok::GrokProjection;

use super::state::Contract;

pub const CONTRACT_VERSION: u32 = 1;

fn key_of(parts: &Value) -> String {
    sha256_hex(&serde_json::to_vec(parts).expect("contract parts serialize"))
}

/// Claude Code：契约就是代理投影里的关键字段和独有字段。
pub fn claude(projection: &ClaudeProjection) -> Contract {
    Contract {
        version: CONTRACT_VERSION,
        key: key_of(&json!({
            "app": "claude",
            "version": CONTRACT_VERSION,
            "env": sorted(&projection.env),
            "exclusive": sorted(&projection.exclusive),
        })),
        exclusive: projection.exclusive.clone(),
    }
}

/// Gemini CLI：代理地址、占位 Key 和路由供应商的模型名（都在投影里）。
pub fn gemini(projection: &GeminiProjection) -> Contract {
    Contract {
        version: CONTRACT_VERSION,
        key: key_of(&json!({
            "app": "gemini",
            "version": CONTRACT_VERSION,
            "projection": projection.to_value(),
        })),
        exclusive: Map::new(),
    }
}

/// Grok Build：路由供应商的整张模型表（地址、Key 已换成本地代理的）。模型表里其余的键
/// （模型名、窗口、推理摘要等）不同，客户端就要跟着改。
pub fn grok(projection: &GrokProjection) -> Contract {
    Contract {
        version: CONTRACT_VERSION,
        key: key_of(&json!({
            "app": "grokbuild",
            "version": CONTRACT_VERSION,
            "projection": projection.to_value(),
        })),
        exclusive: Map::new(),
    }
}

fn sorted(map: &Map<String, Value>) -> Value {
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    Value::Array(
        entries
            .into_iter()
            .map(|(key, value)| json!([key, value]))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::project::claude::{proxy_projection, ProxyAuth};

    fn contract_for(row: Value, url: &str) -> Contract {
        claude(&proxy_projection(
            &ClaudeProjection::of(&row),
            url,
            ProxyAuth::FollowRow,
            None,
        ))
    }

    #[test]
    fn providers_with_the_same_client_view_share_a_contract() {
        let a = json!({ "env": {
            "ANTHROPIC_BASE_URL": "https://a.example",
            "ANTHROPIC_AUTH_TOKEN": "sk-a",
            "ANTHROPIC_MODEL": "claude-sonnet-4-6"
        }});
        let b = json!({ "env": {
            "ANTHROPIC_BASE_URL": "https://b.example",
            "ANTHROPIC_AUTH_TOKEN": "sk-b",
            "ANTHROPIC_MODEL": "claude-sonnet-4-6"
        }});
        assert_eq!(
            contract_for(a, "http://127.0.0.1:15721").key,
            contract_for(b, "http://127.0.0.1:15721").key
        );
    }

    #[test]
    fn exclusive_fields_and_the_proxy_address_change_the_contract() {
        let plain = json!({ "env": { "ANTHROPIC_AUTH_TOKEN": "sk" }});
        let artifact_off = json!({ "env": {
            "ANTHROPIC_AUTH_TOKEN": "sk",
            "CLAUDE_CODE_DISABLE_ARTIFACT": "1"
        }});
        let base = contract_for(plain.clone(), "http://127.0.0.1:15721");
        assert_ne!(
            base.key,
            contract_for(artifact_off, "http://127.0.0.1:15721").key
        );
        assert_ne!(base.key, contract_for(plain, "http://127.0.0.1:15722").key);
    }

    #[test]
    fn the_contract_ignores_map_order() {
        let first = json!({ "env": {
            "ANTHROPIC_AUTH_TOKEN": "sk",
            "CLAUDE_CODE_DISABLE_ARTIFACT": "1",
            "CLAUDE_CODE_MAX_OUTPUT_TOKENS": "8192"
        }});
        let second = json!({ "env": {
            "CLAUDE_CODE_MAX_OUTPUT_TOKENS": "8192",
            "CLAUDE_CODE_DISABLE_ARTIFACT": "1",
            "ANTHROPIC_AUTH_TOKEN": "sk"
        }});
        assert_eq!(
            contract_for(first, "http://127.0.0.1:15721").key,
            contract_for(second, "http://127.0.0.1:15721").key
        );
    }
}
