//! Explicit, selective Codex activation. Preview is read-only; commit uses the
//! existing journal and checks both the reviewed snapshot and publication preimage.
use super::{
    codex_direct,
    editor_toml::{insert_at, item_at, remove_at, render},
    SwitchResult,
};
use crate::codex_config::{
    get_codex_auth_path, get_codex_config_path, get_codex_model_catalog_path,
};
use crate::live::engine::{digest, read_current};
use crate::live::patch::toml::parse;
use crate::live::patch::{Guarded, LivePatch, LiveWriteError, WholeFile};
use crate::live::project::codex::CodexConfigPatch;
use crate::mode::state::PendingTarget;
use crate::{app_config::AppType, error::AppError, provider::Provider, store::AppState};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;
use toml_edit::{DocumentMut, Item};

const CONNECTION: &[&str] = &[
    "model_provider",
    "model_providers",
    "openai_base_url",
    "experimental_bearer_token",
    "base_url",
    "wire_api",
];
const EFFORT: &str = "model_reasoning_effort";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionRequest {
    pub revision: String,
    pub selected: Vec<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    #[serde(default)]
    pub exit_to_direct: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub id: String,
    pub label: String,
    pub before: String,
    pub after: String,
    pub selected: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub model: String,
    pub reasoning_levels: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub revision: String,
    pub proxy: bool,
    pub native: bool,
    pub routing: bool,
    pub exit_to_direct: bool,
    pub fields: Vec<Field>,
    pub models: Vec<Model>,
    pub current_models: Vec<Model>,
    pub default_model: Option<String>,
    pub current_model: Option<String>,
    pub current_effort: Option<String>,
}

#[derive(Clone, Serialize, PartialEq)]
pub(crate) struct Snapshot {
    config: Option<String>,
    auth: Option<String>,
    catalog: Option<String>,
    selected_catalog: Option<String>,
}

impl Snapshot {
    fn read() -> Result<Self, AppError> {
        let path = get_codex_config_path();
        let config = read_current(&path)?;
        let doc = parse(&path, config.as_deref())?;
        Ok(Self {
            config: digest(config.as_deref()),
            auth: digest(read_current(&get_codex_auth_path())?.as_deref()),
            catalog: digest(read_current(&get_codex_model_catalog_path())?.as_deref()),
            selected_catalog: digest(current_catalog_bytes(&doc).as_deref()),
        })
    }
}

fn error(message: &str) -> AppError {
    AppError::Message(message.to_string())
}
fn parse_text(text: &str) -> Result<DocumentMut, AppError> {
    text.parse()
        .map_err(|e| error(&format!("Codex TOML 无法解析：{e}")))
}

/// Keep link globals outside config.toml, including executable settings. They
/// require a separate, unchecked-by-default field selection before publication.
pub(crate) fn import_globals(text: &str) -> Result<String, AppError> {
    let source = parse_text(text)?;
    let mut globals = DocumentMut::new();
    for entry in super::codex_editor::entries(&source, &[]) {
        if entry.path.first().is_some_and(|p| {
            matches!(
                p.as_str(),
                "model_providers" | "modelCatalog" | "importedGlobalConfig"
            )
        }) {
            continue;
        }
        insert_at(&mut globals, &entry.path, entry.item);
    }
    Ok(globals.to_string())
}

fn globals(provider: &Provider) -> Result<DocumentMut, AppError> {
    let row = parse_text(super::editor_toml::config_text(&provider.settings_config))?;
    let mut doc = DocumentMut::new();
    for entry in super::codex_editor::entries(&row, &[]) {
        if entry.path.first().is_some_and(|p| p == "model_providers") {
            continue;
        }
        insert_at(&mut doc, &entry.path, entry.item);
    }
    if let Some(staged) = provider
        .settings_config
        .get("importedGlobalConfig")
        .and_then(Value::as_str)
    {
        for entry in super::codex_editor::entries(&parse_text(staged)?, &[]) {
            if entry.path.first().is_some_and(|p| p == "model_providers") {
                continue;
            }
            insert_at(&mut doc, &entry.path, entry.item);
        }
    }
    Ok(doc)
}

fn paths(doc: &DocumentMut) -> BTreeSet<Vec<String>> {
    let mut result = BTreeSet::new();
    for (key, item) in doc.iter() {
        if CONNECTION.contains(&key) || key == EFFORT || key == "model_catalog_json" {
            continue;
        }
        if let Some(table) = item.as_table_like() {
            for (child, _) in table.iter() {
                result.insert(vec![key.into(), child.into()]);
            }
        } else {
            result.insert(vec![key.into()]);
        }
    }
    result
}

fn field_id(path: &[String]) -> String {
    serde_json::to_string(path).expect("string path")
}
fn value(doc: &DocumentMut, key: &str) -> Option<String> {
    doc.get(key).and_then(Item::as_str).map(str::to_string)
}
fn sensitive(key: &str) -> bool {
    let key = key.to_lowercase();
    [
        "key",
        "token",
        "secret",
        "password",
        "authorization",
        "credential",
        "env",
        "headers",
        "query_params",
        "args",
    ]
    .iter()
    .any(|word| key.contains(word))
}
fn mask(item: &Item, key: &str) -> Item {
    if sensitive(key) {
        return toml_edit::value("••••••");
    }
    if let Some(table) = item.as_table_like() {
        let mut copy = toml_edit::Table::new();
        for (name, child) in table.iter() {
            copy.insert(name, mask(child, name));
        }
        Item::Table(copy)
    } else if let Some(array) = item.as_array() {
        let mut copy = toml_edit::Array::new();
        for child in array.iter() {
            if let Ok(value) = mask(&Item::Value(child.clone()), key).into_value() {
                copy.push(value);
            }
        }
        toml_edit::value(copy)
    } else if let Some(array) = item.as_array_of_tables() {
        let mut copy = toml_edit::ArrayOfTables::new();
        for child in array.iter() {
            if let Ok(table) = mask(&Item::Table(child.clone()), key).into_table() {
                copy.push(table);
            }
        }
        Item::ArrayOfTables(copy)
    } else if let Some(text) = item.as_str().filter(|s| s.contains("://")) {
        if let Ok(mut url) = url::Url::parse(text) {
            let _ = url.set_username("");
            let _ = url.set_password(None);
            if url.query().is_some() {
                url.set_query(Some("redacted"));
            }
            url.set_fragment(None);
            toml_edit::value(url.to_string())
        } else {
            toml_edit::value("••••••")
        }
    } else {
        item.clone()
    }
}
fn display(doc: &DocumentMut, path: &[String]) -> String {
    item_at(doc, path)
        .map(|item| render(&mask(item, path.last().map(String::as_str).unwrap_or(""))))
        .unwrap_or_else(|| "（未设置）".to_string())
}
fn connection_display(doc: &DocumentMut) -> String {
    CONNECTION
        .iter()
        .filter(|key| doc.contains_key(key))
        .map(|key| format!("{key}: {}", display(doc, &[key.to_string()])))
        .collect::<Vec<_>>()
        .join("\n")
}
fn models(catalog: Option<&Value>, native: bool) -> Vec<Model> {
    catalog
        .and_then(|v| v.get("models"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| {
            Some(Model {
                model: row
                    .get(if native { "slug" } else { "model" })?
                    .as_str()?
                    .to_string(),
                reasoning_levels: row
                    .get(if native {
                        "supported_reasoning_levels"
                    } else {
                        "reasoningLevels"
                    })
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|level| {
                        if native {
                            level.get("effort").and_then(Value::as_str)
                        } else {
                            level.as_str()
                        }
                    })
                    .map(str::to_string)
                    .collect(),
            })
        })
        .collect()
}
fn native_catalog(bytes: Option<&[u8]>) -> Option<Value> {
    bytes.and_then(|b| serde_json::from_slice(b).ok())
}
fn current_catalog_bytes(doc: &DocumentMut) -> Option<Vec<u8>> {
    let pointer = value(doc, "model_catalog_json")?;
    let path = std::path::PathBuf::from(pointer);
    let path = if path.is_absolute() {
        path
    } else {
        get_codex_config_path()
            .parent()
            .unwrap_or(Path::new("."))
            .join(path)
    };
    std::fs::read(path).ok()
}
fn current_models(doc: &DocumentMut) -> Vec<Model> {
    models(
        native_catalog(current_catalog_bytes(doc).as_deref()).as_ref(),
        true,
    )
}
fn revision(
    state: &AppState,
    provider: &Provider,
    snapshot: &Snapshot,
    exit_to_direct: bool,
) -> Result<String, AppError> {
    let bytes = serde_json::to_vec(&json!([
        provider,
        snapshot,
        exit_to_direct,
        state.db.get_all_providers("codex")?,
        crate::mode::current::provider_for(
            &state.db,
            &AppType::Codex,
            crate::mode::current::Purpose::Direct
        )?,
        crate::mode::state::stack(&crate::live::engine::DeviceStore::for_device(), "codex")?,
        crate::mode::current::mode_state(&AppType::Codex),
        crate::settings::preserve_codex_official_auth_on_switch(),
        crate::settings::unify_codex_session_history()
    ]))
    .map_err(|e| error(&e.to_string()))?;
    Ok(digest(Some(&bytes)).expect("present bytes"))
}
fn provider(state: &AppState, id: &str) -> Result<Provider, AppError> {
    state
        .db
        .get_all_providers("codex")?
        .get(id)
        .cloned()
        .ok_or_else(|| error("供应商不存在"))
}
fn make_preview(
    state: &AppState,
    provider: &Provider,
    planned: &codex_direct::Planned,
    snapshot: &Snapshot,
    exit_to_direct: bool,
) -> Result<Preview, AppError> {
    let path = get_codex_config_path();
    let base = parse(&path, read_current(&path)?.as_deref())?;
    let mut proposed = base.clone();
    planned.config().apply_to(&path, &mut proposed)?;
    let extras = globals(provider)?;
    for entry_path in paths(&extras) {
        if let Some(item) = item_at(&extras, &entry_path) {
            insert_at(&mut proposed, &entry_path, item.clone());
        }
    }
    // Reasoning is decided against the effective model, separately from overwrite toggles.
    let mut fields = vec![
        Field {
            id: "connection".into(),
            label: "连接信息（地址、凭据、路由）".into(),
            before: connection_display(&base),
            after: connection_display(&proposed),
            selected: true,
        },
        Field {
            id: "catalog".into(),
            label: "模型目录".into(),
            before: display(&base, &["model_catalog_json".into()]),
            after: display(&proposed, &["model_catalog_json".into()]),
            selected: true,
        },
    ];
    let all_paths: BTreeSet<_> = paths(&base).union(&paths(&proposed)).cloned().collect();
    for p in all_paths {
        let before = item_at(&base, &p).map(render);
        let after = item_at(&proposed, &p).map(render);
        if before == after && p != ["model"] {
            continue;
        }
        // Absent imported values are never deletion instructions for user preferences.
        if after.is_none() {
            continue;
        }
        let selected = p == ["model"]
            || crate::live::floor::CODEX_FLOOR_NESTED
                .iter()
                .any(|nested| nested.iter().copied().eq(p.iter().map(String::as_str)));
        fields.push(Field {
            id: field_id(&p),
            label: p.join("."),
            before: display(&base, &p),
            after: display(&proposed, &p),
            selected,
        });
    }
    if !fields.iter().any(|f| f.id == "[\"model\"]") {
        fields.push(Field {
            id: "[\"model\"]".into(),
            label: "model".into(),
            before: display(&base, &["model".into()]),
            after: "请选择默认模型".into(),
            selected: true,
        });
    }
    let routing = crate::mode::current::is_proxy(&AppType::Codex) && !exit_to_direct;
    let mut target_models = if routing {
        models(native_catalog(planned.catalog()).as_ref(), true)
    } else {
        models(provider.settings_config.get("modelCatalog"), false)
    };
    if target_models.is_empty() {
        target_models = models(native_catalog(planned.catalog()).as_ref(), true);
    }
    let existing_models = current_models(&base);
    if let Some(catalog) = fields.iter_mut().find(|f| f.id == "catalog") {
        let describe = |rows: &[Model]| {
            rows.iter()
                .map(|row| format!("{}: {}", row.model, row.reasoning_levels.join(", ")))
                .collect::<Vec<_>>()
                .join("\n")
        };
        catalog.before = format!("{}\n{}", catalog.before, describe(&existing_models));
        catalog.after = format!("{}\n{}", catalog.after, describe(&target_models));
    }
    Ok(Preview {
        revision: revision(state, provider, snapshot, exit_to_direct)?,
        proxy: false,
        native: false,
        routing,
        exit_to_direct,
        fields,
        models: target_models,
        current_models: existing_models,
        default_model: value(
            &parse_text(super::editor_toml::config_text(&provider.settings_config))?,
            "model",
        ),
        current_model: value(&base, "model"),
        current_effort: value(&base, EFFORT),
    })
}

#[cfg(test)]
pub fn preview(state: &AppState, id: &str) -> Result<Preview, AppError> {
    preview_for(state, id, false)
}

pub fn preview_for(state: &AppState, id: &str, exit_to_direct: bool) -> Result<Preview, AppError> {
    let provider = provider(state, id)?;
    let snapshot = Snapshot::read()?;
    let start_revision = revision(state, &provider, &snapshot, exit_to_direct)?;
    let mode = crate::mode::current::mode_state(&AppType::Codex);
    if mode.is_proxy()
        && !exit_to_direct
        && (!mode.attached || codex_direct::is_official(&provider))
    {
        return Ok(Preview {
            revision: start_revision,
            proxy: true,
            native: false,
            routing: true,
            exit_to_direct,
            fields: vec![Field {
                id: "connection".into(),
                label: "代理路由".into(),
                before: crate::mode::current::mode_state(&AppType::Codex)
                    .proxy_route
                    .unwrap_or_default(),
                after: provider.name.clone(),
                selected: true,
            }],
            models: vec![],
            current_models: vec![],
            default_model: None,
            current_model: None,
            current_effort: None,
        });
    }
    let (planned, _, _) =
        crate::mode::controller::reviewed_codex_plan(state, &provider, exit_to_direct, false)?;
    let preview = make_preview(state, &provider, &planned, &snapshot, exit_to_direct)?;
    if Snapshot::read()? != snapshot || preview.revision != start_revision {
        return Err(error("预览期间 Codex 配置发生变化，请重试"));
    }
    if codex_direct::is_official(&provider) {
        return Ok(Preview {
            native: true,
            fields: vec![Field {
                id: "connection".into(),
                label: "使用官方账号配置".into(),
                before: "当前连接与模型设置".into(),
                after: format!("{}（按 v4.0.17 官方账号切换规则写入）", provider.name),
                selected: true,
            }],
            ..preview
        });
    }
    Ok(preview)
}

#[derive(Clone)]
pub(crate) struct Selection {
    snapshot: Snapshot,
    selected: BTreeSet<String>,
    paths: Vec<Vec<String>>,
    extras: String,
    model: Option<String>,
    effort: Option<String>,
}
impl Selection {
    pub(crate) fn guarded_catalog(&self, bytes: Vec<u8>) -> Guarded {
        Guarded {
            expected_pre: self.snapshot.catalog.clone(),
            then: WholeFile::Write(bytes),
        }
    }
    pub(crate) fn ensure_auth(&self, bytes: Option<&[u8]>) -> Result<(), AppError> {
        if digest(bytes) != self.snapshot.auth {
            return Err(error("登录凭据已改变，请重新确认"));
        }
        Ok(())
    }
    pub(crate) fn connection(&self) -> bool {
        self.selected.contains("connection")
    }
    pub(crate) fn catalog(&self) -> bool {
        self.selected.contains("catalog")
    }
    pub(crate) fn ensure_unchanged(&self) -> Result<(), AppError> {
        if Snapshot::read()? != self.snapshot {
            return Err(error("Codex 配置已改变，请重新预览后再确认"));
        }
        Ok(())
    }
}

pub(crate) struct SelectionPatch<'a> {
    pub selection: &'a Selection,
    pub config: &'a CodexConfigPatch,
}
impl LivePatch for SelectionPatch<'_> {
    fn apply(&self, path: &Path, pre: Option<&[u8]>) -> Result<Vec<u8>, LiveWriteError> {
        if digest(pre) != self.selection.snapshot.config {
            return Err(LiveWriteError::Conflict {
                path: path.to_path_buf(),
            });
        }
        let mut base = parse(path, pre)?;
        let mut proposed = base.clone();
        self.config.apply_to(path, &mut proposed)?;
        let extras = parse(path, Some(self.selection.extras.as_bytes()))?;
        for p in paths(&extras) {
            if let Some(item) = item_at(&extras, &p) {
                insert_at(&mut proposed, &p, item.clone());
            }
        }
        let mut chosen = self.selection.paths.clone();
        if self.selection.connection() {
            chosen.extend(CONNECTION.iter().map(|s| vec![s.to_string()]));
        }
        if self.selection.catalog() {
            chosen.push(vec!["model_catalog_json".into()]);
        }
        for p in chosen {
            match item_at(&proposed, &p).cloned() {
                Some(item) => insert_at(&mut base, &p, item),
                None if p.first().is_some_and(|k| {
                    CONNECTION.contains(&k.as_str()) || k == "model_catalog_json"
                }) =>
                {
                    remove_at(&mut base, &p)
                }
                None => {}
            }
        }
        if let Some(model) = &self.selection.model {
            base["model"] = toml_edit::value(model.as_str());
        }
        if let Some(effort) = &self.selection.effort {
            if value(&base, EFFORT).as_deref() != Some(effort.as_str()) {
                base[EFFORT] = toml_edit::value(effort.as_str());
            }
        }
        Ok(base.to_string().into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::project::codex::RouteWrite;

    fn patch() -> CodexConfigPatch {
        let route = parse_text("[custom]\nname = 'New'\nbase_url = 'https://new.example/v1'\nexperimental_bearer_token = 'secret-new'").unwrap();
        CodexConfigPatch {
            top: vec![
                ("model".into(), "new-model".into()),
                (EFFORT.into(), "ultra".into()),
            ],
            nested: vec![],
            exclusive: vec![("model_context_window".into(), 999.into())],
            outgoing: vec![],
            route: RouteWrite::Custom(route["custom"].as_table().unwrap().clone()),
            catalog: true,
            retired: vec![],
        }
    }
    fn selection(base: &str, chosen: &[&str]) -> Selection {
        Selection {
            snapshot: Snapshot {
                config: digest(Some(base.as_bytes())),
                auth: None,
                catalog: None,
                selected_catalog: None,
            },
            selected: chosen.iter().map(|s| s.to_string()).collect(),
            paths: chosen
                .iter()
                .filter_map(|s| serde_json::from_str(s).ok())
                .collect(),
            extras: "[mcp_servers.demo]\ncommand = 'imported'\n[features]\nnew_flag = true".into(),
            model: chosen.contains(&"[\"model\"]").then(|| "new-model".into()),
            effort: Some("high".into()),
        }
    }
    const BASE: &str = "# keep comment\nmodel = 'old-model'\nmodel_reasoning_effort = 'high'\nmodel_context_window = 123\n[model_providers.old]\nbase_url = 'https://old.example'\n[mcp_servers.demo]\ncommand = 'local'\n[mcp_servers.keep]\ncommand = 'keep'\n";

    #[test]
    fn selected_route_and_model_preserve_unselected_settings_and_effort() {
        let selection = selection(BASE, &["connection", "catalog", "[\"model\"]"]);
        let config = patch();
        let bytes = SelectionPatch {
            selection: &selection,
            config: &config,
        }
        .apply(Path::new("test.toml"), Some(BASE.as_bytes()))
        .unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let doc = parse_text(&text).unwrap();
        assert_eq!(value(&doc, "model").as_deref(), Some("new-model"));
        assert_eq!(value(&doc, EFFORT).as_deref(), Some("high"));
        assert_eq!(
            doc["mcp_servers"]["demo"]["command"].as_str(),
            Some("local")
        );
        assert_eq!(doc["model_context_window"].as_integer(), Some(123));
        assert_eq!(
            doc["model_providers"]["custom"]["experimental_bearer_token"].as_str(),
            Some("secret-new")
        );
        assert!(text.contains("# keep comment"));
        assert!(!doc.contains_key("features"));
    }
    #[test]
    fn selected_global_field_keeps_connection_and_neighboring_mcp_servers() {
        let selection = selection(BASE, &["[\"mcp_servers\",\"demo\"]"]);
        let config = patch();
        let bytes = SelectionPatch {
            selection: &selection,
            config: &config,
        }
        .apply(Path::new("test.toml"), Some(BASE.as_bytes()))
        .unwrap();
        let doc = parse_text(std::str::from_utf8(&bytes).unwrap()).unwrap();
        assert_eq!(
            doc["mcp_servers"]["demo"]["command"].as_str(),
            Some("imported")
        );
        assert_eq!(doc["mcp_servers"]["keep"]["command"].as_str(), Some("keep"));
        assert_eq!(value(&doc, "model").as_deref(), Some("old-model"));
        assert!(doc["model_providers"].get("custom").is_none());
        assert!(!doc.contains_key("model_catalog_json"));
    }
    #[test]
    fn concurrent_edit_is_rejected_before_publication() {
        let selection = selection(BASE, &["connection"]);
        let config = patch();
        let err = SelectionPatch {
            selection: &selection,
            config: &config,
        }
        .apply(Path::new("test.toml"), Some(b"model = 'changed'"))
        .unwrap_err();
        assert!(matches!(err, LiveWriteError::Conflict { .. }));
    }
    #[test]
    fn preview_masks_credentials_in_nested_tables_and_urls() {
        let doc = parse_text("[model_providers.custom]\nbase_url = 'https://user:password@relay.example/v1?api_key=secret'\nexperimental_bearer_token = 'another-secret'\n[model_providers.custom.http_headers]\nAuthorization = 'private'").unwrap();
        let shown = connection_display(&doc);
        for secret in ["password", "api_key=secret", "another-secret", "private"] {
            assert!(!shown.contains(secret));
        }
        assert!(shown.contains("relay.example"));
        let doc = parse_text("rows = [{ password = 'inline-secret', url = 'https://x.example?q=private' }]\n[[servers]]\ntoken = 'array-secret'").unwrap();
        assert!(!display(&doc, &["rows".into()]).contains("inline-secret"));
        assert!(!display(&doc, &["rows".into()]).contains("q=private"));
        assert!(!display(&doc, &["servers".into()]).contains("array-secret"));
    }
    #[test]
    fn imported_globals_are_staged_separately_from_connection_fields() {
        let doc = import_globals("model = 'm'\nmodel_reasoning_effort = 'high'\n[model_providers.custom]\nbase_url = 'https://example.com'\n[mcp_servers.test]\ncommand = 'example'").unwrap();
        assert!(doc.contains("mcp_servers"));
        assert!(!doc.contains("base_url"));
        assert!(!doc.contains("model_reasoning_effort"));
    }
}

pub fn commit(
    state: &AppState,
    id: &str,
    request: SelectionRequest,
) -> Result<SwitchResult, AppError> {
    let _guard = crate::mode::controller::lock_settled_blocking(state, &AppType::Codex)?;
    let provider = provider(state, id)?;
    let snapshot = Snapshot::read()?;
    let proxy = crate::mode::current::is_proxy(&AppType::Codex);
    if revision(state, &provider, &snapshot, request.exit_to_direct)? != request.revision {
        return Err(error("供应商、模式或 Codex 配置已改变，请重新打开差异确认"));
    }
    if proxy
        && !request.exit_to_direct
        && (!crate::mode::current::mode_state(&AppType::Codex).attached
            || codex_direct::is_official(&provider))
    {
        if !request.selected.iter().any(|s| s == "connection") {
            return Ok(SwitchResult::default());
        }
        let route = futures::executor::block_on(crate::mode::controller::switch_route_locked(
            state,
            &AppType::Codex,
            &provider,
        ))
        .map_err(|e| error(&e.to_string()))?;
        return Ok(SwitchResult {
            route: Some(route),
            ..Default::default()
        });
    }
    if codex_direct::is_official(&provider) {
        if request.exit_to_direct && proxy {
            return Err(error("官方账号请使用原有的退出代理流程"));
        }
        if !request.selected.iter().any(|s| s == "connection") {
            return Ok(SwitchResult::default());
        }
        return super::ProviderService::switch_codex_direct(
            state,
            &provider,
            &state.db.get_all_providers("codex")?,
        );
    }
    let (planned, _, mut pending) = crate::mode::controller::reviewed_codex_plan(
        state,
        &provider,
        request.exit_to_direct,
        false,
    )?;
    let preview = make_preview(
        state,
        &provider,
        &planned,
        &snapshot,
        request.exit_to_direct,
    )?;
    let selected: BTreeSet<String> = request.selected.into_iter().collect();
    if selected
        .iter()
        .any(|id| !preview.fields.iter().any(|f| &f.id == id))
    {
        return Err(error("覆盖项不在当前预览中，请重新确认"));
    }
    let model_selected = selected.contains("[\"model\"]");
    let changes_models =
        model_selected || selected.contains("catalog") || selected.contains("connection");
    let model = if model_selected {
        request.model.clone()
    } else {
        preview.current_model.clone()
    };
    let available = if selected.contains("catalog") {
        &preview.models
    } else {
        &preview.current_models
    };
    let effort = if changes_models {
        let model = model
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| error("请选择默认模型"))?;
        let row = available
            .iter()
            .find(|row| row.model == model)
            .ok_or_else(|| error("默认模型不在所选目录中，请选择模型目录及默认模型"))?;
        Some(
            request
                .effort
                .filter(|s| row.reasoning_levels.contains(s))
                .ok_or_else(|| error("请确认新模型支持的默认思考等级"))?,
        )
    } else {
        // A global-only edit must not require or invent a model/effort/catalog.
        None
    };
    let selected_paths = selected
        .iter()
        .filter_map(|id| serde_json::from_str::<Vec<String>>(id).ok())
        .collect();
    let connection = selected.contains("connection");
    if request.exit_to_direct && proxy && !connection {
        return Err(error("回到直连需要勾选连接信息；取消不会修改配置或模式"));
    }
    if !connection {
        pending = PendingTarget::default();
    }
    let selection = Selection {
        snapshot,
        selected,
        paths: selected_paths,
        extras: globals(&provider)?.to_string(),
        model: model_selected.then_some(model).flatten(),
        effort,
    };
    selection.ensure_unchanged()?;
    let (mut planned, prepared, _) = crate::mode::controller::reviewed_codex_plan(
        state,
        &provider,
        request.exit_to_direct,
        connection,
    )?;
    planned.set_selection(selection);
    codex_direct::run(
        &state.db,
        crate::mode::state::op::SWITCH,
        planned,
        &prepared,
        pending,
    )?;
    if connection {
        crate::mode::controller::finish_reviewed_codex_switch(
            state,
            &provider,
            proxy && request.exit_to_direct,
        );
    }
    Ok(SwitchResult {
        route: (proxy && !request.exit_to_direct && connection).then(|| {
            crate::mode::controller::RouteSwitchResult {
                kind: if crate::mode::state::stack(
                    &crate::live::engine::DeviceStore::for_device(),
                    "codex",
                )
                .is_ok_and(|s| s.enabled)
                {
                    crate::mode::controller::RouteSwitchKind::Stack
                } else {
                    crate::mode::controller::RouteSwitchKind::Route
                },
                client_config_changed: true,
            }
        }),
        ..Default::default()
    })
}
