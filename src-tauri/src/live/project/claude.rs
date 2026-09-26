//! Claude Code 的直连投影：供应商行 → `settings.json` 的关键字段和独有字段。
//!
//! 行里其余的键（旧版回填进来的插件、hooks，深链带进来的超时设置等）不投影：它们归
//! 用户和客户端，新版只从 live 里读写它们。

use std::path::Path;

use serde_json::{Map, Value};

use crate::live::floor;
use crate::live::patch::json::{ClearScope, JsonPatch};
use crate::live::patch::{KeyPath, LiveWriteError};
use crate::live::residue;

/// 旧 Bedrock API Key 预设把 Key 写在顶层 `apiKey`，Claude Code 读的是这个变量。
const BEDROCK_BEARER_ENV: &str = "AWS_BEARER_TOKEN_BEDROCK";

/// 一个供应商在 `settings.json` 里拥有的键。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClaudeProjection {
    /// 顶层的关键字段（`model`、`apiKeyHelper` 等）。
    pub top: Map<String, Value>,
    /// `env` 里的关键字段（地址、凭据、模型名、协议选择器）。
    pub env: Map<String, Value>,
    /// `env` 里的供应商独有字段（兼容开关、窗口值）。
    pub exclusive: Map<String, Value>,
}

impl ClaudeProjection {
    /// 从供应商行（或编辑器里的完整配置）取出关键字段和独有字段。
    ///
    /// 存量的 Bedrock API Key 行在这里转换：选了 Bedrock、顶层有 `apiKey` 时，投影成
    /// `env.AWS_BEARER_TOKEN_BEDROCK`（`env` 里已有就以它为准），行本身不改写。
    pub fn of(settings: &Value) -> Self {
        let mut projection = Self::default();
        if let Some(root) = settings.as_object() {
            for (key, value) in root {
                if floor::claude_floor_top(key) {
                    projection.top.insert(key.clone(), value.clone());
                }
            }
        }
        if let Some(env) = settings.get("env").and_then(Value::as_object) {
            for (key, value) in env {
                if floor::claude_floor_env(key) {
                    projection.env.insert(key.clone(), value.clone());
                } else if floor::claude_exclusive_env(key) {
                    projection.exclusive.insert(key.clone(), value.clone());
                }
            }
        }

        if projection
            .env
            .get("CLAUDE_CODE_USE_BEDROCK")
            .is_some_and(is_truthy)
        {
            if let Some(key) = projection.top.shift_remove("apiKey") {
                projection
                    .env
                    .entry(BEDROCK_BEARER_ENV.to_string())
                    .or_insert(key);
            }
        }
        projection
    }

    fn set_entries(&self) -> Vec<(KeyPath, Value)> {
        let env = KeyPath::new(&["env"]);
        self.top
            .iter()
            .map(|(key, value)| (KeyPath::root().child(key), value.clone()))
            .chain(
                self.env
                    .iter()
                    .chain(&self.exclusive)
                    .map(|(key, value)| (env.child(key), value.clone())),
            )
            .collect()
    }
}

/// 以 live 为底切到 `target`：
/// - 关键字段：一律清空，再写 `target` 的；
/// - 独有字段：先删 `prev` 带进来、而且值没被改过的，再写 `target` 的；
/// - 残留清理：删掉旧版下发过的有害窗口值，`target` 自己要写的键除外。
///
/// `prev` 是 live 当前对应的供应商（直连指针指向的那家）；没有就只做残留清理。
pub fn direct_patch(prev: Option<&ClaudeProjection>, target: &ClaudeProjection) -> JsonPatch {
    let env = KeyPath::new(&["env"]);
    let outgoing = prev
        .into_iter()
        .flat_map(|prev| &prev.exclusive)
        .map(|(key, value)| (env.child(key), vec![value.clone()]));
    let residue = residue::CLAUDE_RESIDUE_ENV
        .iter()
        .map(|(key, values)| (env.child(key), residue::residue_values(values)));

    JsonPatch {
        clear: vec![
            ClearScope {
                parent: KeyPath::root(),
                is_floor: floor::claude_floor_top,
            },
            ClearScope {
                parent: env.clone(),
                is_floor: floor::claude_floor_env,
            },
        ],
        set: target.set_entries(),
        remove_if: outgoing.chain(residue).collect(),
        ..JsonPatch::default()
    }
}

/// 在内存里算出「切到 `target` 之后 `settings.json` 会是什么样」，不写盘。
/// 编辑器显示和切换用的是同一个补丁。
pub fn project_onto(
    path: &Path,
    live: &Value,
    prev: Option<&ClaudeProjection>,
    target: &ClaudeProjection,
) -> Result<Value, LiveWriteError> {
    let mut doc = live.clone();
    direct_patch(prev, target).apply_to(path, &mut doc)?;
    Ok(doc)
}

/// 把关键字段和独有字段存回供应商行：行里这两类键换成 `projection` 的，其余内容原样
/// 保留（降级后旧版会整份使用这些行）。
pub fn store_into_row(row: &Value, projection: &ClaudeProjection) -> Value {
    let mut row = if row.is_object() {
        row.clone()
    } else {
        Value::Object(Map::new())
    };
    let env = KeyPath::new(&["env"]);
    let patch = JsonPatch {
        clear: vec![
            ClearScope {
                parent: KeyPath::root(),
                is_floor: floor::claude_floor_top,
            },
            ClearScope {
                parent: env.clone(),
                is_floor: floor::claude_floor_env,
            },
            ClearScope {
                parent: env,
                is_floor: floor::claude_exclusive_env,
            },
        ],
        set: projection.set_entries(),
        ..JsonPatch::default()
    };
    if patch
        .apply_to(Path::new("provider settings"), &mut row)
        .is_err()
    {
        // 行里的 `env` 不是对象：整个换成投影出来的 env。
        if let Some(root) = row.as_object_mut() {
            root.insert("env".to_string(), Value::Object(Map::new()));
        }
        patch
            .apply_to(Path::new("provider settings"), &mut row)
            .expect("env is an object now");
    }
    row
}

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        Value::String(text) => matches!(
            text.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn project(live: &Value, prev: Option<&Value>, target: &Value) -> Value {
        let prev = prev.map(ClaudeProjection::of);
        project_onto(
            Path::new("settings.json"),
            live,
            prev.as_ref(),
            &ClaudeProjection::of(target),
        )
        .expect("project")
    }

    fn qwen() -> Value {
        json!({ "env": {
            "ANTHROPIC_BASE_URL": "https://qwen.example",
            "ANTHROPIC_AUTH_TOKEN": "sk-qwen",
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "983616",
            "API_TIMEOUT_MS": "300000"
        }})
    }

    fn kimi() -> Value {
        json!({ "env": {
            "ANTHROPIC_BASE_URL": "https://kimi.example",
            "ANTHROPIC_AUTH_TOKEN": "sk-kimi",
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "262144",
            "CLAUDE_CODE_DISABLE_ARTIFACT": "1"
        }})
    }

    #[test]
    fn projection_takes_key_and_exclusive_fields_only() {
        let row = json!({
            "model": "picked",
            "hooks": { "Stop": [] },
            "env": {
                "ANTHROPIC_BASE_URL": "https://a.example",
                "CLAUDE_CODE_USE_POWERSHELL_TOOL": "1",
                "ENABLE_TOOL_SEARCH": "true",
                "API_TIMEOUT_MS": "300000"
            }
        });
        let projection = ClaudeProjection::of(&row);
        assert_eq!(
            projection.top,
            json!({ "model": "picked" }).as_object().unwrap().clone()
        );
        assert_eq!(
            Value::Object(projection.env),
            json!({ "ANTHROPIC_BASE_URL": "https://a.example" })
        );
        assert_eq!(
            Value::Object(projection.exclusive),
            json!({ "ENABLE_TOOL_SEARCH": "true" })
        );
    }

    #[test]
    fn legacy_bedrock_api_key_becomes_the_bearer_env() {
        let legacy = json!({
            "apiKey": "legacy-key",
            "env": { "CLAUDE_CODE_USE_BEDROCK": "1", "AWS_REGION": "us-west-2" }
        });
        let projection = ClaudeProjection::of(&legacy);
        assert!(projection.top.is_empty());
        assert_eq!(projection.env[BEDROCK_BEARER_ENV], json!("legacy-key"));

        let both = json!({
            "apiKey": "stale",
            "env": { "CLAUDE_CODE_USE_BEDROCK": "1", BEDROCK_BEARER_ENV: "fresh" }
        });
        assert_eq!(
            ClaudeProjection::of(&both).env[BEDROCK_BEARER_ENV],
            json!("fresh")
        );

        // 没选 Bedrock 时顶层 apiKey 原样投影。
        let plain = json!({ "apiKey": "k" });
        assert_eq!(ClaudeProjection::of(&plain).top["apiKey"], json!("k"));
    }

    #[test]
    fn switching_replaces_key_fields_and_keeps_everything_else_in_place() {
        let live = json!({
            "model": "qwen-picked",
            "env": {
                "ANTHROPIC_BASE_URL": "https://qwen.example",
                "CLAUDE_CODE_USE_POWERSHELL_TOOL": "1",
                "ANTHROPIC_AUTH_TOKEN": "sk-qwen",
                "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "983616",
                "API_TIMEOUT_MS": "300000"
            },
            "hooks": { "Stop": [] }
        });
        let out = project(&live, Some(&qwen()), &kimi());
        assert_eq!(
            serde_json::to_string(&out).unwrap(),
            serde_json::to_string(&json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://kimi.example",
                    "CLAUDE_CODE_USE_POWERSHELL_TOOL": "1",
                    "ANTHROPIC_AUTH_TOKEN": "sk-kimi",
                    "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "262144",
                    "API_TIMEOUT_MS": "300000",
                    "CLAUDE_CODE_DISABLE_ARTIFACT": "1"
                },
                "hooks": { "Stop": [] }
            }))
            .unwrap()
        );
    }

    #[test]
    fn exclusive_fields_leave_only_when_unchanged() {
        let official = json!({ "env": {} });
        let live = json!({ "env": {
            "ANTHROPIC_BASE_URL": "https://kimi.example",
            "CLAUDE_CODE_DISABLE_ARTIFACT": "1",
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "262144"
        }});
        assert_eq!(
            project(&live, Some(&kimi()), &official),
            json!({ "env": {} })
        );

        // 用户在 live 里把它改成了 0：不是 CC Switch 写的，保留。
        let edited = json!({ "env": { "CLAUDE_CODE_DISABLE_ARTIFACT": "0" } });
        assert_eq!(
            project(&edited, Some(&kimi()), &official),
            json!({ "env": { "CLAUDE_CODE_DISABLE_ARTIFACT": "0" } })
        );
    }

    #[test]
    fn residue_goes_but_the_targets_own_value_stays_in_place() {
        // 旧版给 Kimi 注入的 262144，上一家行里没有：残留清理兜住。
        let live = json!({ "env": {
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "262144",
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW": 262144,
            "DEBUG": "1"
        }});
        let bare_kimi = json!({ "env": { "ANTHROPIC_BASE_URL": "https://kimi.example" } });
        assert_eq!(
            project(&live, Some(&bare_kimi), &json!({})),
            json!({ "env": { "DEBUG": "1" } })
        );

        // 切入千问：它自己要写 983616，不能被残留清理删掉，也不挪位置。
        let live = json!({ "env": {
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "983616",
            "DEBUG": "1"
        }});
        let out = project(&live, None, &qwen());
        assert_eq!(
            out["env"]
                .as_object()
                .unwrap()
                .keys()
                .next()
                .map(String::as_str),
            Some("CLAUDE_CODE_MAX_CONTEXT_TOKENS")
        );
        assert_eq!(
            out["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("983616")
        );
    }

    #[test]
    fn user_window_values_that_cc_switch_never_sent_are_kept() {
        let live = json!({ "env": { "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "500000" } });
        assert_eq!(project(&live, None, &json!({})), live);
    }

    #[test]
    fn storing_into_a_row_keeps_its_other_content() {
        let row = json!({
            "hooks": { "Stop": [] },
            "apiKey": "legacy",
            "env": {
                "ANTHROPIC_BASE_URL": "https://old.example",
                "API_TIMEOUT_MS": "300000",
                "CLAUDE_CODE_DISABLE_ARTIFACT": "1"
            }
        });
        let edited = ClaudeProjection::of(&json!({ "env": {
            "ANTHROPIC_BASE_URL": "https://new.example",
            "ENABLE_TOOL_SEARCH": "true"
        }}));
        assert_eq!(
            store_into_row(&row, &edited),
            json!({
                "hooks": { "Stop": [] },
                "env": {
                    "ANTHROPIC_BASE_URL": "https://new.example",
                    "API_TIMEOUT_MS": "300000",
                    "ENABLE_TOOL_SEARCH": "true"
                }
            })
        );
        assert_eq!(
            store_into_row(&json!({ "env": "oops" }), &edited)["env"],
            json!({ "ANTHROPIC_BASE_URL": "https://new.example", "ENABLE_TOOL_SEARCH": "true" })
        );
    }
}
