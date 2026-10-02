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
///
/// 存量 Bedrock API Key 行（顶层 `apiKey`，`env` 里没有 `AWS_BEARER_TOKEN_BEDROCK`）的 Key
/// 仍存回顶层：投影把它挪进了 `env`，编辑器显示的也是 `env` 里的，但旧版的代理只从顶层
/// 读，存进 `env` 的话降级后代理模式就找不到 Key。
pub fn store_into_row(row: &Value, projection: &ClaudeProjection) -> Value {
    let legacy = legacy_bedrock_shape(row, projection);
    let projection = legacy.as_ref().unwrap_or(projection);
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

/// 行是存量 Bedrock API Key 的写法、存回的内容还是 Bedrock 带 Key 时，把 Key 放回顶层
/// `apiKey`（投影的反向转换）。
fn legacy_bedrock_shape(row: &Value, projection: &ClaudeProjection) -> Option<ClaudeProjection> {
    let legacy_row = row.get("apiKey").is_some()
        && row
            .get("env")
            .and_then(|env| env.get(BEDROCK_BEARER_ENV))
            .is_none();
    let bedrock = projection
        .env
        .get("CLAUDE_CODE_USE_BEDROCK")
        .is_some_and(is_truthy);
    if !legacy_row || !bedrock || projection.top.contains_key("apiKey") {
        return None;
    }
    let mut projection = projection.clone();
    let key = projection.env.shift_remove(BEDROCK_BEARER_ENV)?;
    projection.top.insert("apiKey".to_string(), key);
    Some(projection)
}

/// 代理模式下写进客户端的凭据占位符。旧版只认这个字面值来识别接管态，不能改。
pub const PROXY_TOKEN_PLACEHOLDER: &str = "PROXY_MANAGED";

/// 代理契约里的稳定模型别名：客户端只看到这几个名字，真实模型由代理映射。
const PROXY_HAIKU_ALIAS: &str = "claude-haiku-4-5";
const PROXY_SONNET_ALIAS: &str = "claude-sonnet-5";
const PROXY_OPUS_ALIAS: &str = "claude-opus-5";
const PROXY_FABLE_ALIAS: &str = "claude-fable-5";
// 写给 Claude Code 时沿用文档示例的大写形式；解析侧大小写不敏感。
pub(crate) const ONE_M_MARKER_FOR_CLIENT: &str = "[1M]";

/// 代理契约里怎么写凭据。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyAuth {
    /// 路由供应商的行里有 `ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_API_KEY` 就沿用同名键写占位
    /// 符，都没有就写 `ANTHROPIC_AUTH_TOKEN`。
    FollowRow,
    /// 托管账号（Copilot、Codex、xAI）：只写一个键，两个都在会触发 Claude Code 的
    /// 「Both ANTHROPIC_AUTH_TOKEN and ANTHROPIC_API_KEY set」警告（#4919）。
    /// - Codex 系要 `ANTHROPIC_AUTH_TOKEN`，缺了会弹登录提示（#3784）；
    /// - Copilot 默认也用 `ANTHROPIC_AUTH_TOKEN`：`ANTHROPIC_API_KEY` 占位会触发自定义 key
    ///   确认框，默认选项是拒绝，之后就是未登录；只有表单显式选了 `ANTHROPIC_API_KEY`
    ///   才用它，避开和 /login 的 key 冲突（#1049）。
    Managed { auth_token: bool },
}

/// Stack 模式下 Claude Code 四档别名都指向的模型：默认那家列表里的第一个
/// （`mode::stack::claude_route_default`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StackRoleModel<'a> {
    /// 发布给客户端的 Stack id，1M 模型带 `[1M]`。
    pub id: &'a str,
    /// 模型自己的显示名（不带供应商名）。
    pub name: &'a str,
}

/// 代理契约：代理模式下 `settings.json` 的关键字段和独有字段。
///
/// - 关键字段：本地代理地址、占位凭据、按角色写的模型；其余关键字段（协议选择器、云凭据、
///   `/model` 的选择等）一律清空，否则 Claude Code 会绕过代理；
///   - 路由模式（`stack_default` 为 `None`）：稳定的 `claude-*` 别名，显示名跟着路由供应商，
///     真实模型由代理映射；
///   - Stack 模式：四档都写 `stack_default` 的 Stack id，请求直达默认那家的这个模型；
/// - 独有字段：路由供应商的。它们在客户端发请求时生效，代理不能替它补上。
pub fn proxy_projection(
    route: &ClaudeProjection,
    proxy_url: &str,
    auth: ProxyAuth,
    stack_default: Option<StackRoleModel<'_>>,
) -> ClaudeProjection {
    let mut env = Map::new();
    env.insert(
        "ANTHROPIC_BASE_URL".to_string(),
        Value::String(proxy_url.to_string()),
    );
    let fields = match stack_default {
        Some(model) => stack_model_fields(model),
        None => proxy_model_fields(&route.env),
    };
    for (key, value) in fields {
        env.insert(key.to_string(), Value::String(value));
    }
    let placeholder = Value::String(PROXY_TOKEN_PLACEHOLDER.to_string());
    match auth {
        ProxyAuth::FollowRow => {
            let mut wrote_any = false;
            for key in ["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"] {
                if route.env.contains_key(key) {
                    env.insert(key.to_string(), placeholder.clone());
                    wrote_any = true;
                }
            }
            if !wrote_any {
                env.insert("ANTHROPIC_AUTH_TOKEN".to_string(), placeholder);
            }
        }
        ProxyAuth::Managed { auth_token } => {
            let key = if auth_token {
                "ANTHROPIC_AUTH_TOKEN"
            } else {
                "ANTHROPIC_API_KEY"
            };
            env.insert(key.to_string(), placeholder);
        }
    }
    ClaudeProjection {
        top: Map::new(),
        env,
        exclusive: route.exclusive.clone(),
    }
}

/// 按角色写的模型别名和显示名。
///
/// 回落顺序：haiku 用自己的、再用 `ANTHROPIC_SMALL_FAST_MODEL`、再用 `ANTHROPIC_MODEL`；
/// sonnet、opus 用自己的、再用 `ANTHROPIC_MODEL`、再用 `ANTHROPIC_SMALL_FAST_MODEL`；
/// fable 没配就不写（映射侧会 fable→opus 降级，和官方一致）。上游模型带 1M 标记时，
/// 别名也带上，Claude Code 才按 1M 计算窗口。
fn proxy_model_fields(env: &Map<String, Value>) -> Vec<(&'static str, String)> {
    let default_model = env_string(env, "ANTHROPIC_MODEL");
    let small_fast_model = env_string(env, "ANTHROPIC_SMALL_FAST_MODEL");
    let haiku = env_string(env, "ANTHROPIC_DEFAULT_HAIKU_MODEL")
        .or(small_fast_model)
        .or(default_model);
    let sonnet = env_string(env, "ANTHROPIC_DEFAULT_SONNET_MODEL")
        .or(default_model)
        .or(small_fast_model);
    let opus = env_string(env, "ANTHROPIC_DEFAULT_OPUS_MODEL")
        .or(default_model)
        .or(small_fast_model);
    let fable = env_string(env, "ANTHROPIC_DEFAULT_FABLE_MODEL");

    let mut fields = Vec::with_capacity(9);
    let roles = [
        (
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
            PROXY_HAIKU_ALIAS,
            false,
            haiku,
        ),
        (
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
            PROXY_SONNET_ALIAS,
            true,
            sonnet,
        ),
        (
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
            PROXY_OPUS_ALIAS,
            true,
            opus,
        ),
        (
            "ANTHROPIC_DEFAULT_FABLE_MODEL",
            "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
            PROXY_FABLE_ALIAS,
            true,
            fable,
        ),
    ];
    for (model_key, name_key, alias, supports_one_m, upstream) in roles {
        let Some(upstream) = upstream else {
            continue;
        };
        let mut client_model = alias.to_string();
        if supports_one_m && has_one_m_marker(upstream) {
            client_model.push_str(ONE_M_MARKER_FOR_CLIENT);
        }
        fields.push((model_key, client_model));
        let display_name = env_string(env, name_key)
            .map(str::to_string)
            .unwrap_or_else(|| {
                crate::proxy::model_mapper::strip_one_m_suffix_for_upstream(upstream)
                    .trim()
                    .to_string()
            });
        if !display_name.is_empty() {
            fields.push((name_key, display_name));
        }
    }
    if let Some(subagent) = env_string(env, "CLAUDE_CODE_SUBAGENT_MODEL") {
        fields.push(("CLAUDE_CODE_SUBAGENT_MODEL", subagent.to_string()));
    }
    fields
}

/// Stack 模式的四档：都写同一个 Stack id，显示名也一样。haiku 档不带 1M 标记（和路由契约
/// 一样，haiku 别名不写 1M；去掉标记的 id 解析到同一个模型）。不写
/// `CLAUDE_CODE_SUBAGENT_MODEL`：子代理跟随主模型，也就是用户在 `/model` 里选的。
fn stack_model_fields(model: StackRoleModel<'_>) -> Vec<(&'static str, String)> {
    let haiku = model
        .id
        .strip_suffix(ONE_M_MARKER_FOR_CLIENT)
        .unwrap_or(model.id);
    let roles = [
        (
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
            haiku,
        ),
        (
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
            model.id,
        ),
        (
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
            model.id,
        ),
        (
            "ANTHROPIC_DEFAULT_FABLE_MODEL",
            "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
            model.id,
        ),
    ];
    let name = model.name.trim();
    let mut fields = Vec::with_capacity(roles.len() * 2);
    for (model_key, name_key, id) in roles {
        fields.push((model_key, id.to_string()));
        if !name.is_empty() {
            fields.push((name_key, name.to_string()));
        }
    }
    fields
}

pub(crate) fn env_string<'a>(env: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    env.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(crate) fn has_one_m_marker(model: &str) -> bool {
    model
        .trim_end()
        .to_ascii_lowercase()
        .ends_with(crate::claude_desktop_config::ONE_M_CONTEXT_MARKER)
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

    #[test]
    fn a_legacy_bedrock_row_keeps_its_key_at_the_top_level() {
        let row = json!({
            "apiKey": "old-key",
            "env": { "CLAUDE_CODE_USE_BEDROCK": "1", "AWS_REGION": "us-east-1" }
        });
        let projected = ClaudeProjection::of(&row);
        assert_eq!(projected.env[BEDROCK_BEARER_ENV], "old-key");
        assert_eq!(store_into_row(&row, &projected), row, "round trip");

        // 编辑器里改了 Key：新值还存回顶层。
        let mut edited = projected.clone();
        edited
            .env
            .insert(BEDROCK_BEARER_ENV.to_string(), json!("new-key"));
        let stored = store_into_row(&row, &edited);
        assert_eq!(stored["apiKey"], "new-key");
        assert!(stored["env"].get(BEDROCK_BEARER_ENV).is_none());

        // 删了 Key、不再用 Bedrock、或者行本来就是 env 写法：按投影存。
        let mut removed = projected.clone();
        removed.env.shift_remove(BEDROCK_BEARER_ENV);
        assert!(store_into_row(&row, &removed).get("apiKey").is_none());
        let mut not_bedrock = projected.clone();
        not_bedrock.env.shift_remove("CLAUDE_CODE_USE_BEDROCK");
        let stored = store_into_row(&row, &not_bedrock);
        assert!(stored.get("apiKey").is_none());
        assert_eq!(stored["env"][BEDROCK_BEARER_ENV], "old-key");
        let env_row = json!({ "env": {
            "CLAUDE_CODE_USE_BEDROCK": "1",
            BEDROCK_BEARER_ENV: "env-key"
        }});
        assert_eq!(
            store_into_row(&env_row, &ClaudeProjection::of(&env_row)),
            env_row
        );
    }

    #[test]
    fn stack_mode_points_every_alias_at_the_default_model() {
        let row = json!({ "env": {
            "ANTHROPIC_AUTH_TOKEN": "sk",
            "ANTHROPIC_MODEL": "glm-5.2[1M]",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "glm-4.7-air",
            "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME": "ignored",
            "CLAUDE_CODE_SUBAGENT_MODEL": "glm-4.7-air"
        }});
        let route = ClaudeProjection::of(&row);
        let stacked = proxy_projection(
            &route,
            "http://127.0.0.1:15721",
            ProxyAuth::FollowRow,
            Some(StackRoleModel {
                id: "ccs-claude-z--glm-5.2[1M]",
                name: "GLM 5.2",
            }),
        );
        let env = |key: &str| stacked.env.get(key).and_then(Value::as_str);
        for role in ["SONNET", "OPUS", "FABLE"] {
            assert_eq!(
                env(&format!("ANTHROPIC_DEFAULT_{role}_MODEL")),
                Some("ccs-claude-z--glm-5.2[1M]")
            );
        }
        assert_eq!(
            env("ANTHROPIC_DEFAULT_HAIKU_MODEL"),
            Some("ccs-claude-z--glm-5.2")
        );
        for role in ["HAIKU", "SONNET", "OPUS", "FABLE"] {
            assert_eq!(
                env(&format!("ANTHROPIC_DEFAULT_{role}_MODEL_NAME")),
                Some("GLM 5.2")
            );
        }
        assert_eq!(env("CLAUDE_CODE_SUBAGENT_MODEL"), None);
        assert_eq!(env("ANTHROPIC_AUTH_TOKEN"), Some(PROXY_TOKEN_PLACEHOLDER));

        // 路由模式照旧写 `claude-*` 别名和行里的子代理模型。
        let routed = proxy_projection(&route, "http://127.0.0.1:15721", ProxyAuth::FollowRow, None);
        assert_eq!(
            routed.env["ANTHROPIC_DEFAULT_SONNET_MODEL"],
            "claude-sonnet-5[1M]"
        );
        assert_eq!(routed.env["CLAUDE_CODE_SUBAGENT_MODEL"], "glm-4.7-air");
    }
}
