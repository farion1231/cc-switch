//! Gemini CLI 供应商编辑器：底部的环境变量和 settings.json 就是「切到这个供应商之后这两个
//! 文件的样子」。
//!
//! - 显示：在内存里对当前 live 做一次切换投影（和切换用同一个补丁）：`.env` 的关键字段换成
//!   这个供应商的，`settings.json` 只换认证方式和模型名，其余是 live 原样。
//! - 保存：关键字段写回这个供应商的行（行里其余内容原样保留）；其余改动是 Gemini CLI 的全局
//!   设置，经引擎写进 live，只改用户动过的键。编辑的是直连模式下的当前供应商时，关键字段
//!   在同一次写入里也换进 live。
//! - 三方比较：每个改动都带着打开编辑器时的原值，live 里已经被别的程序改成了第三个值就算
//!   冲突，由用户选保留哪一边。
//!
//! 改动的粒度：`.env` 按变量；settings.json 按叶子（两边都是对象就往下比）。认证方式和模型名
//! 是关键字段，不算全局改动；认证方式由供应商类型决定，编辑器里改了也不存。

use std::path::Path;

use serde_json::{Map, Value};

use crate::database::Database;
use crate::error::AppError;
use crate::live::engine::read_current;
use crate::live::floor;
use crate::live::patch::dotenv::{self, DotenvPatch};
use crate::live::patch::json::{self as patch_json, value_at, JsonPatch};
use crate::live::patch::{KeyPath, LivePatch, LiveWriteError};
use crate::live::project::gemini::GeminiProjection;
use crate::mode::state::{op, PendingTarget};
use crate::provider::Provider;
use crate::store::AppState;

use super::claude_editor::{ConflictPolicy, EditorView, InactiveField, JsonChange};
use super::gemini_direct;

fn floor_paths() -> impl Iterator<Item = KeyPath> {
    floor::GEMINI_FLOOR_SETTINGS
        .iter()
        .map(|segments| KeyPath::new(segments))
}

fn env_of(settings: &Value) -> Map<String, Value> {
    settings
        .get("env")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

fn config_of(settings: &Value) -> Value {
    match settings.get("config") {
        Some(config) if config.is_object() => config.clone(),
        _ => Value::Object(Map::new()),
    }
}

/// 编辑器显示的内容。`settings_config` 是这个供应商的行；`category` 用来认出官方卡。
pub fn view(
    _state: &AppState,
    settings_config: &Value,
    category: Option<&str>,
) -> Result<EditorView, AppError> {
    let projection = GeminiProjection::of(settings_config, category == Some("official"));

    let env_file = gemini_direct::env_file();
    let env_pre = read_current(&env_file.path)?;
    let env_bytes = projection
        .env_patch()
        .apply(&env_file.path, env_pre.as_deref())?;
    let env: Map<String, Value> = dotenv::entries(&String::from_utf8_lossy(&env_bytes))
        .into_iter()
        .map(|(key, value)| (key, Value::String(value)))
        .collect();

    let settings_file = gemini_direct::settings_file();
    let settings_pre = read_current(&settings_file.path)?;
    let (mut config, _) = patch_json::parse(&settings_file.path, settings_pre.as_deref())?;
    projection
        .settings_patch()
        .apply_to(&settings_file.path, &mut config)?;

    let mut shown = settings_config.as_object().cloned().unwrap_or_default();
    shown.insert("env".to_string(), Value::Object(env.clone()));
    shown.insert("config".to_string(), config.clone());
    Ok(EditorView {
        inactive: inactive_fields(settings_config, &env, &config),
        settings: Value::Object(shown),
    })
}

/// 行里保存着、但不随切换生效的设置（值和显示的不同才列出）：非关键的环境变量，以及
/// settings.json 的顶层键（去掉关键字段后比）。
fn inactive_fields(row: &Value, env: &Map<String, Value>, config: &Value) -> Vec<InactiveField> {
    let mut fields = Vec::new();
    for (key, value) in env_of(row) {
        if floor::gemini_floor_env(&key) || !value.is_string() {
            continue;
        }
        if env.get(&key) != Some(&value) {
            fields.push(InactiveField {
                path: vec!["env".to_string(), key],
                value,
            });
        }
    }
    let row_config = strip_floor(&KeyPath::root(), &config_of(row)).unwrap_or_default();
    let shown = strip_floor(&KeyPath::root(), config).unwrap_or_default();
    if let Some(row_config) = row_config.as_object() {
        for (key, value) in row_config {
            if shown.get(key) != Some(value) {
                fields.push(InactiveField {
                    path: vec!["config".to_string(), key.clone()],
                    value: value.clone(),
                });
            }
        }
    }
    fields
}

/// `value` 位于 `at`；去掉它下面的关键字段，因此变空的对象一并去掉。`value` 自己因此
/// 变空时返回 `None`：和打开编辑器时一样，只剩关键字段的对象算不存在。
fn strip_floor(at: &KeyPath, value: &Value) -> Option<Value> {
    let mut value = value.clone();
    for floor_path in floor_paths() {
        let Some(rest) = floor_path.0.strip_prefix(at.0.as_slice()) else {
            continue;
        };
        if rest.is_empty() || remove_pruning(&mut value, rest) {
            return None;
        }
    }
    Some(value)
}

/// 删掉 `path`，并把因此变空的父对象一起删掉。返回 `value` 自己是否因此变空。
fn remove_pruning(value: &mut Value, path: &[String]) -> bool {
    let Some((first, rest)) = path.split_first() else {
        return false;
    };
    let Some(map) = value.as_object_mut() else {
        return false;
    };
    if rest.is_empty() {
        if map.shift_remove(first).is_none() {
            return false;
        }
    } else {
        let Some(child) = map.get_mut(first) else {
            return false;
        };
        if !remove_pruning(child, rest) {
            return false;
        }
        map.shift_remove(first);
    }
    map.is_empty()
}

/// 把关键字段放回原处（缺的父对象补上）；用户把某一层改成了非对象时放不回去，跳过。
fn restore(doc: &mut Value, path: &[String], value: Value) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut current = doc;
    for segment in parents {
        let Some(map) = current.as_object_mut() else {
            return;
        };
        current = map
            .entry(segment.clone())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    if let Some(map) = current.as_object_mut() {
        map.insert(last.clone(), value);
    }
}

/// `.env` 里一个变量的改动。
#[derive(Debug, Clone, PartialEq)]
struct EnvChange {
    key: String,
    /// 打开编辑器时的值；`None` 表示当时没有。
    before: Option<String>,
    /// 保存的值；`None` 表示删掉。
    after: Option<String>,
}

/// 一次编辑器保存要写进 live 的全局改动。
#[derive(Debug, Clone)]
pub(crate) struct GeminiEdits {
    env: Vec<EnvChange>,
    settings: Vec<JsonChange>,
    on_conflict: ConflictPolicy,
}

impl GeminiEdits {
    pub(crate) fn touches_env(&self) -> bool {
        !self.env.is_empty()
    }

    pub(crate) fn touches_settings(&self) -> bool {
        !self.settings.is_empty()
    }
}

fn env_changes(base: &Map<String, Value>, edited: &Map<String, Value>) -> Vec<EnvChange> {
    let text = |map: &Map<String, Value>, key: &str| {
        map.get(key).and_then(Value::as_str).map(str::to_string)
    };
    base.keys()
        .chain(edited.keys().filter(|key| !base.contains_key(*key)))
        .filter(|key| !floor::gemini_floor_env(key))
        .filter_map(|key| {
            let before = text(base, key);
            let after = text(edited, key);
            (before != after).then(|| EnvChange {
                key: key.clone(),
                before,
                after,
            })
        })
        .collect()
}

fn json_changes(base: &Value, edited: &Value) -> Vec<JsonChange> {
    let object = |doc: &Value| {
        strip_floor(&KeyPath::root(), doc)
            .and_then(|doc| doc.as_object().cloned())
            .unwrap_or_default()
    };
    let mut changes = Vec::new();
    diff_objects(
        &KeyPath::root(),
        &object(base),
        &object(edited),
        &mut changes,
    );
    changes
}

/// 两边都是对象就往下比；否则整个值算一处改动。先按原有顺序，新加的键排在后面。
fn diff_objects(
    parent: &KeyPath,
    before: &Map<String, Value>,
    after: &Map<String, Value>,
    changes: &mut Vec<JsonChange>,
) {
    let keys = before
        .keys()
        .chain(after.keys().filter(|key| !before.contains_key(*key)));
    for key in keys {
        match (before.get(key), after.get(key)) {
            (Some(Value::Object(old)), Some(Value::Object(new))) => {
                diff_objects(&parent.child(key), old, new, changes)
            }
            (old, new) if old != new => changes.push(JsonChange {
                path: parent.child(key),
                before: old.cloned(),
                after: new.cloned(),
            }),
            _ => {}
        }
    }
}

/// 写 `.env`：先按三方比较应用编辑器的改动，再换关键字段。
pub(crate) struct EnvWrite<'a> {
    pub edits: Option<&'a GeminiEdits>,
    pub key_fields: Option<DotenvPatch>,
}

impl LivePatch for EnvWrite<'_> {
    fn apply(&self, path: &Path, pre: Option<&[u8]>) -> Result<Vec<u8>, LiveWriteError> {
        let mut bytes = pre.map(<[u8]>::to_vec);
        if let Some(edits) = self.edits.filter(|edits| edits.touches_env()) {
            let text = match pre {
                Some(bytes) => crate::live::patch::decode_utf8(path, bytes)?,
                None => "",
            };
            let current = dotenv::entries(text);
            let current_of = |key: &str| {
                current
                    .iter()
                    .find(|(existing, _)| existing == key)
                    .map(|(_, value)| value.clone())
            };
            let accepted = edits.on_conflict.resolve(path, &edits.env, |change| {
                let now = current_of(&change.key);
                (now != change.before && now != change.after)
                    .then(|| format!(".env {}", change.key))
            })?;
            let mut patch = DotenvPatch::default();
            for change in accepted {
                match &change.after {
                    Some(value) => patch.set.push((change.key.clone(), value.clone())),
                    None => patch.remove.push(change.key.clone()),
                }
            }
            bytes = Some(patch.apply(path, bytes.as_deref())?);
        }
        match &self.key_fields {
            Some(key_fields) => key_fields.apply(path, bytes.as_deref()),
            None => Ok(bytes.unwrap_or_default()),
        }
    }
}

/// 写 `settings.json`：先按三方比较应用编辑器的改动（改动整片替换一个对象时，里面的
/// 关键字段保留 live 原来的值），再换关键字段。
pub(crate) struct SettingsWrite<'a> {
    pub edits: Option<&'a GeminiEdits>,
    pub key_fields: Option<JsonPatch>,
}

impl LivePatch for SettingsWrite<'_> {
    fn apply(&self, path: &Path, pre: Option<&[u8]>) -> Result<Vec<u8>, LiveWriteError> {
        let (mut doc, style) = patch_json::parse(path, pre)?;
        if let Some(edits) = self.edits.filter(|edits| edits.touches_settings()) {
            let accepted = edits.on_conflict.resolve(path, &edits.settings, |change| {
                let now =
                    value_at(&doc, &change.path).and_then(|value| strip_floor(&change.path, value));
                change.conflicts_with(now.as_ref())
            })?;
            let patch = JsonChange::patch(&accepted);
            let kept: Vec<(KeyPath, Value)> = floor_paths()
                .filter_map(|floor_path| {
                    let value = value_at(&doc, &floor_path)?.clone();
                    Some((floor_path, value))
                })
                .collect();
            patch.apply_to(path, &mut doc)?;
            for (floor_path, value) in kept {
                restore(&mut doc, &floor_path.0, value);
            }
        }
        if let Some(key_fields) = &self.key_fields {
            key_fields.apply_to(path, &mut doc)?;
        }
        patch_json::serialize(path, &doc, &style)
    }
}

/// 一次编辑器保存：存进行的内容，和要写进 live 的全局改动。
pub(crate) struct GeminiEditorPlan {
    pub row_settings: Value,
    pub edits: GeminiEdits,
}

/// 把编辑器里的配置拆开：关键字段换进行（行里其余内容原样保留），其余部分和 `base` 比，
/// 得出用户改过的全局设置。
pub(crate) fn plan_save(
    stored_row: Option<&Value>,
    edited: &Value,
    base: &Value,
    on_conflict: ConflictPolicy,
) -> Result<GeminiEditorPlan, AppError> {
    for (name, doc) in [("edited", edited), ("base", base)] {
        crate::gemini_config::validate_gemini_settings(doc).map_err(|err| {
            AppError::localized(
                "provider.gemini.editor.invalid_shape",
                format!("Gemini 配置格式错误（{name}）：{err}"),
                format!("Invalid Gemini configuration ({name}): {err}"),
            )
        })?;
    }
    Ok(GeminiEditorPlan {
        row_settings: store_into_row(stored_row, edited),
        edits: GeminiEdits {
            env: env_changes(&env_of(base), &env_of(edited)),
            settings: json_changes(&config_of(base), &config_of(edited)),
            on_conflict,
        },
    })
}

/// 把编辑器里的关键字段存回行：`env` 的关键字段、`config.model.name` 换成编辑器的，其余
/// 内容原样保留（降级后旧版会整份使用这些行）。认证方式由供应商类型决定，不存。
fn store_into_row(stored_row: Option<&Value>, edited: &Value) -> Value {
    let mut row = stored_row
        .filter(|row| row.is_object())
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let root = row.as_object_mut().expect("row is an object");

    let mut env = root
        .get("env")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    env.retain(|key, _| !floor::gemini_floor_env(key));
    for (key, value) in env_of(edited) {
        if floor::gemini_floor_env(&key) && value.is_string() {
            env.insert(key, value);
        }
    }
    root.insert("env".to_string(), Value::Object(env));

    let model_name = edited
        .pointer("/config/model/name")
        .filter(|name| !name.is_null())
        .cloned();
    let mut config = root
        .get("config")
        .filter(|config| config.is_object())
        .cloned();
    match model_name {
        Some(name) => {
            let config = config.get_or_insert_with(|| Value::Object(Map::new()));
            let model = config
                .as_object_mut()
                .expect("config is an object")
                .entry("model")
                .or_insert_with(|| Value::Object(Map::new()));
            if !model.is_object() {
                *model = Value::Object(Map::new());
            }
            model
                .as_object_mut()
                .expect("model is an object")
                .insert("name".to_string(), name);
        }
        None => {
            if let Some(config) = config.as_mut() {
                remove_pruning(config, &["model".to_string(), "name".to_string()]);
            }
        }
    }
    if let Some(config) = config {
        root.insert("config".to_string(), config);
    }
    row
}

/// 把编辑器保存的改动写进 live。`key_fields` 有值时（直连模式下编辑当前供应商）关键
/// 字段在同一次写入里换成它的；`set_pointer` 为新增第一个供应商，指针随操作落定。
pub(crate) fn write_live(
    db: &Database,
    edits: &GeminiEdits,
    key_fields: Option<&Provider>,
    set_pointer: bool,
) -> Result<(), AppError> {
    let projection = key_fields.map(gemini_direct::projection).transpose()?;
    if projection.is_none() && !edits.touches_env() && !edits.touches_settings() {
        return Ok(());
    }
    let pointer = key_fields
        .filter(|_| set_pointer)
        .map(|target| target.id.clone());
    gemini_direct::run_with_edits(
        db,
        if pointer.is_some() {
            op::SWITCH
        } else {
            op::APPLY
        },
        projection.as_ref(),
        PendingTarget::pointer(pointer),
        Some(edits),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn settings_apply(edits: &GeminiEdits, live: &Value) -> Result<Value, LiveWriteError> {
        let out = SettingsWrite {
            edits: Some(edits),
            key_fields: None,
        }
        .apply(
            Path::new("settings.json"),
            Some(live.to_string().as_bytes()),
        )?;
        Ok(serde_json::from_slice(&out).unwrap())
    }

    #[test]
    fn key_fields_are_not_global_changes() {
        let base = json!({
            "env": {"GEMINI_API_KEY": "a", "GEMINI_SANDBOX": "docker"},
            "config": {"model": {"name": "m-a", "compressionThreshold": 0.5}, "security": {"auth": {"selectedType": "gemini-api-key"}}, "ui": {"theme": "dark"}},
        });
        let edited = json!({
            "env": {"GEMINI_API_KEY": "b", "DEBUG": "1"},
            "config": {"model": {"name": "m-b", "compressionThreshold": 0.6}, "security": {"auth": {"selectedType": "oauth-personal"}}, "ui": {"theme": "dark"}},
        });
        let plan = plan_save(None, &edited, &base, ConflictPolicy::Refuse).unwrap();
        assert_eq!(
            plan.edits.env,
            vec![
                EnvChange {
                    key: "GEMINI_SANDBOX".into(),
                    before: Some("docker".into()),
                    after: None
                },
                EnvChange {
                    key: "DEBUG".into(),
                    before: None,
                    after: Some("1".into())
                },
            ]
        );
        assert_eq!(
            plan.edits.settings,
            vec![JsonChange {
                path: KeyPath::new(&["model", "compressionThreshold"]),
                before: Some(json!(0.5)),
                after: Some(json!(0.6)),
            }]
        );
        assert_eq!(plan.row_settings["env"], json!({"GEMINI_API_KEY": "b"}));
        assert_eq!(
            plan.row_settings["config"],
            json!({"model": {"name": "m-b"}})
        );
    }

    #[test]
    fn saving_keeps_the_rest_of_the_row() {
        let stored = json!({
            "env": {"GEMINI_API_KEY": "a", "GEMINI_MODEL": "old", "HTTPS_PROXY": "x"},
            "config": {"model": {"name": "old", "maxSessionTurns": 3}, "mcpServers": {"fs": {}}},
        });
        let edited = json!({"env": {"GEMINI_API_KEY": "b"}, "config": {}});
        let plan = plan_save(Some(&stored), &edited, &edited, ConflictPolicy::Refuse).unwrap();
        assert_eq!(
            plan.row_settings,
            json!({
                "env": {"HTTPS_PROXY": "x", "GEMINI_API_KEY": "b"},
                "config": {"model": {"maxSessionTurns": 3}, "mcpServers": {"fs": {}}},
            })
        );
    }

    #[test]
    fn replacing_a_whole_object_keeps_the_live_key_fields() {
        let base = json!({"config": {"security": {"auth": {"selectedType": "gemini-api-key"}}}});
        let edited = json!({"config": {"security": "reset"}});
        let plan = plan_save(None, &edited, &base, ConflictPolicy::Refuse).unwrap();
        // 只剩关键字段的 security 算不存在：这是一次新增，live 里也只有关键字段，不算冲突。
        // 用户把它改成了非对象，关键字段放不回去。
        let live = json!({"security": {"auth": {"selectedType": "oauth-personal"}}});
        assert_eq!(
            settings_apply(&plan.edits, &live).unwrap(),
            json!({"security": "reset"})
        );

        let base = json!({"config": {"model": {"name": "m", "compressionThreshold": 0.5}}});
        let edited = json!({"config": {}});
        let plan = plan_save(None, &edited, &base, ConflictPolicy::Refuse).unwrap();
        let live = json!({"model": {"name": "live-m", "compressionThreshold": 0.5}, "ui": {}});
        assert_eq!(
            settings_apply(&plan.edits, &live).unwrap(),
            json!({"model": {"name": "live-m"}, "ui": {}})
        );
    }

    #[test]
    fn edits_detect_three_way_conflicts_in_the_env_file() {
        let base = json!({"env": {"DEBUG": "1"}});
        let edited = json!({"env": {"DEBUG": "2"}});
        let plan = plan_save(None, &edited, &base, ConflictPolicy::Refuse).unwrap();
        let write = EnvWrite {
            edits: Some(&plan.edits),
            key_fields: None,
        };
        let err = write
            .apply(Path::new(".env"), Some(b"# mine\nDEBUG=3\n"))
            .unwrap_err();
        assert!(matches!(err, LiveWriteError::EditConflict { .. }));
        let out = write
            .apply(Path::new(".env"), Some(b"# mine\nDEBUG=1\nX=y\n"))
            .unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "# mine\nDEBUG=2\nX=y\n");
    }
}
