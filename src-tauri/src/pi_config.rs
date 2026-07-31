//! Pi Agent live configuration helpers.
//!
//! Pi keeps model providers in `models.json`, credentials in `auth.json`, and
//! the active provider/model in `settings.json` under `~/.pi/agent` by default.

use json_five::rt::parser::{
    from_str as parse_round_trip, JSONKeyValuePair, JSONObjectContext,
    JSONText as RoundTripDocument, JSONValue as RoundTripValue, KeyValuePairContext,
};
use json_five::tokenize::TokType;
use json_five::{source_to_tokens, tokens_to_source};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::{atomic_write, get_app_config_dir, get_home_dir};
use crate::error::AppError;
use crate::provider::Provider;

#[derive(Debug)]
struct JsonFileSnapshot {
    path: PathBuf,
    raw: Option<Vec<u8>>,
    value: Value,
    kind: JsonFileKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JsonFileKind {
    ModelsJsonc,
    Json,
}

#[derive(Clone, Copy)]
struct PendingDocument<'a> {
    snapshot: &'a JsonFileSnapshot,
    next: &'a [u8],
    secure_if_new: bool,
}

#[derive(Clone, Debug)]
enum ModelsMutation {
    Upsert { provider_id: String, config: Value },
    Remove { provider_id: String },
}

fn pi_config_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn lock_pi_config() -> Result<std::sync::MutexGuard<'static, ()>, AppError> {
    pi_config_lock()
        .lock()
        .map_err(|_| AppError::Message("Pi configuration write lock is poisoned".to_string()))
}

pub fn get_pi_dir() -> PathBuf {
    if let Some(override_dir) = crate::settings::get_pi_override_dir() {
        return override_dir;
    }

    if let Some(raw) = std::env::var_os("PI_CODING_AGENT_DIR") {
        let value = raw.to_string_lossy();
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    get_home_dir().join(".pi").join("agent")
}

pub fn get_pi_models_path() -> PathBuf {
    get_pi_dir().join("models.json")
}

pub fn get_pi_settings_path() -> PathBuf {
    get_pi_dir().join("settings.json")
}

pub fn get_pi_auth_path() -> PathBuf {
    get_pi_dir().join("auth.json")
}

fn jsonc_error(path: &Path, message: impl std::fmt::Display) -> AppError {
    AppError::Config(format!(
        "Pi models.json is not valid Pi JSONC (double-quoted JSON with // comments and trailing commas): {}: {message}",
        path.display()
    ))
}

fn parse_pi_jsonc(path: &Path, raw: &[u8]) -> Result<Value, AppError> {
    let source = std::str::from_utf8(raw)
        .map_err(|error| jsonc_error(path, format!("invalid UTF-8: {error}")))?;
    let tokens = source_to_tokens(source).map_err(|error| jsonc_error(path, error))?;

    for token in &tokens {
        let unsupported = match token.tok_type {
            TokType::Name => Some("unquoted object keys"),
            TokType::SingleQuotedString => Some("single-quoted strings"),
            TokType::BlockComment => Some("block comments"),
            TokType::Infinity | TokType::Nan | TokType::Hexadecimal | TokType::Plus => {
                Some("JSON5 number syntax")
            }
            _ => None,
        };
        if let Some(feature) = unsupported {
            return Err(jsonc_error(
                path,
                format!(
                    "{feature} are not supported (line {})",
                    token.context.as_ref().map_or(1, |ctx| ctx.start_lineno)
                ),
            ));
        }
    }

    let mut json_tokens = Vec::with_capacity(tokens.len());
    for (index, token) in tokens.iter().enumerate() {
        if token.tok_type == TokType::LineComment {
            let mut replacement = token.clone();
            replacement.lexeme = token
                .lexeme
                .chars()
                .filter(|character| matches!(character, '\r' | '\n'))
                .collect();
            json_tokens.push(replacement);
            continue;
        }
        if token.tok_type == TokType::Comma {
            let next_significant = tokens[index + 1..].iter().find(|candidate| {
                !matches!(
                    candidate.tok_type,
                    TokType::Whitespace | TokType::LineComment
                )
            });
            if next_significant.is_some_and(|candidate| {
                matches!(
                    candidate.tok_type,
                    TokType::RightBrace | TokType::RightBracket
                )
            }) {
                continue;
            }
        }
        json_tokens.push(token.clone());
    }

    let json_source = tokens_to_source(&json_tokens);
    serde_json::from_str(&json_source).map_err(|error| jsonc_error(path, error))
}

fn parse_snapshot_value(path: &Path, raw: &[u8], kind: JsonFileKind) -> Result<Value, AppError> {
    if raw.iter().all(|byte| byte.is_ascii_whitespace()) {
        return Ok(Value::Object(Map::new()));
    }
    match kind {
        JsonFileKind::ModelsJsonc => parse_pi_jsonc(path, raw),
        JsonFileKind::Json => {
            serde_json::from_slice(raw).map_err(|error| AppError::json(path, error))
        }
    }
}

fn read_snapshot(path: &Path, kind: JsonFileKind) -> Result<JsonFileSnapshot, AppError> {
    let raw = match fs::read(path) {
        Ok(raw) => Some(raw),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(AppError::io(path, error)),
    };
    let value = match raw.as_deref() {
        None => Value::Object(Map::new()),
        Some(raw) => parse_snapshot_value(path, raw, kind)?,
    };

    Ok(JsonFileSnapshot {
        path: path.to_path_buf(),
        raw,
        value,
        kind,
    })
}

fn read_json_or_empty_object(path: &Path, kind: JsonFileKind) -> Result<Value, AppError> {
    read_snapshot(path, kind).map(|snapshot| snapshot.value)
}

fn current_raw(path: &Path) -> Result<Option<Vec<u8>>, AppError> {
    match fs::read(path) {
        Ok(raw) => Ok(Some(raw)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AppError::io(path, error)),
    }
}

fn ensure_unchanged(snapshot: &JsonFileSnapshot) -> Result<(), AppError> {
    if current_raw(&snapshot.path)? != snapshot.raw {
        return Err(AppError::Config(format!(
            "Pi configuration changed on disk while CC Switch was updating it: {}. Reload and try again.",
            snapshot.path.display()
        )));
    }
    Ok(())
}

fn restore_snapshot(snapshot: &JsonFileSnapshot) -> Result<(), AppError> {
    match &snapshot.raw {
        Some(raw) => atomic_write(&snapshot.path, raw),
        None if snapshot.path.exists() => {
            fs::remove_file(&snapshot.path).map_err(|error| AppError::io(&snapshot.path, error))
        }
        None => Ok(()),
    }
}

fn cleanup_backups(backup_root: &Path) -> Result<(), AppError> {
    let retain = crate::settings::effective_backup_retain_count();
    let mut entries = fs::read_dir(backup_root)
        .map_err(|error| AppError::io(backup_root, error))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .collect::<Vec<_>>();
    if entries.len() <= retain {
        return Ok(());
    }

    entries.sort_by_key(|entry| entry.metadata().and_then(|meta| meta.modified()).ok());
    let remove_count = entries.len().saturating_sub(retain);
    for entry in entries.into_iter().take(remove_count) {
        if let Err(error) = fs::remove_dir_all(entry.path()) {
            log::warn!(
                "Failed to remove old Pi backup {}: {error}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

fn create_backup(snapshots: &[&JsonFileSnapshot]) -> Result<(), AppError> {
    if snapshots.iter().all(|snapshot| snapshot.raw.is_none()) {
        return Ok(());
    }

    let backup_root = get_app_config_dir().join("backups").join("pi");
    fs::create_dir_all(&backup_root).map_err(|error| AppError::io(&backup_root, error))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let base_name = format!("pi_{timestamp}");
    let mut backup_dir = backup_root.join(&base_name);
    let mut suffix = 1;
    while backup_dir.exists() {
        backup_dir = backup_root.join(format!("{base_name}_{suffix}"));
        suffix += 1;
    }
    fs::create_dir_all(&backup_dir).map_err(|error| AppError::io(&backup_dir, error))?;

    for snapshot in snapshots {
        let Some(raw) = snapshot.raw.as_deref() else {
            continue;
        };
        let filename = snapshot
            .path
            .file_name()
            .ok_or_else(|| AppError::Config("Invalid Pi config filename".to_string()))?;
        let backup_path = backup_dir.join(filename);
        atomic_write(&backup_path, raw)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&backup_path, fs::Permissions::from_mode(0o600))
                .map_err(|error| AppError::io(&backup_path, error))?;
        }
    }
    cleanup_backups(&backup_root)
}

fn write_pending_document(document: &PendingDocument<'_>) -> Result<(), AppError> {
    atomic_write(&document.snapshot.path, document.next)?;
    #[cfg(unix)]
    if document.secure_if_new && document.snapshot.raw.is_none() {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&document.snapshot.path, fs::Permissions::from_mode(0o600))
            .map_err(|error| AppError::io(&document.snapshot.path, error))?;
    }
    Ok(())
}

fn apply_documents_with<F>(documents: &[PendingDocument<'_>], mut writer: F) -> Result<(), AppError>
where
    F: FnMut(&PendingDocument<'_>) -> Result<(), AppError>,
{
    for (index, document) in documents.iter().enumerate() {
        if let Err(operation_error) = writer(document) {
            let rollback_errors = documents[..=index]
                .iter()
                .rev()
                .filter_map(|document| {
                    restore_snapshot(document.snapshot)
                        .err()
                        .map(|error| format!("{}: {error}", document.snapshot.path.display()))
                })
                .collect::<Vec<_>>();
            return if rollback_errors.is_empty() {
                Err(operation_error)
            } else {
                Err(AppError::Message(format!(
                    "{operation_error}; rollback also failed for {}",
                    rollback_errors.join(", ")
                )))
            };
        }
    }
    Ok(())
}

fn serialize_json(value: &Value) -> Result<Vec<u8>, AppError> {
    serde_json::to_vec_pretty(value).map_err(|source| AppError::JsonSerialize { source })
}

fn round_trip_key_name(key: &RoundTripValue) -> Option<String> {
    let RoundTripValue::DoubleQuotedString(raw) = key else {
        return None;
    };
    serde_json::from_str::<String>(&format!("\"{raw}\"")).ok()
}

fn parse_round_trip_value(value: &Value, base_indent: usize) -> Result<RoundTripValue, AppError> {
    let source =
        serde_json::to_string_pretty(value).map_err(|source| AppError::JsonSerialize { source })?;
    let indent = " ".repeat(base_indent);
    let source = source.replace('\n', &format!("\n{indent}"));
    parse_round_trip(&source)
        .map(|document| document.value)
        .map_err(|error| AppError::Config(format!("Failed to build Pi models.json value: {error}")))
}

fn parse_round_trip_key(key: &str) -> Result<RoundTripValue, AppError> {
    let source = serde_json::to_string(key).map_err(|source| AppError::JsonSerialize { source })?;
    parse_round_trip(&source)
        .map(|document| document.value)
        .map_err(|error| AppError::Config(format!("Failed to build Pi provider key: {error}")))
}

fn separator_for(wsc: &str, indent: usize) -> String {
    if wsc.is_empty() || wsc.contains('\n') || wsc.contains('\r') {
        format!("\n{}", " ".repeat(indent))
    } else {
        " ".to_string()
    }
}

fn object_parts_mut(
    value: &mut RoundTripValue,
) -> Result<(&mut Vec<JSONKeyValuePair>, &mut JSONObjectContext), AppError> {
    match value {
        RoundTripValue::JSONObject {
            key_value_pairs,
            context: Some(context),
        } => Ok((key_value_pairs, context)),
        _ => Err(AppError::Config(
            "Pi models.json providers must be a JSON object".to_string(),
        )),
    }
}

fn append_object_pair(
    object: &mut RoundTripValue,
    key: &str,
    value: RoundTripValue,
    child_indent: usize,
) -> Result<(), AppError> {
    let (pairs, object_context) = object_parts_mut(object)?;
    let default_closing = format!("\n{}", " ".repeat(child_indent.saturating_sub(2)));

    let new_context = if let Some(last) = pairs.last_mut() {
        let context = last.context.as_mut().ok_or_else(|| {
            AppError::Config("Pi models.json round-trip context is missing".to_string())
        })?;
        let had_trailing_comma = context.wsc.3.is_some();
        let closing = if let Some(after_comma) = context.wsc.3.take() {
            after_comma
        } else {
            std::mem::take(&mut context.wsc.2)
        };
        let separator = separator_for(&closing, child_indent);
        context.wsc.3 = Some(separator);

        if had_trailing_comma {
            KeyValuePairContext {
                wsc: (String::new(), " ".to_string(), String::new(), Some(closing)),
            }
        } else {
            KeyValuePairContext {
                wsc: (String::new(), " ".to_string(), closing, None),
            }
        }
    } else {
        let closing = if object_context.wsc.0.is_empty() {
            default_closing
        } else {
            std::mem::take(&mut object_context.wsc.0)
        };
        object_context.wsc.0 = separator_for(&closing, child_indent);
        KeyValuePairContext {
            wsc: (String::new(), " ".to_string(), closing, None),
        }
    };

    pairs.push(JSONKeyValuePair {
        key: parse_round_trip_key(key)?,
        value,
        context: Some(new_context),
    });
    Ok(())
}

fn find_object_pair_index(pairs: &[JSONKeyValuePair], key: &str) -> Option<usize> {
    pairs
        .iter()
        .position(|pair| round_trip_key_name(&pair.key).as_deref() == Some(key))
}

fn upsert_round_trip_provider(
    document: &mut RoundTripDocument,
    provider_id: &str,
    config: &Value,
) -> Result<(), AppError> {
    let (root_pairs, _) = object_parts_mut(&mut document.value)
        .map_err(|_| AppError::Config("Pi models.json root must be a JSON object".to_string()))?;
    let providers_index = find_object_pair_index(root_pairs, "providers");
    if let Some(index) = providers_index {
        let provider_value = parse_round_trip_value(config, 4)?;
        let (provider_pairs, _) = object_parts_mut(&mut root_pairs[index].value)?;
        if let Some(provider_index) = find_object_pair_index(provider_pairs, provider_id) {
            provider_pairs[provider_index].value = provider_value;
        } else {
            append_object_pair(&mut root_pairs[index].value, provider_id, provider_value, 4)?;
        }
        return Ok(());
    }

    let mut providers = RoundTripValue::JSONObject {
        key_value_pairs: Vec::new(),
        context: Some(JSONObjectContext {
            wsc: (String::new(),),
        }),
    };
    append_object_pair(
        &mut providers,
        provider_id,
        parse_round_trip_value(config, 4)?,
        4,
    )?;
    append_object_pair(&mut document.value, "providers", providers, 2)
}

fn remove_round_trip_provider(
    document: &mut RoundTripDocument,
    provider_id: &str,
) -> Result<(), AppError> {
    let (root_pairs, _) = object_parts_mut(&mut document.value)
        .map_err(|_| AppError::Config("Pi models.json root must be a JSON object".to_string()))?;
    let Some(providers_index) = find_object_pair_index(root_pairs, "providers") else {
        return Ok(());
    };
    let (provider_pairs, provider_context) =
        object_parts_mut(&mut root_pairs[providers_index].value)?;
    let Some(remove_index) = find_object_pair_index(provider_pairs, provider_id) else {
        return Ok(());
    };

    let removed = provider_pairs.remove(remove_index);
    let removed_context = removed.context.ok_or_else(|| {
        AppError::Config("Pi models.json round-trip context is missing".to_string())
    })?;
    if remove_index < provider_pairs.len() {
        let following_trivia = removed_context.wsc.3.unwrap_or(removed_context.wsc.2);
        let next_context = provider_pairs[remove_index]
            .context
            .as_mut()
            .ok_or_else(|| {
                AppError::Config("Pi models.json round-trip context is missing".to_string())
            })?;
        next_context.wsc.0 = format!("{following_trivia}{}", next_context.wsc.0);
        return Ok(());
    }

    let closing = removed_context.wsc.3.unwrap_or(removed_context.wsc.2);
    if let Some(previous) = provider_pairs.last_mut() {
        let context = previous.context.as_mut().ok_or_else(|| {
            AppError::Config("Pi models.json round-trip context is missing".to_string())
        })?;
        context.wsc.2.push_str(&closing);
        context.wsc.3 = None;
    } else {
        provider_context.wsc.0.push_str(&closing);
    }
    Ok(())
}

fn render_models_jsonc(
    snapshot: &JsonFileSnapshot,
    next: &Value,
    mutations: &[ModelsMutation],
) -> Result<Vec<u8>, AppError> {
    if next == &snapshot.value {
        return Ok(snapshot.raw.clone().unwrap_or_default());
    }

    let Some(raw) = snapshot
        .raw
        .as_deref()
        .filter(|raw| !raw.iter().all(|byte| byte.is_ascii_whitespace()))
    else {
        return serialize_json(next);
    };
    let source = std::str::from_utf8(raw)
        .map_err(|error| jsonc_error(&snapshot.path, format!("invalid UTF-8: {error}")))?;
    let mut document =
        parse_round_trip(source).map_err(|error| jsonc_error(&snapshot.path, error))?;
    for mutation in mutations {
        match mutation {
            ModelsMutation::Upsert {
                provider_id,
                config,
            } => upsert_round_trip_provider(&mut document, provider_id, config)?,
            ModelsMutation::Remove { provider_id } => {
                remove_round_trip_provider(&mut document, provider_id)?
            }
        }
    }

    let rendered = document.to_string().into_bytes();
    let reparsed = parse_pi_jsonc(&snapshot.path, &rendered)?;
    if reparsed != *next {
        return Err(AppError::Config(
            "Refusing to write Pi models.json because round-trip output changed unrelated data"
                .to_string(),
        ));
    }
    Ok(rendered)
}

fn commit_documents(
    models_snapshot: &JsonFileSnapshot,
    models_next: &Value,
    auth_snapshot: &JsonFileSnapshot,
    auth_next: &Value,
    settings_snapshot: &JsonFileSnapshot,
    settings_next: &Value,
    models_mutations: &[ModelsMutation],
) -> Result<(), AppError> {
    let models_changed = models_next != &models_snapshot.value;
    let auth_changed = auth_next != &auth_snapshot.value;
    let settings_changed = settings_next != &settings_snapshot.value;
    if !models_changed && !auth_changed && !settings_changed {
        return Ok(());
    }

    for snapshot in [auth_snapshot, models_snapshot, settings_snapshot] {
        ensure_unchanged(snapshot)?;
    }

    let auth_raw = auth_changed
        .then(|| serialize_json(auth_next))
        .transpose()?;
    let models_raw = models_changed
        .then(|| render_models_jsonc(models_snapshot, models_next, models_mutations))
        .transpose()?;
    let settings_raw = settings_changed
        .then(|| serialize_json(settings_next))
        .transpose()?;

    let mut documents = Vec::with_capacity(3);
    if let Some(next) = auth_raw.as_deref() {
        parse_snapshot_value(&auth_snapshot.path, next, auth_snapshot.kind)?;
        documents.push(PendingDocument {
            snapshot: auth_snapshot,
            next,
            secure_if_new: true,
        });
    }
    if let Some(next) = models_raw.as_deref() {
        let reparsed = parse_snapshot_value(&models_snapshot.path, next, models_snapshot.kind)?;
        if reparsed != *models_next {
            return Err(AppError::Config(
                "Refusing to write Pi models.json because it failed validation".to_string(),
            ));
        }
        documents.push(PendingDocument {
            snapshot: models_snapshot,
            next,
            secure_if_new: false,
        });
    }
    if let Some(next) = settings_raw.as_deref() {
        parse_snapshot_value(&settings_snapshot.path, next, settings_snapshot.kind)?;
        documents.push(PendingDocument {
            snapshot: settings_snapshot,
            next,
            secure_if_new: false,
        });
    }

    let snapshots = documents
        .iter()
        .map(|document| document.snapshot)
        .collect::<Vec<_>>();
    create_backup(&snapshots)?;
    for snapshot in [auth_snapshot, models_snapshot, settings_snapshot] {
        ensure_unchanged(snapshot)?;
    }
    apply_documents_with(&documents, |document| {
        ensure_unchanged(document.snapshot)?;
        write_pending_document(document)
    })
}

fn mutate_pi_documents<F>(mutate: F) -> Result<(), AppError>
where
    F: FnOnce(&mut Value, &mut Value, &mut Value, &mut Vec<ModelsMutation>) -> Result<(), AppError>,
{
    let _guard = lock_pi_config()?;
    let models_snapshot = read_snapshot(&get_pi_models_path(), JsonFileKind::ModelsJsonc)?;
    let auth_snapshot = read_snapshot(&get_pi_auth_path(), JsonFileKind::Json)?;
    let settings_snapshot = read_snapshot(&get_pi_settings_path(), JsonFileKind::Json)?;
    let mut models_next = models_snapshot.value.clone();
    let mut auth_next = auth_snapshot.value.clone();
    let mut settings_next = settings_snapshot.value.clone();
    let mut models_mutations = Vec::new();

    mutate(
        &mut models_next,
        &mut auth_next,
        &mut settings_next,
        &mut models_mutations,
    )?;
    commit_documents(
        &models_snapshot,
        &models_next,
        &auth_snapshot,
        &auth_next,
        &settings_snapshot,
        &settings_next,
        &models_mutations,
    )
}

fn object_mut<'a>(
    value: &'a mut Value,
    path: &Path,
    description: &str,
) -> Result<&'a mut Map<String, Value>, AppError> {
    value.as_object_mut().ok_or_else(|| {
        AppError::Config(format!(
            "{description} must be a JSON object: {}",
            path.display()
        ))
    })
}

fn first_string<'a>(object: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn validate_pi_api(value: Option<&Value>, field: &str) -> Result<(), AppError> {
    let Some(value) = value else {
        return Ok(());
    };
    let api = value
        .as_str()
        .ok_or_else(|| AppError::Config(format!("Pi provider field '{field}' must be a string")))?;
    if api.trim().is_empty() {
        return Err(AppError::Config(format!(
            "Pi provider field '{field}' must not be empty"
        )));
    }
    Ok(())
}

fn validate_pi_headers(value: Option<&Value>, field: &str) -> Result<(), AppError> {
    let Some(value) = value else {
        return Ok(());
    };
    let headers = value.as_object().ok_or_else(|| {
        AppError::Config(format!("Pi provider field '{field}' must be an object"))
    })?;
    if let Some((name, _)) = headers
        .iter()
        .find(|(name, value)| name.trim().is_empty() || !value.is_string())
    {
        return Err(AppError::Config(format!(
            "Pi provider header '{field}.{name}' must have a non-empty name and string value"
        )));
    }
    Ok(())
}

/// Validate the Pi fields CC Switch understands while allowing unknown fields
/// to pass through for forward compatibility with newer Pi releases.
pub(crate) fn validate_pi_provider_config(config: &Value) -> Result<(), AppError> {
    let source = config.as_object().ok_or_else(|| {
        AppError::Config("Pi provider settings must be a JSON object".to_string())
    })?;

    let mut base_urls = Vec::with_capacity(2);
    for field in ["baseUrl", "baseURL"] {
        if let Some(value) = source.get(field) {
            let base_url = value.as_str().ok_or_else(|| {
                AppError::Config(format!("Pi provider field '{field}' must be a string"))
            })?;
            let base_url = base_url.trim();
            if base_url.is_empty() {
                return Err(AppError::Config(format!(
                    "Pi provider field '{field}' must not be empty"
                )));
            }
            base_urls.push((field, base_url));
        }
    }
    if base_urls.len() == 2 && base_urls[0].1 != base_urls[1].1 {
        return Err(AppError::Config(
            "Pi provider fields 'baseUrl' and 'baseURL' must not conflict".to_string(),
        ));
    }

    validate_pi_api(source.get("api"), "api")?;
    if source.get("apiKey").is_some_and(|value| !value.is_string()) {
        return Err(AppError::Config(
            "Pi provider field 'apiKey' must be a string".to_string(),
        ));
    }
    if let Some(oauth) = source.get("oauth") {
        if oauth.as_str() != Some("radius") {
            return Err(AppError::Config(
                "Pi provider field 'oauth' currently supports only 'radius'".to_string(),
            ));
        }
        if base_urls.is_empty() {
            return Err(AppError::Config(
                "Pi provider field 'baseUrl' is required when 'oauth' is configured".to_string(),
            ));
        }
    }
    if source
        .get("authHeader")
        .is_some_and(|value| !value.is_boolean())
    {
        return Err(AppError::Config(
            "Pi provider field 'authHeader' must be a boolean".to_string(),
        ));
    }
    validate_pi_headers(source.get("headers"), "headers")?;
    for field in ["compat", "modelOverrides"] {
        if source.get(field).is_some_and(|value| !value.is_object()) {
            return Err(AppError::Config(format!(
                "Pi provider field '{field}' must be an object"
            )));
        }
    }

    if source
        .get("defaultModel")
        .is_some_and(|value| !value.is_string())
    {
        return Err(AppError::Config(
            "Pi provider field 'defaultModel' must be a string".to_string(),
        ));
    }
    let Some(models_value) = source.get("models") else {
        return Ok(());
    };
    let models = models_value.as_array().ok_or_else(|| {
        AppError::Config("Pi provider field 'models' must be an array".to_string())
    })?;

    // Whether baseUrl/api/defaultModel may be inherited depends on Pi's
    // evolving built-in provider catalog, so this fragment validator must not
    // hard-code those contextual requirements.
    let mut model_ids = HashSet::with_capacity(models.len());
    for (index, model) in models.iter().enumerate() {
        let model = model.as_object().ok_or_else(|| {
            AppError::Config(format!(
                "Pi provider field 'models[{index}]' must be an object with an id"
            ))
        })?;
        let id = model.get("id").and_then(Value::as_str).ok_or_else(|| {
            AppError::Config(format!(
                "Pi provider field 'models[{index}].id' must be a string"
            ))
        })?;
        validate_pi_api(model.get("api"), &format!("models[{index}].api"))?;
        let id = id.trim();
        if id.is_empty() {
            return Err(AppError::Config(format!(
                "Pi provider field 'models[{index}].id' must not be empty"
            )));
        }
        if !model_ids.insert(id) {
            return Err(AppError::Config(format!(
                "Pi provider contains duplicate model id '{id}'"
            )));
        }
    }

    Ok(())
}

fn model_ids_from_config(config: &Value) -> Vec<String> {
    config
        .get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|model| model.get("id").and_then(Value::as_str))
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn normalized_models_from_config(config: &Value) -> Option<Vec<Value>> {
    config
        .get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|model| {
                    let object = model.as_object()?;
                    let id = object.get("id").and_then(Value::as_str)?.trim();
                    if id.is_empty() {
                        return None;
                    }

                    let mut normalized = object.clone();
                    normalized.insert("id".to_string(), Value::String(id.to_string()));
                    Some(Value::Object(normalized))
                })
                .collect()
        })
}

fn normalize_provider_config(config: &Value) -> Result<Value, AppError> {
    let Some(source) = config.as_object() else {
        return Err(AppError::Config(
            "Pi provider settings must be a JSON object".to_string(),
        ));
    };
    validate_pi_provider_config(config)?;

    let mut output = source.clone();

    if let Some(base_url) = first_string(source, &["baseUrl", "baseURL"]) {
        output.insert("baseUrl".to_string(), Value::String(base_url.to_string()));
        output.remove("baseURL");
    }

    if let Some(models) = normalized_models_from_config(config) {
        output.insert("models".to_string(), Value::Array(models));
    }

    // Pi stores the selected model in settings.json, not per provider.
    output.remove("defaultModel");
    // Pi's managed API keys live in auth.json. Legacy inline keys are migrated
    // the next time the provider is updated by CC Switch.
    output.remove("apiKey");

    Ok(Value::Object(output))
}

fn default_model_for_provider(config: &Value) -> Option<String> {
    config
        .get("defaultModel")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| model_ids_from_config(config).into_iter().next())
}

fn providers_mut<'a>(
    models_root: &'a mut Value,
    models_path: &Path,
) -> Result<&'a mut Map<String, Value>, AppError> {
    let root = object_mut(models_root, models_path, "Pi models.json root")?;
    let providers = root
        .entry("providers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    object_mut(providers, models_path, "Pi models.json providers")
}

fn insert_provider(
    models_root: &mut Value,
    models_path: &Path,
    provider: &Provider,
    overwrite_existing: bool,
    mutations: &mut Vec<ModelsMutation>,
) -> Result<(), AppError> {
    let providers = providers_mut(models_root, models_path)?;
    if providers.contains_key(&provider.id) && !overwrite_existing {
        return Err(AppError::Config(format!(
            "Pi provider '{}' already exists in models.json and is not managed by CC Switch",
            provider.id
        )));
    }

    let normalized = normalize_provider_config(&provider.settings_config)?;
    providers.insert(provider.id.clone(), normalized.clone());
    mutations.push(ModelsMutation::Upsert {
        provider_id: provider.id.clone(),
        config: normalized,
    });
    Ok(())
}

fn update_provider_auth(auth_root: &mut Value, provider: &Provider) -> Result<(), AppError> {
    let auth_path = get_pi_auth_path();
    let auth = object_mut(auth_root, &auth_path, "Pi auth.json root")?;
    let requested_key = provider
        .settings_config
        .get("apiKey")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();

    match auth.get_mut(&provider.id) {
        Some(credential) => {
            let credential_type = credential.get("type").and_then(Value::as_str);
            if credential_type != Some("api_key") {
                if requested_key.is_empty() {
                    return Ok(());
                }
                return Err(AppError::Config(format!(
                    "Pi provider '{}' uses OAuth or an unsupported credential type in auth.json. CC Switch will not overwrite it with an API key.",
                    provider.id
                )));
            }
            if requested_key.is_empty() {
                auth.remove(&provider.id);
            } else {
                *credential = json!({ "type": "api_key", "key": requested_key });
            }
        }
        None if !requested_key.is_empty() => {
            auth.insert(
                provider.id.clone(),
                json!({ "type": "api_key", "key": requested_key }),
            );
        }
        None => {}
    }
    Ok(())
}

fn remove_provider_auth(auth_root: &mut Value, provider_id: &str) -> Result<(), AppError> {
    let auth_path = get_pi_auth_path();
    let auth = object_mut(auth_root, &auth_path, "Pi auth.json root")?;
    if auth
        .get(provider_id)
        .and_then(|credential| credential.get("type"))
        .and_then(Value::as_str)
        == Some("api_key")
    {
        auth.remove(provider_id);
    }
    Ok(())
}

fn select_provider(settings_root: &mut Value, provider: &Provider) -> Result<(), AppError> {
    let settings_path = get_pi_settings_path();
    let settings = object_mut(settings_root, &settings_path, "Pi settings.json root")?;
    settings.insert(
        "defaultProvider".to_string(),
        Value::String(provider.id.clone()),
    );
    if let Some(default_model) = default_model_for_provider(&provider.settings_config) {
        settings.insert("defaultModel".to_string(), Value::String(default_model));
    } else {
        settings.remove("defaultModel");
    }
    Ok(())
}

/// Add or update a provider in Pi's additive registry without selecting it.
pub fn upsert_pi_live_provider(
    provider: &Provider,
    overwrite_existing: bool,
) -> Result<(), AppError> {
    mutate_pi_documents(|models_root, auth_root, _, mutations| {
        insert_provider(
            models_root,
            &get_pi_models_path(),
            provider,
            overwrite_existing,
            mutations,
        )?;
        update_provider_auth(auth_root, provider)
    })
}

/// Add or update a provider and select it as Pi's default provider/model.
pub fn activate_pi_live_provider(
    provider: &Provider,
    overwrite_existing: bool,
) -> Result<(), AppError> {
    mutate_pi_documents(|models_root, auth_root, settings_root, mutations| {
        insert_provider(
            models_root,
            &get_pi_models_path(),
            provider,
            overwrite_existing,
            mutations,
        )?;
        update_provider_auth(auth_root, provider)?;
        select_provider(settings_root, provider)
    })
}

/// Compatibility wrapper retained for callers that mean "activate".
pub fn write_pi_live_provider(provider: &Provider) -> Result<(), AppError> {
    activate_pi_live_provider(provider, true)
}

/// Synchronize all CC Switch-managed providers and the selected default.
pub fn sync_pi_live_providers(
    providers: &[&Provider],
    active_provider: Option<&Provider>,
) -> Result<(), AppError> {
    mutate_pi_documents(|models_root, auth_root, settings_root, mutations| {
        let models_path = get_pi_models_path();
        for provider in providers {
            insert_provider(models_root, &models_path, provider, true, mutations)?;
            update_provider_auth(auth_root, provider)?;
        }
        if let Some(active_provider) = active_provider {
            select_provider(settings_root, active_provider)?;
        }
        Ok(())
    })
}

pub fn pi_provider_exists(provider_id: &str) -> Result<bool, AppError> {
    let _guard = lock_pi_config()?;
    let models_path = get_pi_models_path();
    let models_root = read_json_or_empty_object(&models_path, JsonFileKind::ModelsJsonc)?;
    let root = models_root.as_object().ok_or_else(|| {
        AppError::Config(format!(
            "Pi models.json root must be a JSON object: {}",
            models_path.display()
        ))
    })?;
    Ok(root
        .get("providers")
        .and_then(Value::as_object)
        .is_some_and(|providers| providers.contains_key(provider_id)))
}

/// Remove a managed provider, but never leave Pi's default pointing at a
/// provider that no longer exists.
pub fn remove_pi_live_provider(provider_id: &str) -> Result<(), AppError> {
    mutate_pi_documents(|models_root, auth_root, settings_root, mutations| {
        if settings_root.get("defaultProvider").and_then(Value::as_str) == Some(provider_id) {
            return Err(AppError::Config(format!(
                "Cannot remove active Pi provider '{provider_id}'. Switch first."
            )));
        }

        providers_mut(models_root, &get_pi_models_path())?.remove(provider_id);
        mutations.push(ModelsMutation::Remove {
            provider_id: provider_id.to_string(),
        });
        remove_provider_auth(auth_root, provider_id)?;
        Ok(())
    })
}

fn provider_models_for_form(provider_config: &Value) -> Value {
    let models = provider_config
        .get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|model| model.as_object().cloned().map(Value::Object))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Value::Array(models)
}

fn provider_config_for_storage(
    provider_id: &str,
    provider_config: &Value,
    auth_root: &Value,
    default_model: Option<&str>,
) -> Result<Value, AppError> {
    validate_pi_provider_config(provider_config)?;
    let mut form_config = provider_config.clone();
    let object = form_config.as_object_mut().ok_or_else(|| {
        AppError::Config(format!(
            "Pi provider '{provider_id}' in models.json must be a JSON object"
        ))
    })?;
    if let Some(base_url) = object.get("baseURL").cloned() {
        object.insert("baseUrl".to_string(), base_url);
    }
    object.insert(
        "models".to_string(),
        provider_models_for_form(provider_config),
    );
    if let Some(api_key) = auth_root
        .get(provider_id)
        .filter(|credential| credential.get("type").and_then(Value::as_str) == Some("api_key"))
        .and_then(|credential| credential.get("key"))
        .and_then(Value::as_str)
    {
        object.insert("apiKey".to_string(), Value::String(api_key.to_string()));
    }
    if let Some(default_model) = default_model {
        object.insert(
            "defaultModel".to_string(),
            Value::String(default_model.to_string()),
        );
    }
    Ok(form_config)
}

#[derive(Debug)]
pub struct PiLiveRegistry {
    pub providers: Vec<Provider>,
    pub default_provider: Option<String>,
}

pub fn read_pi_live_providers() -> Result<PiLiveRegistry, AppError> {
    let _guard = lock_pi_config()?;
    let models_path = get_pi_models_path();
    let auth_path = get_pi_auth_path();
    let settings_path = get_pi_settings_path();
    let models_root = read_json_or_empty_object(&models_path, JsonFileKind::ModelsJsonc)?;
    let auth_root = read_json_or_empty_object(&auth_path, JsonFileKind::Json)?;
    let settings_root = read_json_or_empty_object(&settings_path, JsonFileKind::Json)?;
    if !auth_root.is_object() {
        return Err(AppError::Config(format!(
            "Pi auth.json root must be a JSON object: {}",
            auth_path.display()
        )));
    }
    if !settings_root.is_object() {
        return Err(AppError::Config(format!(
            "Pi settings.json root must be a JSON object: {}",
            settings_path.display()
        )));
    }
    let default_provider = settings_root
        .get("defaultProvider")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let default_model = settings_root
        .get("defaultModel")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let providers = models_root
        .get("providers")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            AppError::Config(format!(
                "Pi models.json does not define a providers object: {}",
                models_path.display()
            ))
        })?;

    let mut imported = Vec::with_capacity(providers.len());
    for (provider_id, provider_config) in providers {
        let selected_model = (default_provider.as_deref() == Some(provider_id.as_str()))
            .then_some(default_model)
            .flatten();
        let settings_config =
            provider_config_for_storage(provider_id, provider_config, &auth_root, selected_model)?;
        let name = provider_config
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(provider_id)
            .to_string();
        imported.push(Provider::with_id(
            provider_id.clone(),
            name,
            settings_config,
            None,
        ));
    }

    Ok(PiLiveRegistry {
        providers: imported,
        default_provider,
    })
}

pub fn read_pi_live_settings() -> Result<Value, AppError> {
    let registry = read_pi_live_providers()?;
    let default_provider = registry.default_provider.ok_or_else(|| {
        AppError::Config("Pi settings.json does not define defaultProvider".to_string())
    })?;
    let provider = registry
        .providers
        .into_iter()
        .find(|provider| provider.id == default_provider)
        .ok_or_else(|| {
            AppError::Config(format!(
                "Pi models.json does not define provider '{default_provider}'"
            ))
        })?;
    let default_model = provider.settings_config.get("defaultModel").cloned();

    Ok(json!({
        "defaultProvider": default_provider,
        "defaultModel": default_model.unwrap_or(Value::Null),
        "providerConfig": provider.settings_config
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn normalize_provider_config_writes_pi_base_url_key() {
        let config = json!({
            "baseURL": "https://api.example.com/v1",
            "apiKey": "sk-test",
            "api": "openai-completions",
            "models": [],
            "futurePiField": { "keep": true }
        });

        let normalized = normalize_provider_config(&config).unwrap();

        assert_eq!(
            normalized.get("baseUrl"),
            Some(&json!("https://api.example.com/v1"))
        );
        assert!(
            normalized.get("baseURL").is_none(),
            "Pi models.json must use baseUrl, not baseURL"
        );
        assert!(
            normalized.get("apiKey").is_none(),
            "Pi API keys must be stored in auth.json"
        );
        assert_eq!(
            normalized.pointer("/futurePiField/keep"),
            Some(&json!(true)),
            "unknown Pi fields must survive normalization"
        );
    }

    #[test]
    fn accepts_non_empty_pi_and_extension_api_identifiers() {
        for api in [
            "openai-completions",
            "openai-codex-responses",
            "extension-owned-api",
        ] {
            validate_pi_provider_config(&json!({
                "baseUrl": "https://api.example.com/v1",
                "api": api,
                "models": [{ "id": "model-1" }]
            }))
            .unwrap_or_else(|error| panic!("{api} should be accepted: {error}"));
        }

        validate_pi_provider_config(&json!({
            "baseUrl": "https://api.example.com/v1",
            "models": [{ "id": "model-1", "api": "anthropic-messages" }]
        }))
        .expect("model-level API should satisfy Pi's API requirement");
    }

    #[test]
    fn allows_builtin_overrides_and_future_fields() {
        validate_pi_provider_config(&json!({
            "baseUrl": "https://proxy.example.com/v1",
            "oauth": "radius",
            "authHeader": true,
            "headers": { "x-route": "$PI_ROUTE" },
            "compat": { "supportsDeveloperRole": false },
            "modelOverrides": {
                "known-model": { "contextWindow": 200000 }
            },
            "futurePiField": { "enabled": true }
        }))
        .expect("built-in overrides and unknown future fields should be accepted");

        validate_pi_provider_config(&json!({
            "models": [{ "id": "custom-model" }],
            "defaultModel": "built-in-model"
        }))
        .expect("built-in providers may inherit endpoint, API, and model catalog entries");
    }

    #[test]
    fn rejects_invalid_pi_provider_schema() {
        let cases = [
            (json!({ "api": "" }), "must not be empty"),
            (
                json!({
                    "baseUrl": "",
                    "api": "openai-completions"
                }),
                "baseUrl",
            ),
            (
                json!({
                    "baseUrl": "https://api.example.com/v1",
                    "models": [{ "name": "missing-id" }]
                }),
                "models[0].id",
            ),
            (
                json!({
                    "baseUrl": "https://api.example.com/v1",
                    "models": ["legacy-shorthand"]
                }),
                "must be an object with an id",
            ),
            (
                json!({
                    "baseUrl": "https://api.example.com/v1",
                    "api": "openai-completions",
                    "models": [{ "id": "same" }, { "id": "same" }]
                }),
                "duplicate model id",
            ),
            (
                json!({
                    "baseUrl": "https://api.example.com/v1",
                    "api": "openai-completions",
                    "models": [{ "id": "model-1" }],
                    "defaultModel": 42
                }),
                "defaultModel",
            ),
            (
                json!({
                    "baseUrl": "https://api.example.com/v1",
                    "api": "openai-completions",
                    "headers": { "x-route": 42 },
                    "models": [{ "id": "model-1" }]
                }),
                "string value",
            ),
        ];

        for (config, expected) in cases {
            let error =
                validate_pi_provider_config(&config).expect_err("invalid config must be rejected");
            assert!(
                error.to_string().contains(expected),
                "expected '{expected}' in error, got {error}"
            );
        }
    }

    #[test]
    fn pi_jsonc_accepts_line_comments_and_trailing_commas_only() {
        let path = Path::new("models.json");
        let parsed = parse_pi_jsonc(
            path,
            br#"{
  // registry comment
  "providers": {
    "custom": {
      "models": [{ "id": "model-1", },],
    },
  },
}"#,
        )
        .expect("Pi JSONC should parse");
        assert_eq!(
            parsed.pointer("/providers/custom/models/0/id"),
            Some(&json!("model-1"))
        );

        for invalid in [
            "{'providers': {}}",
            "{providers: {}}",
            "{/* comment */ \"providers\": {}}",
        ] {
            assert!(
                parse_pi_jsonc(path, invalid.as_bytes()).is_err(),
                "full JSON5 syntax must be rejected: {invalid}"
            );
        }
    }

    #[test]
    fn round_trip_replaces_only_the_target_provider() {
        let dir = tempdir().expect("create temp dir");
        let path = dir.path().join("models.json");
        let original = r#"// file comment
{
  "futureRoot": { "keep": true },
  "providers": {
    // other provider comment
    "other": { "future": 42 },
    "target": {
      // comments inside the replaced provider are not guaranteed
      "baseUrl": "https://old.example/v1",
      "models": [{ "id": "old" }],
    },
  },
}
// trailing file comment
"#;
        fs::write(&path, original).expect("seed models");
        let snapshot = read_snapshot(&path, JsonFileKind::ModelsJsonc).expect("snapshot models");
        let mut next = snapshot.value.clone();
        let config = json!({
            "baseUrl": "https://new.example/v1",
            "api": "extension-api",
            "models": [{ "id": "new" }]
        });
        next.pointer_mut("/providers/target")
            .map(|target| *target = config.clone())
            .expect("target provider");
        let rendered = render_models_jsonc(
            &snapshot,
            &next,
            &[ModelsMutation::Upsert {
                provider_id: "target".to_string(),
                config,
            }],
        )
        .expect("render models");
        let rendered = String::from_utf8(rendered).expect("utf-8 models");

        assert!(rendered.contains("// file comment"));
        assert!(rendered.contains("// other provider comment"));
        assert!(rendered.contains("// trailing file comment"));
        assert!(rendered.contains("\"futureRoot\": { \"keep\": true }"));
        assert!(rendered.contains("\"other\": { \"future\": 42 }"));
        assert!(!rendered.contains("https://old.example/v1"));
        assert_eq!(
            parse_pi_jsonc(&path, rendered.as_bytes()).expect("reparse rendered models"),
            next
        );
    }

    #[test]
    fn round_trip_adds_and_removes_a_provider_without_losing_surrounding_comments() {
        let dir = tempdir().expect("create temp dir");
        let path = dir.path().join("models.json");
        let original = r#"// file comment
{
  "providers": {
    // existing provider comment
    "existing": { "future": true },
  },
  "futureRoot": 42,
}
// trailing file comment
"#;
        fs::write(&path, original).expect("seed models");
        let snapshot = read_snapshot(&path, JsonFileKind::ModelsJsonc).expect("snapshot models");
        let config = json!({
            "baseUrl": "https://new.example/v1",
            "api": "extension-api",
            "models": [{ "id": "new" }]
        });
        let mut with_added = snapshot.value.clone();
        with_added
            .pointer_mut("/providers")
            .and_then(Value::as_object_mut)
            .expect("providers object")
            .insert("added".to_string(), config.clone());
        let added = render_models_jsonc(
            &snapshot,
            &with_added,
            &[ModelsMutation::Upsert {
                provider_id: "added".to_string(),
                config,
            }],
        )
        .expect("add provider");
        let added_text = String::from_utf8(added.clone()).expect("utf-8 models");
        for comment in [
            "// file comment",
            "// existing provider comment",
            "// trailing file comment",
        ] {
            assert!(added_text.contains(comment), "missing {comment}");
        }
        assert_eq!(
            parse_pi_jsonc(&path, &added).expect("parse added provider"),
            with_added
        );

        fs::write(&path, added).expect("seed added models");
        let added_snapshot =
            read_snapshot(&path, JsonFileKind::ModelsJsonc).expect("snapshot added models");
        let mut with_removed = added_snapshot.value.clone();
        with_removed
            .pointer_mut("/providers")
            .and_then(Value::as_object_mut)
            .expect("providers object")
            .remove("added");
        let removed = render_models_jsonc(
            &added_snapshot,
            &with_removed,
            &[ModelsMutation::Remove {
                provider_id: "added".to_string(),
            }],
        )
        .expect("remove provider");
        let removed_text = String::from_utf8(removed.clone()).expect("utf-8 models");
        for comment in [
            "// file comment",
            "// existing provider comment",
            "// trailing file comment",
        ] {
            assert!(removed_text.contains(comment), "missing {comment}");
        }
        assert_eq!(
            parse_pi_jsonc(&path, &removed).expect("parse removed provider"),
            with_removed
        );
    }

    #[test]
    fn round_trip_removes_a_nonfinal_provider_without_losing_the_next_comment() {
        let cases = [
            (
                "leading",
                r#"{
  "providers": {
    "remove": { "models": [{ "id": "old" }] },
    // comment owned by the provider that remains
    "keep": { "future": true },
  },
}
"#,
            ),
            (
                "middle",
                r#"{
  "providers": {
    "first": { "future": "first" },
    "remove": { "models": [{ "id": "old" }] },
    // comment owned by the provider that remains
    "keep": { "future": true },
  },
}
"#,
            ),
        ];

        for (case, original) in cases {
            let dir = tempdir().expect("create temp dir");
            let path = dir.path().join("models.json");
            fs::write(&path, original).expect("seed models");
            let snapshot =
                read_snapshot(&path, JsonFileKind::ModelsJsonc).expect("snapshot models");
            let mut next = snapshot.value.clone();
            next.pointer_mut("/providers")
                .and_then(Value::as_object_mut)
                .expect("providers object")
                .remove("remove");

            let removed = render_models_jsonc(
                &snapshot,
                &next,
                &[ModelsMutation::Remove {
                    provider_id: "remove".to_string(),
                }],
            )
            .unwrap_or_else(|error| panic!("remove {case} provider: {error}"));
            let removed_text = String::from_utf8(removed.clone()).expect("utf-8 models");

            assert!(
                removed_text.contains("// comment owned by the provider that remains"),
                "{case} removal lost the next provider's comment"
            );
            assert_eq!(
                parse_pi_jsonc(&path, &removed).expect("parse removed provider"),
                next,
                "{case} removal changed the semantic configuration"
            );
        }
    }

    #[test]
    fn auth_updates_protect_oauth_and_clear_only_api_keys() {
        let oauth = json!({
            "type": "oauth",
            "accessToken": "keep-oauth"
        });
        let mut auth = json!({ "oauth-provider": oauth.clone() });
        let empty_provider = Provider::with_id(
            "oauth-provider".to_string(),
            "OAuth".to_string(),
            json!({ "apiKey": "" }),
            None,
        );
        update_provider_auth(&mut auth, &empty_provider).expect("empty key preserves OAuth");
        assert_eq!(auth.get("oauth-provider"), Some(&oauth));

        let key_provider = Provider::with_id(
            "oauth-provider".to_string(),
            "OAuth".to_string(),
            json!({ "apiKey": "replace" }),
            None,
        );
        assert!(update_provider_auth(&mut auth, &key_provider)
            .expect_err("OAuth replacement must be blocked")
            .to_string()
            .contains("will not overwrite"));
        assert_eq!(auth.get("oauth-provider"), Some(&oauth));

        auth.as_object_mut().expect("auth object").insert(
            "api-provider".to_string(),
            json!({ "type": "api_key", "key": "old" }),
        );
        remove_provider_auth(&mut auth, "api-provider").expect("remove API key");
        remove_provider_auth(&mut auth, "oauth-provider").expect("preserve OAuth on delete");
        assert!(auth.get("api-provider").is_none());
        assert_eq!(auth.get("oauth-provider"), Some(&oauth));
    }

    #[test]
    fn transaction_detects_external_changes_to_each_pi_file() {
        for changed_file in ["models", "auth", "settings"] {
            let dir = tempdir().expect("create temp dir");
            let models_path = dir.path().join("models.json");
            let auth_path = dir.path().join("auth.json");
            let settings_path = dir.path().join("settings.json");
            let original_models = br#"{"providers":{}}"#;
            let original_auth = br#"{}"#;
            let original_settings = br#"{}"#;
            fs::write(&models_path, original_models).expect("seed models");
            fs::write(&auth_path, original_auth).expect("seed auth");
            fs::write(&settings_path, original_settings).expect("seed settings");
            let models_snapshot =
                read_snapshot(&models_path, JsonFileKind::ModelsJsonc).expect("snapshot models");
            let auth_snapshot =
                read_snapshot(&auth_path, JsonFileKind::Json).expect("snapshot auth");
            let settings_snapshot =
                read_snapshot(&settings_path, JsonFileKind::Json).expect("snapshot settings");
            let mut models_next = models_snapshot.value.clone();
            models_next
                .pointer_mut("/providers")
                .and_then(Value::as_object_mut)
                .expect("providers object")
                .insert("managed".to_string(), json!({}));
            let external = match changed_file {
                "models" => br#"{"providers":{"external":{}}}"#.as_slice(),
                "auth" => br#"{"external":{"type":"api_key","key":"external"}}"#.as_slice(),
                "settings" => br#"{"defaultProvider":"external"}"#.as_slice(),
                _ => unreachable!(),
            };
            let changed_path = match changed_file {
                "models" => &models_path,
                "auth" => &auth_path,
                "settings" => &settings_path,
                _ => unreachable!(),
            };
            fs::write(changed_path, external).expect("simulate external edit");

            let error = commit_documents(
                &models_snapshot,
                &models_next,
                &auth_snapshot,
                &auth_snapshot.value,
                &settings_snapshot,
                &settings_snapshot.value,
                &[ModelsMutation::Upsert {
                    provider_id: "managed".to_string(),
                    config: json!({}),
                }],
            )
            .expect_err("external edit must abort the entire transaction");

            assert!(error.to_string().contains("changed on disk"));
            assert_eq!(fs::read(changed_path).unwrap(), external);
            if changed_file != "models" {
                assert_eq!(fs::read(&models_path).unwrap(), original_models);
            }
            if changed_file != "auth" {
                assert_eq!(fs::read(&auth_path).unwrap(), original_auth);
            }
            if changed_file != "settings" {
                assert_eq!(fs::read(&settings_path).unwrap(), original_settings);
            }
        }
    }

    #[test]
    fn rolls_back_auth_and_models_when_settings_write_fails() {
        let dir = tempdir().expect("create temp dir");
        let auth_path = dir.path().join("auth.json");
        let models_path = dir.path().join("models.json");
        let settings_path = dir.path().join("settings.json");
        let original_auth = br#"{"old":{"type":"api_key","key":"old"}}"#;
        let original_models = br#"{"providers":{"old":{}}}"#;
        let original_settings = br#"{"defaultProvider":"old"}"#;
        fs::write(&auth_path, original_auth).expect("seed auth");
        fs::write(&models_path, original_models).expect("seed models");
        fs::write(&settings_path, original_settings).expect("seed settings");
        let auth_snapshot = read_snapshot(&auth_path, JsonFileKind::Json).expect("snapshot auth");
        let models_snapshot =
            read_snapshot(&models_path, JsonFileKind::ModelsJsonc).expect("snapshot models");
        let settings_snapshot =
            read_snapshot(&settings_path, JsonFileKind::Json).expect("snapshot settings");
        let auth_next = br#"{"new":{"type":"api_key","key":"new"}}"#;
        let models_next = br#"{"providers":{"new":{}}}"#;
        let settings_next = br#"{"defaultProvider":"new"}"#;
        let documents = [
            PendingDocument {
                snapshot: &auth_snapshot,
                next: auth_next,
                secure_if_new: false,
            },
            PendingDocument {
                snapshot: &models_snapshot,
                next: models_next,
                secure_if_new: false,
            },
            PendingDocument {
                snapshot: &settings_snapshot,
                next: settings_next,
                secure_if_new: false,
            },
        ];

        let error = apply_documents_with(&documents, |document| {
            if document.snapshot.path == settings_path {
                return Err(AppError::Message(
                    "injected settings write failure".to_string(),
                ));
            }
            atomic_write(&document.snapshot.path, document.next)
        })
        .expect_err("settings write should fail");

        assert!(error
            .to_string()
            .contains("injected settings write failure"));
        assert_eq!(fs::read(&auth_path).unwrap(), original_auth);
        assert_eq!(fs::read(&models_path).unwrap(), original_models);
        assert_eq!(fs::read(&settings_path).unwrap(), original_settings);
    }
}
