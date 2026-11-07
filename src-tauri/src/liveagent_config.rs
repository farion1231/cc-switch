//! LiveAgent's K-brain backend owns config.json (JSONC), not config.sqlite.
//! Providers and MCP share one document and one write lock; MCP moves to the
//! `config.json.live-mcp.json` companion once LiveAgent has saved MCP settings.
//! Never touch legacy DBs.
use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

static CONFIG_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn data_dir() -> Result<PathBuf, AppError> {
    for name in ["LIVEAGENT_HOME", "K_BRAIN_HOME"] {
        if let Some(value) = std::env::var_os(name).filter(|v| !v.is_empty()) {
            let path = PathBuf::from(value);
            if !path.is_absolute() {
                return Err(AppError::Config(format!(
                    "{name} must be an absolute directory"
                )));
            }
            return Ok(path);
        }
    }
    let root = get_home_dir().join(".liveagent");
    Ok(resolve_pointer(&root))
}

fn resolve_pointer(root: &Path) -> PathBuf {
    fs::read(root.join("data-dir.json"))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .and_then(|v| v.get("path").and_then(Value::as_str).map(PathBuf::from))
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| root.to_owned())
}

pub(crate) fn config_path() -> Result<PathBuf, AppError> {
    Ok(data_dir()?.join("config.json"))
}

fn bytes(path: &Path) -> Result<Option<Vec<u8>>, AppError> {
    match fs::read(path) {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AppError::io(path, e)),
    }
}

fn parse(raw: Option<&[u8]>) -> Result<Value, AppError> {
    let Some(raw) = raw else {
        return Ok(json!({}));
    };
    let text =
        std::str::from_utf8(raw).map_err(|_| AppError::Config("Invalid LiveAgent UTF-8".into()))?;
    let value: Value = json5::from_str(text).map_err(|_| {
        AppError::Config("Invalid LiveAgent JSONC; configuration was not changed".into())
    })?;
    if !value.is_object()
        || ["providers", "mcp"]
            .iter()
            .any(|key| value.get(key).is_some_and(|v| !v.is_object()))
    {
        return Err(AppError::Config(
            "LiveAgent providers and mcp must be objects".into(),
        ));
    }
    Ok(value)
}

pub(crate) fn read() -> Result<Value, AppError> {
    let _guard = CONFIG_LOCK
        .lock()
        .map_err(|e| AppError::Config(e.to_string()))?;
    parse(bytes(&config_path()?)?.as_deref())
}

/// Keep the file lock through the database commit. On failure restore only our
/// own write, never a newer edit made by LiveAgent or another process.
pub(crate) fn update_and_commit<T>(
    edit: impl FnOnce(&mut Value) -> Result<(), AppError>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = CONFIG_LOCK
        .lock()
        .map_err(|e| AppError::Config(e.to_string()))?;
    let data_dir = data_dir()?;
    edit_file(&data_dir.join("config.json"), Some(&data_dir), edit, commit)
}

/// MCP settings edited in LiveAgent live in this companion file. Once it exists
/// K-brain ignores `config.json.mcp` entirely and only falls back to it while the
/// file is missing (K-brain `cmd/kn/backend.go`, `internal/mcp/live.go`).
const MCP_SIDECAR: &str = "config.json.live-mcp.json";

/// MCP servers as `config.json.mcp`-style entries keyed by id, read from whichever
/// file K-brain actually uses.
pub(crate) fn read_mcp() -> Result<Map<String, Value>, AppError> {
    let _guard = CONFIG_LOCK
        .lock()
        .map_err(|e| AppError::Config(e.to_string()))?;
    let data_dir = data_dir()?;
    if let Some(raw) = bytes(&data_dir.join(MCP_SIDECAR))? {
        return sidecar::entries(&parse(Some(&raw))?);
    }
    let document = parse(bytes(&data_dir.join("config.json"))?.as_deref())?;
    Ok(document
        .get("mcp")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default())
}

/// Edit MCP entries (in `config.json.mcp` form) in the file K-brain reads, with the
/// same running guard, conflict check and rollback as [`update_and_commit`].
pub(crate) fn update_mcp_and_commit<T>(
    edit: impl FnOnce(&mut Map<String, Value>) -> Result<(), AppError>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = CONFIG_LOCK
        .lock()
        .map_err(|e| AppError::Config(e.to_string()))?;
    let data_dir = data_dir()?;
    let sidecar_path = data_dir.join(MCP_SIDECAR);
    if sidecar_path.exists() {
        return edit_file(
            &sidecar_path,
            Some(&data_dir),
            |doc| {
                let mut entries = sidecar::entries(doc)?;
                edit(&mut entries)?;
                sidecar::apply(doc, &entries)
            },
            commit,
        );
    }
    edit_file(
        &data_dir.join("config.json"),
        Some(&data_dir),
        |doc| {
            let had_mcp = doc.get("mcp").is_some();
            let mut entries = doc
                .get("mcp")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            edit(&mut entries)?;
            if had_mcp || !entries.is_empty() {
                doc["mcp"] = Value::Object(entries);
            }
            Ok(())
        },
        commit,
    )
}

/// Conversion between K-brain's `LiveSettings` (`{servers:[{id, transport,
/// command, args, …}], selected}`) and `config.json.mcp` entries, so one editor
/// serves both files. Servers we do not change are written back verbatim.
mod sidecar {
    use super::*;

    fn servers(doc: &Value) -> Result<&[Value], AppError> {
        match doc.get("servers") {
            None | Some(Value::Null) => Ok(&[]),
            Some(Value::Array(servers)) => Ok(servers),
            Some(_) => Err(AppError::Config(
                "LiveAgent MCP settings servers must be an array".into(),
            )),
        }
    }

    fn id(server: &Value) -> Option<&str> {
        server
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
    }

    fn entry(server: &Value) -> Result<Value, AppError> {
        let mut obj = server
            .as_object()
            .cloned()
            .ok_or_else(|| AppError::Config("Invalid LiveAgent MCP server".into()))?;
        obj.remove("id");
        let transport = obj.remove("transport");
        let command = obj.remove("command");
        let args = obj.remove("args");
        let http = match transport.as_ref().and_then(Value::as_str) {
            Some("http") => true,
            Some(t) if !t.is_empty() => false,
            // K-brain infers the transport from a non-empty url.
            _ => obj
                .get("url")
                .and_then(Value::as_str)
                .is_some_and(|u| !u.is_empty()),
        };
        if !http {
            obj.remove("url");
            if let Some(command) = command
                .as_ref()
                .and_then(Value::as_str)
                .filter(|c| !c.trim().is_empty())
            {
                let mut argv = vec![json!(command)];
                if let Some(args) = args.as_ref().and_then(Value::as_array) {
                    argv.extend(args.iter().cloned());
                }
                obj.insert("command".into(), json!(argv));
            }
        }
        // Go decodes a missing `enabled` as false.
        let enabled = obj.get("enabled").and_then(Value::as_bool).unwrap_or(false);
        obj.insert("enabled".into(), json!(enabled));
        Ok(Value::Object(obj))
    }

    pub(super) fn entries(doc: &Value) -> Result<Map<String, Value>, AppError> {
        let mut out = Map::new();
        for server in servers(doc)? {
            // K-brain drops servers without an id and keeps the first duplicate.
            let Some(id) = id(server) else { continue };
            if !out.contains_key(id) {
                out.insert(id.into(), entry(server)?);
            }
        }
        Ok(out)
    }

    fn server(id: &str, entry: &Value) -> Value {
        let mut obj = entry.as_object().cloned().unwrap_or_default();
        let command = obj.remove("command");
        let startup = obj.remove("startupTimeout");
        let note = obj.remove("note");
        obj.remove("toolTimeout");
        obj.remove("type");
        let mut out = Map::new();
        out.insert("id".into(), json!(id));
        out.insert(
            "enabled".into(),
            obj.remove("enabled").unwrap_or(json!(true)),
        );
        match command.as_ref().and_then(Value::as_array) {
            Some(argv) if !argv.is_empty() => {
                out.insert("transport".into(), json!("stdio"));
                out.insert("command".into(), argv[0].clone());
                if argv.len() > 1 {
                    out.insert("args".into(), json!(&argv[1..]));
                }
                obj.remove("url");
            }
            _ if obj.contains_key("url") => {
                out.insert("transport".into(), json!("http"));
            }
            _ => {
                out.insert("transport".into(), json!("stdio"));
            }
        }
        if let Some(seconds) = startup.as_ref().and_then(Value::as_u64) {
            obj.insert("timeoutMs".into(), json!(seconds.saturating_mul(1000)));
        }
        if let Some(note) = note.filter(Value::is_string) {
            obj.entry("description").or_insert(note);
        }
        out.extend(obj);
        Value::Object(out)
    }

    pub(super) fn apply(doc: &mut Value, next: &Map<String, Value>) -> Result<(), AppError> {
        let before = entries(doc)?;
        let mut seen = std::collections::HashSet::new();
        let mut servers_out = Vec::new();
        for server in servers(doc)? {
            let Some(id) = id(server) else {
                servers_out.push(server.clone());
                continue;
            };
            if !seen.insert(id.to_string()) {
                servers_out.push(server.clone());
                continue;
            }
            match next.get(id) {
                Some(entry) if before.get(id) == Some(entry) => servers_out.push(server.clone()),
                Some(entry) => servers_out.push(self::server(id, entry)),
                None => {}
            }
        }
        for (id, entry) in next {
            if !seen.contains(id.as_str()) {
                servers_out.push(server(id, entry));
            }
        }
        let removed: Vec<&String> = before.keys().filter(|id| !next.contains_key(*id)).collect();
        if let Some(selected) = doc.get_mut("selected").and_then(Value::as_array_mut) {
            selected.retain(|id| !removed.iter().any(|r| id.as_str() == Some(r.as_str())));
        }
        doc["servers"] = Value::Array(servers_out);
        Ok(())
    }
}

/// `running_guard`: K-brain keeps config.json in memory while its backend runs and
/// writes that snapshot back on its next save, silently dropping our edit. Refuse
/// to change the file then. skills-settings.json is re-read on every use.
fn edit_file<T>(
    path: &Path,
    running_guard: Option<&Path>,
    edit: impl FnOnce(&mut Value) -> Result<(), AppError>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let original = bytes(path)?;
    let mut document = parse(original.as_deref())?;
    let before = document.clone();
    edit(&mut document)?;
    if document == before {
        return commit();
    }
    if let Some(root) = running_guard {
        if running::backend_running(root) {
            return Err(AppError::localized(
                "liveagent_running",
                "LiveAgent 正在运行，它会用内存中的配置覆盖这次修改。请先退出 LiveAgent 再修改模型或 MCP。",
                "LiveAgent is running and would overwrite this change with its in-memory settings. Quit LiveAgent before changing models or MCP servers.",
            ));
        }
    }
    let mut next =
        serde_json::to_vec_pretty(&document).map_err(|e| AppError::Config(e.to_string()))?;
    next.push(b'\n');
    if bytes(path)? != original {
        return Err(AppError::Conflict(
            "LiveAgent configuration changed outside CC Switch; reload and retry".into(),
        ));
    }
    atomic_write_private(path, &next)?;
    match commit() {
        Ok(result) => Ok(result),
        Err(error) => {
            if bytes(path)?.as_deref() != Some(next.as_slice()) {
                return Err(AppError::Conflict(format!("{error}; LiveAgent changed the file before rollback; newer configuration preserved")));
            }
            match original {
                Some(raw) => atomic_write_private(path, &raw)?,
                None => fs::remove_file(path).map_err(|e| AppError::io(path, e))?,
            }
            Err(error)
        }
    }
}

pub(crate) fn get_providers() -> Result<IndexMap<String, Value>, AppError> {
    let document = read()?;
    if document
        .get("providers")
        .and_then(Value::as_object)
        .is_some_and(|providers| providers.values().any(|p| !p.is_object()))
    {
        return Err(AppError::Config(
            "Each LiveAgent provider must be an object".into(),
        ));
    }
    Ok(document
        .get("providers")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(id, v)| (id.clone(), v.clone()))
        .collect())
}

#[derive(Clone, Copy)]
enum Kind {
    Str,
    Bool,
    Int,
    Number,
    Strings,
    Object,
}

/// JSON fields of K-brain's `config.Provider` (pinned 7db2a37). K-brain decodes
/// config.json with `DisallowUnknownFields`: one stray key stops LiveAgent loading
/// its whole configuration, so nothing outside these lists may be written.
const PROVIDER_FIELDS: &[(&str, Kind)] = &[
    ("name", Kind::Str),
    ("type", Kind::Str),
    ("baseUrl", Kind::Str),
    ("isFullUrl", Kind::Bool),
    ("modelsUrl", Kind::Str),
    ("api", Kind::Str),
    ("apiKey", Kind::Str),
    ("activeModels", Kind::Strings),
    ("modelOrder", Kind::Strings),
    ("requestFormat", Kind::Str),
    ("reasoning", Kind::Str),
    ("promptCacheHintMode", Kind::Str),
    ("nativeWebSearchEnabled", Kind::Bool),
    ("useSystemProxy", Kind::Bool),
    ("metadata", Kind::Object),
    ("promptCachingEnabled", Kind::Bool),
    ("promptCacheRetention", Kind::Str),
    ("retryPolicy", Kind::Object),
    ("usageQuery", Kind::Object),
    ("cacheSessionAffinity", Kind::Bool),
    ("cacheControlFormat", Kind::Str),
];
/// `config.PiModel`.
const MODEL_FIELDS: &[(&str, Kind)] = &[
    ("id", Kind::Str),
    ("name", Kind::Str),
    ("displayName", Kind::Str),
    ("ownedBy", Kind::Str),
    ("limitsSource", Kind::Str),
    ("outputModalities", Kind::Strings),
    ("imageAdapter", Kind::Str),
    ("inputModalities", Kind::Strings),
    ("api", Kind::Str),
    ("reasoning", Kind::Bool),
    ("input", Kind::Strings),
    ("contextWindow", Kind::Int),
    ("maxTokens", Kind::Int),
];
const HEADER_FIELDS: &[(&str, Kind)] = &[("key", Kind::Str), ("value", Kind::Str)];
const SAMPLING_FIELDS: &[(&str, Kind)] = &[("temperature", Kind::Number), ("top_p", Kind::Number)];
const CACHE_CAPABILITY_FIELDS: &[(&str, Kind)] = &[
    ("supportsPromptCacheKey", Kind::Bool),
    ("supportsLongRetention", Kind::Bool),
    ("sessionAffinityFormat", Kind::Str),
    ("responsesCacheOptions", Kind::Bool),
];
/// Provider `type` values for which K-brain derives `api` when it is empty.
const TYPED_PROVIDERS: &[&str] = &["claude_code", "gemini", "xai", "codex", "deepseek"];
const APIS: &[&str] = &[
    "openai-completions",
    "openai-responses",
    "anthropic-messages",
    "google-generative-ai",
    "google-gemini",
];

fn kind_matches(value: &Value, kind: Kind) -> bool {
    // Go's decoder leaves null as the zero value for every field type.
    value.is_null()
        || match kind {
            Kind::Str => value.is_string(),
            Kind::Bool => value.is_boolean(),
            // K-brain additionally rejects negative limits.
            Kind::Int => value.as_u64().is_some_and(|v| v <= i64::MAX as u64),
            Kind::Number => value.is_number(),
            Kind::Strings => value
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_string)),
            Kind::Object => value.is_object(),
        }
}

fn check_fields(
    value: &Value,
    fields: &[(&str, Kind)],
    extra: &[&str],
    what: &str,
) -> Result<(), AppError> {
    let object = value
        .as_object()
        .ok_or_else(|| AppError::InvalidInput(format!("LiveAgent {what} must be an object")))?;
    for (key, field) in object {
        if extra.contains(&key.as_str()) {
            continue;
        }
        let Some((_, kind)) = fields.iter().find(|(name, _)| name == key) else {
            return Err(AppError::InvalidInput(format!(
                "LiveAgent does not support the {what} field \"{key}\""
            )));
        };
        if !kind_matches(field, *kind) {
            return Err(AppError::InvalidInput(format!(
                "LiveAgent {what} field \"{key}\" has the wrong type"
            )));
        }
    }
    Ok(())
}

fn check_api(api: Option<&Value>, what: &str) -> Result<(), AppError> {
    match api.and_then(Value::as_str).map(str::trim) {
        None | Some("") => Ok(()),
        Some(api) if APIS.contains(&api) => Ok(()),
        Some(_) => Err(AppError::InvalidInput(format!(
            "Unsupported LiveAgent API format for {what}"
        ))),
    }
}

pub(crate) fn validate_provider(id: &str, provider: &Value) -> Result<(), AppError> {
    if id.trim().is_empty() || !provider.is_object() {
        return Err(AppError::InvalidInput(
            "LiveAgent provider requires a key and an object".into(),
        ));
    }
    check_fields(
        provider,
        PROVIDER_FIELDS,
        &["customHeaders", "cacheCapabilities", "models"],
        "provider",
    )?;
    if let Some(headers) = provider.get("customHeaders").filter(|v| !v.is_null()) {
        for header in headers.as_array().ok_or_else(|| {
            AppError::InvalidInput("LiveAgent customHeaders must be an array".into())
        })? {
            check_fields(header, HEADER_FIELDS, &[], "custom header")?;
        }
    }
    if let Some(caps) = provider.get("cacheCapabilities").filter(|v| !v.is_null()) {
        check_fields(caps, CACHE_CAPABILITY_FIELDS, &[], "cacheCapabilities")?;
    }
    let api = provider
        .get("api")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let typed = provider
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|t| TYPED_PROVIDERS.contains(&t));
    if api.is_empty() && !typed {
        return Err(AppError::InvalidInput(
            "Unsupported LiveAgent API format".into(),
        ));
    }
    check_api(provider.get("api"), "provider")?;
    let url = provider
        .get("baseUrl")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !url.is_empty()
        && !url::Url::parse(url)
            .is_ok_and(|u| matches!(u.scheme(), "https" | "http") && u.host_str().is_some())
    {
        return Err(AppError::InvalidInput(
            "LiveAgent baseUrl must be an HTTP(S) URL".into(),
        ));
    }
    let models = provider
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::InvalidInput("LiveAgent models must be an array".into()))?;
    let mut ids = std::collections::HashSet::new();
    for model in models {
        check_fields(model, MODEL_FIELDS, &["samplingParams"], "model")?;
        if let Some(sampling) = model.get("samplingParams").filter(|v| !v.is_null()) {
            check_fields(sampling, SAMPLING_FIELDS, &[], "samplingParams")?;
        }
        let id = model
            .get("id")
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| AppError::InvalidInput("Each LiveAgent model requires an id".into()))?;
        if !ids.insert(id) {
            return Err(AppError::InvalidInput(
                "Duplicate LiveAgent model id".into(),
            ));
        }
        check_api(model.get("api"), "model")?;
    }
    if let Some(active) = provider.get("activeModels").filter(|v| !v.is_null()) {
        if active.as_array().is_some_and(|items| {
            items
                .iter()
                .any(|v| v.as_str().is_none_or(|id| !ids.contains(id)))
        }) {
            return Err(AppError::InvalidInput(
                "LiveAgent activeModels must reference configured models".into(),
            ));
        }
    }
    Ok(())
}

pub(crate) fn provider_map(
    doc: &mut Value,
) -> Result<&mut serde_json::Map<String, Value>, AppError> {
    if doc.get("providers").is_none() {
        doc["providers"] = json!({});
    }
    doc["providers"]
        .as_object_mut()
        .ok_or_else(|| AppError::Config("Invalid LiveAgent providers".into()))
}

/// Refuse to leave a dangling default/compaction/task/reviewer route.
pub(crate) fn check_routes(
    doc: &Value,
    id: &str,
    replacement: Option<&Value>,
) -> Result<(), AppError> {
    for (provider_field, model_field) in [
        ("defaultProvider", "defaultModel"),
        ("compactProvider", "compactModel"),
        ("taskProvider", "taskModel"),
    ] {
        if doc.get(provider_field).and_then(Value::as_str) != Some(id) {
            continue;
        }
        let model = doc
            .get(model_field)
            .and_then(Value::as_str)
            .unwrap_or_default();
        if replacement.is_none_or(|p| !model_available(p, model)) {
            return Err(AppError::InvalidInput(format!("LiveAgent {provider_field}/{model_field} references this provider; select another model in LiveAgent before removing it")));
        }
    }
    if let Some(approval) = doc.get("aiApproval") {
        if approval.get("provider").and_then(Value::as_str) == Some(id)
            && replacement.is_none_or(|p| {
                !model_available(
                    p,
                    approval
                        .get("model")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                )
            })
        {
            return Err(AppError::InvalidInput(
                "LiveAgent AI approval uses this model; change its route first".into(),
            ));
        }
    }
    Ok(())
}

fn model_available(provider: &Value, model: &str) -> bool {
    provider
        .get("models")
        .and_then(Value::as_array)
        .is_some_and(|ms| {
            ms.iter()
                .any(|m| m.get("id").and_then(Value::as_str) == Some(model))
        })
        && provider
            .get("activeModels")
            .and_then(Value::as_array)
            .is_none_or(|ms| ms.iter().any(|m| m.as_str() == Some(model)))
}

/// Detects a running K-brain backend that serves this data directory.
///
/// `ps` text loses argv boundaries, the process's working directory and its
/// environment, so guessing the root from it either misses the same directory
/// spelled differently or blocks unrelated ones. Read each process's exact argv,
/// cwd and launch environment instead and resolve the root as K-brain does.
pub(crate) mod running {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    #[cfg(test)]
    pub(crate) static FORCE_RUNNING: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);

    /// What we could learn about one process. `argv` is `None` when the process
    /// is K-brain by name but its arguments are unreadable.
    #[derive(Debug, Default)]
    pub(crate) struct Proc {
        pub argv: Option<Vec<String>>,
        pub cwd: Option<PathBuf>,
        pub env: HashMap<String, String>,
    }

    pub(super) fn backend_running(data_dir: &Path) -> bool {
        #[cfg(test)]
        {
            let _ = data_dir;
            FORCE_RUNNING.load(std::sync::atomic::Ordering::SeqCst)
        }
        #[cfg(not(test))]
        {
            processes().iter().any(|p| serves(p, data_dir))
        }
    }

    fn is_kbrain(program: &str) -> bool {
        let name = program
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let name = name.strip_suffix(".exe").unwrap_or(&name);
        matches!(name, "k-brain" | "kn")
    }

    /// Whether `proc` is a K-brain backend whose data root is `data_dir`. When
    /// that cannot be determined, assume it is: a missed backend loses edits.
    pub(crate) fn serves(proc: &Proc, data_dir: &Path) -> bool {
        let Some(argv) = &proc.argv else {
            return true;
        };
        if !(argv.len() >= 2 && is_kbrain(&argv[0]) && argv[1] == "backend") {
            return false;
        }
        match backend_root(&argv[2..], proc.cwd.as_deref(), &proc.env) {
            Some(root) => same_dir(&root, data_dir),
            None => true,
        }
    }

    /// K-brain `cmd/kn/backend.go` and `internal/datapath`: `-data-dir` (made
    /// absolute against the process cwd), else `LIVEAGENT_HOME`, else
    /// `K_BRAIN_HOME`, else `$HOME/.liveagent`. Go's flag package lets the last
    /// occurrence win and stops at the first non-flag argument.
    pub(crate) fn backend_root(
        args: &[String],
        cwd: Option<&Path>,
        env: &HashMap<String, String>,
    ) -> Option<PathBuf> {
        let mut data_dir = None;
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            if arg == "--" || !arg.starts_with('-') || arg == "-" {
                break;
            }
            let flag = arg.trim_start_matches('-');
            let (name, inline) = match flag.split_once('=') {
                Some((name, value)) => (name, Some(value.to_string())),
                None => (flag, None),
            };
            if name == "data-dir" {
                match inline {
                    Some(value) => data_dir = Some(value),
                    None => {
                        i += 1;
                        data_dir = Some(args.get(i)?.clone());
                    }
                }
            } else if inline.is_none()
                && !matches!(name, "parent-stdio" | "prepare-storage" | "h" | "help")
            {
                // Every other backend flag takes a value.
                i += 1;
            }
            i += 1;
        }
        let absolute = |value: &str| -> Option<PathBuf> {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                Some(path)
            } else {
                Some(cwd?.join(path))
            }
        };
        if let Some(value) = data_dir.filter(|v| !v.is_empty()) {
            return absolute(&value);
        }
        for name in ["LIVEAGENT_HOME", "K_BRAIN_HOME"] {
            if let Some(value) = env.get(name).map(|v| v.trim()).filter(|v| !v.is_empty()) {
                return absolute(value);
            }
        }
        env.get("HOME")
            .filter(|h| !h.is_empty())
            .map(|home| Path::new(home).join(".liveagent"))
    }

    /// Same directory despite spelling: `dir/`, `dir//`, `dir/./`, `..`, symlinks.
    fn same_dir(a: &Path, b: &Path) -> bool {
        match (a.canonicalize(), b.canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            // `components()` already folds repeated separators and `.` segments.
            _ => a.components().eq(b.components()),
        }
    }

    #[cfg(all(target_os = "macos", not(test)))]
    fn processes() -> Vec<Proc> {
        use std::ffi::CStr;
        let mut pids = vec![0 as libc::pid_t; 8192];
        let n = unsafe {
            libc::proc_listallpids(
                pids.as_mut_ptr().cast(),
                (pids.len() * std::mem::size_of::<libc::pid_t>()) as i32,
            )
        };
        if n <= 0 {
            return Vec::new();
        }
        pids.truncate(n as usize);
        let mut out = Vec::new();
        for pid in pids.into_iter().filter(|p| *p > 0) {
            let mut name = [0u8; 256];
            let len = unsafe { libc::proc_name(pid, name.as_mut_ptr().cast(), name.len() as u32) };
            if len <= 0 || !is_kbrain(&String::from_utf8_lossy(&name[..len as usize])) {
                continue;
            }
            let (argv, env) = match procargs(pid) {
                Some((argv, env)) => (Some(argv), env),
                None => (None, HashMap::new()),
            };
            let cwd = {
                let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
                let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as i32;
                let got = unsafe {
                    libc::proc_pidinfo(
                        pid,
                        libc::PROC_PIDVNODEPATHINFO,
                        0,
                        (&mut info as *mut libc::proc_vnodepathinfo).cast(),
                        size,
                    )
                };
                (got == size).then(|| {
                    let raw = unsafe { CStr::from_ptr(info.pvi_cdir.vip_path.as_ptr().cast()) };
                    PathBuf::from(raw.to_string_lossy().into_owned())
                })
            };
            out.push(Proc { argv, cwd, env });
        }
        out
    }

    /// `KERN_PROCARGS2`: argc, exec path, NUL padding, argv[argc], then env.
    #[cfg(all(target_os = "macos", not(test)))]
    fn procargs(pid: libc::pid_t) -> Option<(Vec<String>, HashMap<String, String>)> {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        let mut size: libc::size_t = 0;
        let ok = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if ok != 0 || size < 4 {
            return None;
        }
        let mut buf = vec![0u8; size];
        let ok = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                buf.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if ok != 0 || size < 4 {
            return None;
        }
        buf.truncate(size);
        let argc = i32::from_ne_bytes(buf[..4].try_into().ok()?) as usize;
        let mut rest = &buf[4..];
        // Skip the exec path and its padding.
        let end = rest.iter().position(|b| *b == 0)?;
        rest = &rest[end..];
        let start = rest.iter().position(|b| *b != 0)?;
        rest = &rest[start..];
        let mut strings = rest
            .split(|b| *b == 0)
            .map(|s| String::from_utf8_lossy(s).into_owned());
        let argv: Vec<String> = strings.by_ref().take(argc).collect();
        if argv.len() != argc {
            return None;
        }
        let env = strings
            .take_while(|s| !s.is_empty())
            .filter_map(|kv| {
                kv.split_once('=')
                    .map(|(k, v)| (k.to_string(), v.to_string()))
            })
            .collect();
        Some((argv, env))
    }

    #[cfg(all(target_os = "linux", not(test)))]
    fn processes() -> Vec<Proc> {
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let dir = entry.path();
            if !entry
                .file_name()
                .to_string_lossy()
                .bytes()
                .all(|b| b.is_ascii_digit())
            {
                continue;
            }
            let comm = std::fs::read_to_string(dir.join("comm")).unwrap_or_default();
            let cmdline = std::fs::read(dir.join("cmdline")).ok();
            let argv: Option<Vec<String>> = cmdline.filter(|c| !c.is_empty()).map(|c| {
                c.strip_suffix(&[0])
                    .unwrap_or(&c)
                    .split(|b| *b == 0)
                    .map(|s| String::from_utf8_lossy(s).into_owned())
                    .collect()
            });
            let named = is_kbrain(comm.trim())
                || argv
                    .as_ref()
                    .and_then(|a| a.first())
                    .is_some_and(|p| is_kbrain(p));
            if !named {
                continue;
            }
            let env = std::fs::read(dir.join("environ"))
                .map(|raw| {
                    raw.split(|b| *b == 0)
                        .filter_map(|kv| {
                            let kv = String::from_utf8_lossy(kv);
                            kv.split_once('=')
                                .map(|(k, v)| (k.to_string(), v.to_string()))
                        })
                        .collect()
                })
                .unwrap_or_default();
            out.push(Proc {
                argv,
                cwd: std::fs::read_link(dir.join("cwd")).ok(),
                env,
            });
        }
        out
    }

    /// Windows: `tasklist` has no command lines; any K-brain process counts.
    #[cfg(all(windows, not(test)))]
    fn processes() -> Vec<Proc> {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let Ok(output) = std::process::Command::new("tasklist")
            .args(["/FO", "CSV", "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(std::process::Stdio::null())
            .output()
        else {
            return Vec::new();
        };
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.split(',').next())
            .filter(|image| is_kbrain(image.trim_matches('"')))
            .map(|_| Proc::default())
            .collect()
    }

    #[cfg(all(not(any(target_os = "macos", target_os = "linux", windows)), not(test)))]
    fn processes() -> Vec<Proc> {
        Vec::new()
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeSettings {
    pub config_path: String,
    pub default_provider: String,
    pub default_model: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveAgentStatus {
    /// The LiveAgent app or its data directory is present.
    pub installed: bool,
    /// K-brain's config.json exists, so CC Switch can manage it.
    pub configured: bool,
    pub config_path: String,
}

/// Cheap presence check for the Apps page; never parses config.json.
pub(crate) fn status() -> Result<LiveAgentStatus, AppError> {
    let dir = data_dir()?;
    let config = dir.join("config.json");
    let bundles: &[PathBuf] = if cfg!(target_os = "macos") {
        &[
            PathBuf::from("/Applications/LiveAgent.app"),
            get_home_dir().join("Applications").join("LiveAgent.app"),
        ]
    } else {
        &[]
    };
    let configured = config.is_file();
    Ok(LiveAgentStatus {
        installed: configured || dir.is_dir() || bundles.iter().any(|b| b.exists()),
        configured,
        config_path: config.to_string_lossy().into(),
    })
}

pub(crate) fn native_settings() -> Result<NativeSettings, AppError> {
    let doc = read()?;
    Ok(NativeSettings {
        config_path: config_path()?.to_string_lossy().into(),
        default_provider: doc["defaultProvider"].as_str().unwrap_or_default().into(),
        default_model: doc["defaultModel"].as_str().unwrap_or_default().into(),
    })
}

pub(crate) fn set_default(provider: &str, model: &str) -> Result<(), AppError> {
    update_and_commit(
        |doc| {
            if !doc
                .get("providers")
                .and_then(|p| p.get(provider))
                .is_some_and(|p| model_available(p, model))
            {
                return Err(AppError::InvalidInput(
                    "Select an enabled LiveAgent model".into(),
                ));
            }
            doc["defaultProvider"] = json!(provider);
            doc["defaultModel"] = json!(model);
            Ok(())
        },
        || Ok(()),
    )
}

/// K-brain's `validManagedName`: 1–64 lowercase letters, digits or single hyphens.
pub(crate) fn valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

#[cfg(test)]
pub(crate) fn update_skill_selection_and_commit<T>(
    name: &str,
    enabled: bool,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    update_skill_selections_and_commit(&[(name, enabled)], commit)
}

pub(crate) fn update_skill_selections_and_commit<T>(
    changes: &[(&str, bool)],
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    for (name, enabled) in changes {
        if *enabled && !valid_skill_name(name) {
            return Err(AppError::InvalidInput(
                "LiveAgent skill names require 1–64 lowercase letters, digits or single hyphens"
                    .into(),
            ));
        }
    }
    let _guard = CONFIG_LOCK
        .lock()
        .map_err(|e| AppError::Config(e.to_string()))?;
    let path = data_dir()?.join("skills-settings.json");
    edit_file(
        &path,
        None,
        |doc| {
            if doc.get("global").is_none() {
                doc["global"] = json!({"enabled":true,"selected":[]});
            }
            if doc.get("projects").is_none() {
                doc["projects"] = json!({});
            }
            let global = doc["global"].as_object_mut().ok_or_else(|| {
                AppError::Config("Invalid LiveAgent global skill settings".into())
            })?;
            // A missing flag means enabled, as in K-brain's loadSettings default.
            if changes.iter().any(|(_, enabled)| *enabled)
                && global.get("enabled").and_then(Value::as_bool) == Some(false)
            {
                return Err(AppError::InvalidInput(
                    "Skills are globally disabled in LiveAgent; enable them in LiveAgent first"
                        .into(),
                ));
            }
            if global.get("selected").is_none_or(Value::is_null) {
                global.insert("selected".into(), json!([]));
            }
            let selected = global
                .get_mut("selected")
                .unwrap()
                .as_array_mut()
                .ok_or_else(|| AppError::Config("Invalid LiveAgent selected skills".into()))?;
            if selected.iter().any(|s| !s.is_string()) {
                return Err(AppError::Config(
                    "Invalid LiveAgent selected skill name".into(),
                ));
            }
            let previous = selected.clone();
            for (name, enabled) in changes {
                let was_selected = selected.iter().any(|s| s.as_str() == Some(*name));
                if *enabled && !was_selected {
                    selected.push(json!(name));
                }
                if !enabled {
                    selected.retain(|s| s.as_str() != Some(*name));
                }
            }
            if *selected != previous {
                let revision = doc.get("revision").and_then(Value::as_u64).unwrap_or(0);
                doc["revision"] = json!(revision
                    .checked_add(1)
                    .ok_or_else(|| AppError::Config("LiveAgent skill revision overflow".into()))?);
            }
            Ok(())
        },
        commit,
    )
}

#[cfg(test)]
pub(crate) mod test_support {
    pub struct TestRoot {
        pub dir: tempfile::TempDir,
        previous: Option<std::ffi::OsString>,
    }
    impl TestRoot {
        pub fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let previous = std::env::var_os("LIVEAGENT_HOME");
            std::env::set_var("LIVEAGENT_HOME", dir.path());
            Self { dir, previous }
        }
    }
    impl Drop for TestRoot {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => std::env::set_var("LIVEAGENT_HOME", v),
                None => std::env::remove_var("LIVEAGENT_HOME"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_fields_follow_kbrain_strict_decoding() {
        let base = json!({"api":"openai-responses","baseUrl":"https://example.com/v1","apiKey":"k","models":[{"id":"m","contextWindow":1000,"maxTokens":100,"samplingParams":{"temperature":0.2}}],"customHeaders":[{"key":"X","value":"1"}],"cacheCapabilities":{"supportsPromptCacheKey":true},"usageQuery":{"enabled":false},"metadata":{"a":1}});
        validate_provider("p", &base).unwrap();
        for (path, value) in [
            ("/maxOutputToken", json!(1)),
            ("/models/0/maxOutputToken", json!(1)),
            ("/models/0/samplingParams/topP", json!(1)),
            ("/customHeaders/0/name", json!("x")),
            ("/cacheCapabilities/unknown", json!(true)),
        ] {
            let mut bad = base.clone();
            let (parent, key) = path.rsplit_once('/').unwrap();
            bad.pointer_mut(if parent.is_empty() { "" } else { parent })
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert(key.into(), value);
            assert!(
                validate_provider("p", &bad).is_err(),
                "{path} must be rejected"
            );
        }
        let mut wrong_type = base.clone();
        wrong_type["models"][0]["reasoning"] = json!("high");
        assert!(validate_provider("p", &wrong_type).is_err());
        // K-brain derives api from type for LiveAgent's built-in providers.
        validate_provider(
            "b",
            &json!({"type":"claude_code","baseUrl":"","apiKey":"","activeModels":null,"models":[]}),
        )
        .unwrap();
        assert!(validate_provider("b", &json!({"baseUrl":"","apiKey":"","models":[]})).is_err());
    }

    #[test]
    fn backend_root_follows_kbrain_resolution() {
        use running::{serves, Proc};
        use std::collections::HashMap;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("liveagent");
        std::fs::create_dir_all(root.join("child")).unwrap();
        // Directories whose names look like our root followed by a flag.
        std::fs::create_dir_all(tmp.path().join("liveagent -other")).unwrap();
        std::fs::create_dir_all(tmp.path().join("liveagent -parent-stdio")).unwrap();
        let r = root.display().to_string();
        let proc = |args: &[&str], cwd: Option<&Path>, env: &[(&str, &str)]| Proc {
            argv: Some(
                [
                    "/Applications/Live Agent.app/Contents/MacOS/k-brain",
                    "backend",
                ]
                .iter()
                .chain(args)
                .map(|s| s.to_string())
                .collect(),
            ),
            cwd: cwd.map(Path::to_path_buf),
            env: env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<HashMap<_, _>>(),
        };
        let listen = ["-listen", "127.0.0.1:0"];
        let with = |dir: &str| {
            let mut a = listen.to_vec();
            a.extend(["-data-dir", dir, "-parent-stdio"]);
            a.iter().map(|s| s.to_string()).collect::<Vec<_>>()
        };
        let p = |dir: String, cwd: Option<&Path>| {
            let args = with(&dir);
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            serves(&proc(&args, cwd, &[]), &root)
        };
        for same in [
            r.clone(),
            format!("{r}/"),
            format!("{r}//"),
            format!("{r}/./"),
            format!("{r}/child/.."),
        ] {
            assert!(p(same.clone(), None), "{same}");
        }
        assert!(p("liveagent".into(), Some(tmp.path())));
        assert!(p(".".into(), Some(&root)));
        for other in [
            format!("{r}/child"),
            format!("{r} -other"),
            format!("{r} -parent-stdio"),
            format!("{r} other"),
        ] {
            assert!(!p(other.clone(), None), "{other}");
        }
        assert!(serves(
            &proc(&[&format!("-data-dir={r}")], None, &[]),
            &root
        ));
        // A relative root with an unknown cwd cannot be ruled out.
        assert!(p("liveagent".into(), None));
        // Without -data-dir: LIVEAGENT_HOME, then K_BRAIN_HOME, then $HOME/.liveagent.
        let elsewhere = tmp.path().join("elsewhere");
        let e = elsewhere.display().to_string();
        assert!(serves(
            &proc(&listen, None, &[("LIVEAGENT_HOME", &r)]),
            &root
        ));
        assert!(serves(&proc(&listen, None, &[("K_BRAIN_HOME", &r)]), &root));
        assert!(!serves(
            &proc(&listen, None, &[("LIVEAGENT_HOME", &e)]),
            &root
        ));
        assert!(!serves(
            &proc(&listen, None, &[("K_BRAIN_HOME", &e)]),
            &root
        ));
        assert!(!serves(
            &proc(
                &listen,
                None,
                &[("LIVEAGENT_HOME", &e), ("K_BRAIN_HOME", &r)]
            ),
            &root
        ));
        let home = tmp.path().display().to_string();
        assert!(serves(
            &proc(&listen, None, &[("HOME", &home)]),
            &tmp.path().join(".liveagent")
        ));
        assert!(!serves(&proc(&listen, None, &[("HOME", &home)]), &root));
        // The flag wins over the environment.
        assert!(!serves(
            &proc(&["-data-dir", &e], None, &[("LIVEAGENT_HOME", &r)]),
            &root
        ));
        // Not a backend, or unknown process details.
        let not_backend = |argv: &[&str]| Proc {
            argv: Some(argv.iter().map(|s| s.to_string()).collect()),
            ..Default::default()
        };
        assert!(!serves(
            &not_backend(&["/x/k-brain", "run", "backend"]),
            &root
        ));
        assert!(!serves(
            &not_backend(&["/x/k-brain-computer", "backend"]),
            &root
        ));
        assert!(!serves(
            &not_backend(&["vim", &format!("{r}/config.json")]),
            &root
        ));
        // A backend whose root cannot be resolved (no flag, env or HOME) counts.
        assert!(serves(
            &not_backend(&["C:\\k\\k-brain.exe", "backend"]),
            &root
        ));
        assert!(serves(&Proc::default(), &root));
    }

    #[test]
    #[serial_test::serial]
    fn running_backend_blocks_config_changes_but_not_noops() {
        let root = test_support::TestRoot::new();
        let path = root.dir.path().join("config.json");
        fs::write(&path, "{\"providers\":{}}").unwrap();
        running::FORCE_RUNNING.store(true, std::sync::atomic::Ordering::SeqCst);
        let blocked = update_and_commit(
            |d| {
                d["defaultModel"] = json!("m");
                Ok(())
            },
            || Ok(()),
        );
        let unchanged = update_and_commit(|_| Ok(()), || Ok(()));
        let skill = update_skill_selection_and_commit("free", true, || Ok(()));
        running::FORCE_RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(blocked.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"providers\":{}}");
        unchanged.unwrap();
        skill.unwrap();
    }

    #[test]
    #[serial_test::serial]
    fn status_reports_presence_without_parsing_config() {
        let root = test_support::TestRoot::new();
        let before = status().unwrap();
        assert!(before.installed);
        assert!(!before.configured);
        // A broken config.json must not hide that LiveAgent is set up.
        fs::write(root.dir.path().join("config.json"), "{ broken").unwrap();
        let after = status().unwrap();
        assert!(after.installed && after.configured);
        assert!(after.config_path.ends_with("config.json"));
    }

    #[test]
    #[serial_test::serial]
    fn missing_global_enabled_means_enabled() {
        let root = test_support::TestRoot::new();
        let path = root.dir.path().join("skills-settings.json");
        fs::write(
            &path,
            "{\"revision\":1,\"global\":{\"selected\":[]},\"projects\":{}}",
        )
        .unwrap();
        update_skill_selection_and_commit("added", true, || Ok(())).unwrap();
        fs::write(
            &path,
            "{\"revision\":1,\"global\":{\"enabled\":false,\"selected\":[]},\"projects\":{}}",
        )
        .unwrap();
        assert!(update_skill_selection_and_commit("added", true, || Ok(())).is_err());
    }
    #[test]
    fn jsonc_merge_preserves_other_domains_and_rolls_back_exact_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        let original=b"// comment\n{\"providers\":{},\"mcp\":{\"docs\":{\"url\":\"https://example.com\"}},\"language\":\"zh_cn\",}";
        fs::write(&path, original).unwrap();
        let failed: Result<(), AppError> = edit_file(
            &path,
            None,
            |d| {
                d["providers"]["new"] = json!({"api":"openai-responses"});
                Ok(())
            },
            || Err(AppError::Config("database unavailable".into())),
        );
        assert!(failed.is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        edit_file(
            &path,
            None,
            |d| {
                d["providers"]["new"] = json!({"api":"openai-responses"});
                Ok(())
            },
            || Ok(()),
        )
        .unwrap();
        let doc = parse(Some(&fs::read(&path).unwrap())).unwrap();
        assert_eq!(doc["language"], "zh_cn");
        assert!(doc["mcp"]["docs"].is_object());
    }
    #[test]
    fn invalid_files_and_external_edits_are_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        fs::write(&path, "broken").unwrap();
        assert!(edit_file(&path, None, |_| Ok(()), || Ok(())).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "broken");
        fs::write(&path, "{}").unwrap();
        assert!(edit_file(
            &path,
            None,
            |d| {
                d["language"] = json!("en");
                fs::write(&path, "{\"language\":\"zh_cn\"}").unwrap();
                Ok(())
            },
            || Ok(())
        )
        .is_err());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{\"language\":\"zh_cn\"}"
        );
    }
    #[test]
    fn relocated_root_and_provider_routes() {
        let temp = tempfile::tempdir().unwrap();
        let moved = temp.path().join("moved");
        fs::write(
            temp.path().join("data-dir.json"),
            json!({"path":moved}).to_string(),
        )
        .unwrap();
        assert_eq!(resolve_pointer(temp.path()), moved);
        let p = json!({"api":"openai-responses","baseUrl":"https://example.com/v1","apiKey":"key","models":[{"id":"model"}],"activeModels":["model"]});
        validate_provider("p", &p).unwrap();
        let d = json!({"defaultProvider":"p","defaultModel":"model"});
        assert!(check_routes(&d, "p", None).is_err());
        check_routes(&d, "p", Some(&p)).unwrap();
        assert!(validate_provider(
            "p",
            &json!({"api":"openai-responses","models":[{"id":"m"},{"id":"m"}]})
        )
        .is_err());
    }

    #[test]
    #[serial_test::serial]
    fn liveagent_skill_selection_preserves_project_settings_and_rolls_back() {
        let root = test_support::TestRoot::new();
        let path = root.dir.path().join("skills-settings.json");
        let original = json!({"revision":4,"global":{"enabled":true,"selected":["existing"]},"projects":{"project":{"mode":"off","skillNames":["custom"]}}});
        fs::write(&path, original.to_string()).unwrap();
        update_skill_selection_and_commit("new", true, || Ok(())).unwrap();
        let doc: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(doc["revision"], 5);
        assert_eq!(doc["global"]["selected"], json!(["existing", "new"]));
        assert_eq!(doc["projects"], original["projects"]);
        let before = fs::read(&path).unwrap();
        let result: Result<(), AppError> = update_skill_selection_and_commit("new", false, || {
            Err(AppError::Database("failed".into()))
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        update_skill_selection_and_commit("new", false, || Ok(())).unwrap();
        let doc: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(doc["global"]["selected"], json!(["existing"]));
    }
}
