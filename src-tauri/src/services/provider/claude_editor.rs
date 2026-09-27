//! Claude Code 供应商编辑器：底部 JSON 就是「切到这个供应商之后 settings.json 的样子」。
//!
//! - 显示：在内存里对当前 live 做一次切换投影，和切换用同一个补丁：关键字段、独有字段
//!   换成这个供应商的，其余部分是 live 原样。编辑当前的、非当前的、新增的供应商都一样。
//! - 保存：关键字段、独有字段写回这个供应商的行（行里其余内容原样保留）；其余部分的改动
//!   是 Claude Code 的全局设置，经引擎写进 live，只改用户动过的键。编辑的是直连模式下的
//!   当前供应商时，关键字段和独有字段在同一次写入里也换进 live。
//! - 三方比较：每个改动都带着打开编辑器时的原值。live 里这个键已经被别的程序改成了第三个
//!   值，就算冲突，由用户选保留哪一边；没改过的键不提交，也就不会覆盖别人的改动。

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::live::engine::read_current;
use crate::live::floor;
use crate::live::patch::json::{self as patch_json, JsonPatch};
use crate::live::patch::{KeyPath, LivePatch, LiveWriteError};
use crate::live::project::claude::{direct_patch, project_onto, store_into_row, ClaudeProjection};
use crate::mode::operation::{AppWrite, FileChange};
use crate::mode::state::{op, PendingTarget};
use crate::provider::Provider;
use crate::store::AppState;

use super::claude_direct;

/// 旧版存在 settings 里的内部字段：从不写进 live，也不算「不随切换生效的字段」。
const INTERNAL_TOP: &[&str] = &[
    "api_format",
    "apiFormat",
    "openrouter_compat_mode",
    "openrouterCompatMode",
];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorView {
    /// 编辑器里显示的完整配置。
    pub settings: Value,
    /// 行里保存着、但不随切换生效的字段（值和显示的不同才列出）。
    pub inactive: Vec<InactiveField>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InactiveField {
    /// 键路径，比如 `["env", "API_TIMEOUT_MS"]`、`["hooks"]`。
    pub path: Vec<String>,
    pub value: Value,
}

/// live 里的键在编辑期间被别的程序改过时怎么办。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictPolicy {
    /// 报冲突，什么都不写。
    #[default]
    Refuse,
    /// 用编辑器里的值覆盖。
    KeepMine,
    /// 冲突的键保留外部的值，其余改动照常写。
    KeepTheirs,
}

impl ConflictPolicy {
    /// 三方比较：`conflict` 给出冲突改动的显示名（live 里现在的值既不是打开编辑器时的，
    /// 也不是要写的）。有冲突时按策略整体拒绝、跳过冲突的改动或照写；返回要写的改动。
    pub(crate) fn resolve<'c, C>(
        self,
        path: &Path,
        changes: &'c [C],
        conflict: impl Fn(&C) -> Option<String>,
    ) -> Result<Vec<&'c C>, LiveWriteError> {
        let mut conflicts = Vec::new();
        let mut accepted = Vec::new();
        for change in changes {
            if let Some(key) = conflict(change) {
                conflicts.push(key);
                if self == Self::KeepTheirs {
                    continue;
                }
            }
            accepted.push(change);
        }
        if !conflicts.is_empty() && self == Self::Refuse {
            return Err(LiveWriteError::EditConflict {
                path: path.to_path_buf(),
                keys: conflicts,
            });
        }
        Ok(accepted)
    }
}

/// 编辑器保存时随供应商一起提交的上下文。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSave {
    /// 打开编辑器时显示的完整配置（新增对话框里是还没套预设的 live）。
    pub base: Value,
    /// 新增对话框（Codex、Gemini CLI、Grok Build）：投影成 `base` 的那份草稿（预设或模板）。
    /// 显示里有、草稿里没有的独有字段是从 live 带进来的，不归新供应商。
    #[serde(default)]
    pub draft: Option<Value>,
    #[serde(default)]
    pub on_conflict: ConflictPolicy,
}

/// 编辑器显示的内容。`settings_config` 是这个供应商的行（新增时是空对象）。
pub fn view(state: &AppState, settings_config: &Value) -> Result<EditorView, AppError> {
    let file = claude_direct::settings_file();
    let pre = read_current(&file.path)?;
    let (live, _) = patch_json::parse(&file.path, pre.as_deref())?;
    let prev = live_exclusive_owner(state)?;
    let settings = project_onto(
        &file.path,
        &live,
        prev.as_ref(),
        &ClaudeProjection::of(settings_config),
    )?;
    Ok(EditorView {
        inactive: inactive_fields(settings_config, &settings),
        settings,
    })
}

/// live 里的独有字段是谁带进来的：接上代理时是代理契约，否则是直连指针指向的供应商。
fn live_exclusive_owner(state: &AppState) -> Result<Option<ClaudeProjection>, AppError> {
    let mode = crate::mode::current::mode_state(&AppType::Claude);
    if mode.attached {
        if let Some(contract) = mode.contract {
            return Ok(Some(ClaudeProjection {
                exclusive: contract.exclusive,
                ..ClaudeProjection::default()
            }));
        }
    }
    Ok(
        crate::mode::current::direct_provider(&state.db, &AppType::Claude)?
            .map(|row| ClaudeProjection::of(&row.settings_config)),
    )
}

fn inactive_fields(row: &Value, display: &Value) -> Vec<InactiveField> {
    let mut fields = Vec::new();
    if let Some(root) = row.as_object() {
        for (key, value) in root {
            if key == "env" || floor::claude_floor_top(key) || INTERNAL_TOP.contains(&key.as_str())
            {
                continue;
            }
            if display.get(key) != Some(value) {
                fields.push(InactiveField {
                    path: vec![key.clone()],
                    value: value.clone(),
                });
            }
        }
    }
    if let Some(env) = row.get("env").and_then(Value::as_object) {
        for (key, value) in env {
            if floor::claude_floor_env(key) || floor::claude_exclusive_env(key) {
                continue;
            }
            if display.get("env").and_then(|env| env.get(key)) != Some(value) {
                fields.push(InactiveField {
                    path: vec!["env".to_string(), key.clone()],
                    value: value.clone(),
                });
            }
        }
    }
    fields
}

/// 编辑器里对 JSON 全局设置的一处改动。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct JsonChange {
    pub path: KeyPath,
    /// 打开编辑器时的值；`None` 表示当时没有这个键。
    pub before: Option<Value>,
    /// 保存的值；`None` 表示删掉。
    pub after: Option<Value>,
}

impl JsonChange {
    /// live 里这个位置现在是 `now`：既不是打开编辑器时的值，也不是要写的值。
    pub(crate) fn conflicts_with(&self, now: Option<&Value>) -> Option<String> {
        (now != self.before.as_ref() && now != self.after.as_ref()).then(|| self.path.to_string())
    }

    /// 把要写的改动收成一个补丁。
    pub(crate) fn patch(changes: &[&Self]) -> JsonPatch {
        let mut patch = JsonPatch::default();
        for change in changes {
            match &change.after {
                Some(value) => patch.set.push((change.path.clone(), value.clone())),
                None => patch.remove.push(change.path.clone()),
            }
        }
        patch
    }
}

/// 一次编辑器保存：存进行的内容，和要写进 live 的全局改动。
#[derive(Debug, Clone)]
pub(crate) struct EditorPlan {
    pub row_settings: Value,
    changes: Vec<JsonChange>,
}

/// 把编辑器里的完整配置拆开：关键字段、独有字段换进 `stored_row`（新增时为 `None`），
/// 其余部分和 `base` 比，得出用户改过的全局设置。
///
/// 显示里有、行里没有的独有字段是从 live 带进来的（用户自己写的，或者上一家留下、值被
/// 改过的），不归这个供应商：用户没动就不收进行，否则切走时会把用户自己的设置删掉；用户
/// 删了就从 live 删。新增对话框的 `base` 是还没套预设的 live，预设带的独有字段不在里面。
pub(crate) fn plan_save(
    stored_row: Option<&Value>,
    edited: &Value,
    base: &Value,
) -> Result<EditorPlan, AppError> {
    for (name, doc) in [("edited", edited), ("base", base)] {
        let shaped = doc.is_object()
            && doc
                .get("env")
                .is_none_or(|env| env.is_object() || env.is_null());
        if !shaped {
            return Err(AppError::localized(
                "provider.claude.editor.invalid_shape",
                format!("Claude 配置必须是 JSON 对象，env 也必须是对象（{name}）"),
                format!("Claude configuration and its env must be JSON objects ({name})"),
            ));
        }
    }
    let empty = Value::Object(Map::new());
    let stored = stored_row.unwrap_or(&empty);
    let env_value = |doc: &Value, key: &str| doc.get("env").and_then(|env| env.get(key)).cloned();
    let from_live: Vec<String> = base
        .get("env")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(Map::keys)
        .filter(|key| floor::claude_exclusive_env(key) && env_value(stored, key).is_none())
        .cloned()
        .collect();

    let mut projection = ClaudeProjection::of(edited);
    projection.exclusive.retain(|key, value| {
        !from_live.contains(key) || env_value(base, key).as_ref() != Some(value)
    });
    let removed_from_live: Vec<String> = from_live
        .into_iter()
        .filter(|key| env_value(edited, key).is_none())
        .collect();
    Ok(EditorPlan {
        row_settings: store_into_row(stored, &projection),
        changes: global_changes(base, edited, &removed_from_live),
    })
}

/// `removed_from_live`：用户删掉的、从 live 带进来的独有字段，从 live 删。其余独有字段
/// 归供应商行，不算全局改动。
fn global_changes(base: &Value, edited: &Value, removed_from_live: &[String]) -> Vec<JsonChange> {
    let top = |doc: &Value| doc.as_object().cloned().unwrap_or_default();
    let env = |doc: &Value| {
        doc.get("env")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default()
    };

    let mut changes = Vec::new();
    diff_level(
        &KeyPath::root(),
        &top(base),
        &top(edited),
        |key| key == "env" || floor::claude_floor_top(key) || INTERNAL_TOP.contains(&key),
        &mut changes,
    );
    diff_level(
        &KeyPath::new(&["env"]),
        &env(base),
        &env(edited),
        |key| {
            floor::claude_floor_env(key)
                || (floor::claude_exclusive_env(key)
                    && !removed_from_live.iter().any(|removed| removed == key))
        },
        &mut changes,
    );
    changes
}

/// 同一层里改过的键：先按原有顺序，新加的键按编辑器里的顺序排在后面。
fn diff_level(
    parent: &KeyPath,
    before: &Map<String, Value>,
    after: &Map<String, Value>,
    skip: impl Fn(&str) -> bool,
    changes: &mut Vec<JsonChange>,
) {
    let keys = before
        .keys()
        .chain(after.keys().filter(|key| !before.contains_key(*key)));
    for key in keys {
        if skip(key) || before.get(key) == after.get(key) {
            continue;
        }
        changes.push(JsonChange {
            path: parent.child(key),
            before: before.get(key).cloned(),
            after: after.get(key).cloned(),
        });
    }
}

struct EditorPatch<'a> {
    changes: &'a [JsonChange],
    on_conflict: ConflictPolicy,
    key_fields: Option<JsonPatch>,
}

impl LivePatch for EditorPatch<'_> {
    fn apply(&self, path: &Path, pre: Option<&[u8]>) -> Result<Vec<u8>, LiveWriteError> {
        let (mut doc, style) = patch_json::parse(path, pre)?;
        let accepted = self.on_conflict.resolve(path, self.changes, |change| {
            change.conflicts_with(patch_json::value_at(&doc, &change.path))
        })?;
        JsonChange::patch(&accepted).apply_to(path, &mut doc)?;
        if let Some(key_fields) = &self.key_fields {
            key_fields.apply_to(path, &mut doc)?;
        }
        patch_json::serialize(path, &doc, &style)
    }
}

/// 要不要在同一次写入里把关键字段也换进 live。
pub(crate) struct KeyFieldWrite<'a> {
    /// live 现在对应的那一版行（编辑前的行；新增第一个供应商时为 `None`）。
    pub prev: Option<&'a Provider>,
    pub target: &'a Provider,
    /// 同时把当前供应商改成 `target`（新增第一个供应商）。
    pub set_pointer: bool,
}

/// 把编辑器保存的改动写进 live。没有要写的就什么都不做。
pub(crate) fn write_live(
    db: &Database,
    plan: &EditorPlan,
    on_conflict: ConflictPolicy,
    key_fields: Option<KeyFieldWrite<'_>>,
) -> Result<(), AppError> {
    if plan.changes.is_empty() && key_fields.is_none() {
        return Ok(());
    }
    let pointer = key_fields
        .as_ref()
        .filter(|write| write.set_pointer)
        .map(|write| write.target.id.clone());
    let key_patch = key_fields.map(|write| {
        let prev = write
            .prev
            .map(|provider| ClaudeProjection::of(&provider.settings_config));
        direct_patch(
            prev.as_ref(),
            &ClaudeProjection::of(&write.target.settings_config),
        )
    });
    let patch = EditorPatch {
        changes: &plan.changes,
        on_conflict,
        key_fields: key_patch,
    };

    // 关键字段的补丁是按调用方读到的行算的：先补完上一次的操作。
    AppWrite::begin(db, AppType::Claude.as_str())?.run(
        if pointer.is_some() {
            op::SWITCH
        } else {
            op::APPLY
        },
        &[FileChange {
            file: claude_direct::settings_file(),
            patch: &patch,
        }],
        PendingTarget::pointer(pointer),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn apply(
        changes: &[JsonChange],
        policy: ConflictPolicy,
        live: &Value,
    ) -> Result<Value, LiveWriteError> {
        let patch = EditorPatch {
            changes,
            on_conflict: policy,
            key_fields: None,
        };
        let out = patch.apply(
            Path::new("settings.json"),
            Some(live.to_string().as_bytes()),
        )?;
        Ok(serde_json::from_slice(&out).expect("json"))
    }

    #[test]
    fn only_global_settings_count_as_live_changes() {
        let base = json!({
            "env": { "ANTHROPIC_BASE_URL": "https://a.example", "DEBUG": "1" },
            "hooks": { "Stop": [] }
        });
        let edited = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://b.example",
                "CLAUDE_CODE_DISABLE_ARTIFACT": "1",
                "EXTRA": "x"
            },
            "hooks": { "Stop": [] },
            "alwaysThinkingEnabled": false,
            "apiFormat": "openai_chat"
        });
        let changes = global_changes(&base, &edited, &[]);
        assert_eq!(
            changes,
            vec![
                JsonChange {
                    path: KeyPath::new(&["alwaysThinkingEnabled"]),
                    before: None,
                    after: Some(json!(false)),
                },
                JsonChange {
                    path: KeyPath::new(&["env", "DEBUG"]),
                    before: Some(json!("1")),
                    after: None,
                },
                JsonChange {
                    path: KeyPath::new(&["env", "EXTRA"]),
                    before: None,
                    after: Some(json!("x")),
                },
            ]
        );
    }

    #[test]
    fn saving_splits_the_row_from_the_global_settings() {
        let stored = json!({
            "env": { "ANTHROPIC_BASE_URL": "https://old.example", "API_TIMEOUT_MS": "300000" },
            "hooks": { "Stop": ["from backfill"] }
        });
        let base = json!({ "env": { "ANTHROPIC_BASE_URL": "https://old.example" } });
        let edited = json!({
            "env": { "ANTHROPIC_BASE_URL": "https://new.example", "DEBUG": "1" }
        });
        let plan = plan_save(Some(&stored), &edited, &base).expect("plan");
        assert_eq!(
            plan.row_settings,
            json!({
                "env": { "ANTHROPIC_BASE_URL": "https://new.example", "API_TIMEOUT_MS": "300000" },
                "hooks": { "Stop": ["from backfill"] }
            })
        );
        assert_eq!(plan.changes.len(), 1);

        let err = plan_save(None, &json!({ "env": "oops" }), &base).expect_err("bad shape");
        assert!(err.to_string().contains("env"), "{err}");
    }

    #[test]
    fn a_key_changed_elsewhere_is_a_conflict_unless_the_user_picks_a_side() {
        let changes = vec![
            JsonChange {
                path: KeyPath::new(&["x"]),
                before: Some(json!(1)),
                after: Some(json!(2)),
            },
            JsonChange {
                path: KeyPath::new(&["y"]),
                before: None,
                after: Some(json!("mine")),
            },
        ];
        // 窗口打开时 x=1，Claude Code 随后改成了 3；用户在旧窗口里改成 2。
        let live = json!({ "x": 3, "z": true });
        match apply(&changes, ConflictPolicy::Refuse, &live) {
            Err(LiveWriteError::EditConflict { keys, .. }) => assert_eq!(keys, vec!["x"]),
            other => panic!("expected a conflict, got {other:?}"),
        }
        assert_eq!(
            apply(&changes, ConflictPolicy::KeepMine, &live).unwrap(),
            json!({ "x": 2, "z": true, "y": "mine" })
        );
        assert_eq!(
            apply(&changes, ConflictPolicy::KeepTheirs, &live).unwrap(),
            json!({ "x": 3, "z": true, "y": "mine" })
        );
        // 外部已经改成了同样的值：不算冲突。
        assert_eq!(
            apply(&changes, ConflictPolicy::Refuse, &json!({ "x": 2 })).unwrap(),
            json!({ "x": 2, "y": "mine" })
        );
    }

    #[test]
    fn inactive_fields_list_row_values_that_would_not_show_up() {
        let row = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://a.example",
                "API_TIMEOUT_MS": "300000",
                "ENABLE_TOOL_SEARCH": "true"
            },
            "hooks": { "Stop": [] },
            "theme": "dark",
            "apiFormat": "anthropic"
        });
        let display = json!({
            "env": { "ANTHROPIC_BASE_URL": "https://a.example", "ENABLE_TOOL_SEARCH": "true" },
            "hooks": { "Stop": [] }
        });
        assert_eq!(
            inactive_fields(&row, &display),
            vec![
                InactiveField {
                    path: vec!["theme".into()],
                    value: json!("dark"),
                },
                InactiveField {
                    path: vec!["env".into(), "API_TIMEOUT_MS".into()],
                    value: json!("300000"),
                },
            ]
        );
    }
}
