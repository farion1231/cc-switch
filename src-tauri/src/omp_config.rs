//! Oh My Pi (`omp`, can1357/oh-my-pi) owns its defaults and login. CC Switch manages only the
//! explicit `providers` entries in `models.yml` and the MCP servers in `mcp.json`, and reads
//! `config.yml` for the default model.
//!
//! The agent dir is `~/.omp/agent`, or `~/.omp/profiles/<name>/agent` for a named profile.
//! models.yml is edited as text: bytes outside the one entry CC Switch writes are never touched,
//! and a document it can't edit safely (anchors, tags, flow style) is refused, not rewritten.

use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use indexmap::IndexMap;
use serde_json::Value;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub(crate) const MAX_FILE_BYTES: u64 = 1024 * 1024;
static MODELS_LOCK: Mutex<()> = Mutex::new(());
static MCP_LOCK: Mutex<()> = Mutex::new(());
#[cfg(test)]
static TEST_AGENT_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

// ============================================================================
// Path resolution
// ============================================================================

pub(crate) fn get_omp_agent_dir() -> Result<PathBuf, AppError> {
    #[cfg(test)]
    if let Some(path) = TEST_AGENT_DIR
        .lock()
        .expect("lock Oh My Pi test directory")
        .clone()
    {
        return Ok(path);
    }

    if let Some(dir) = crate::settings::get_omp_override_dir() {
        return Ok(dir);
    }

    get_omp_native_agent_dir()
}

pub(crate) fn get_omp_active_profile() -> Result<Option<String>, AppError> {
    let selected = std::env::var_os("OMP_PROFILE").or_else(|| std::env::var_os("PI_PROFILE"));
    normalize_profile(
        selected
            .as_deref()
            .map(|name| name.to_string_lossy())
            .as_deref(),
    )
}

fn normalize_profile(raw: Option<&str>) -> Result<Option<String>, AppError> {
    let name = raw.unwrap_or_default().trim();
    if name.is_empty() || name == "default" {
        return Ok(None);
    }
    let reserved = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved_device = matches!(reserved.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (reserved.len() == 4
            && (reserved.starts_with("COM") || reserved.starts_with("LPT"))
            && reserved.as_bytes()[3].is_ascii_digit());
    if name.len() > 64
        || !name.as_bytes()[0].is_ascii_lowercase() && !name.as_bytes()[0].is_ascii_digit()
        || !name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-".contains(&c))
        || name.ends_with('.')
        || reserved_device
    {
        return Err(AppError::Config(format!(
            "Invalid Oh My Pi profile: {name}"
        )));
    }
    Ok(Some(name.to_string()))
}

fn native_agent_dir(
    home: &Path,
    config_dir: Option<&str>,
    profile: Option<&str>,
    inherited_profile: Option<&str>,
    agent_dir: Option<&Path>,
) -> Result<PathBuf, AppError> {
    // PI_CONFIG_DIR is a directory name relative to home, as in omp's path.join(home, name).
    let config_dir = config_dir.filter(|name| !name.is_empty()).unwrap_or(".omp");
    let root = home.join(config_dir.trim_start_matches(std::path::MAIN_SEPARATOR));
    if let Some(profile) = profile {
        return Ok(root.join("profiles").join(profile).join("agent"));
    }
    // A parent omp process may have exported its named profile directory.
    // Explicit default mode must ignore that derived override.
    let agent_dir = agent_dir.filter(|agent| {
        inherited_profile
            .is_none_or(|profile| *agent != root.join("profiles").join(profile).join("agent"))
    });
    if let Some(agent) = agent_dir.filter(|path| !path.as_os_str().is_empty()) {
        return if agent.is_absolute() {
            Ok(agent.to_path_buf())
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(agent))
                .map_err(|error| {
                    AppError::Config(format!("Cannot resolve Oh My Pi agent directory: {error}"))
                })
        };
    }
    Ok(root.join("agent"))
}

pub(crate) fn get_omp_native_agent_dir() -> Result<PathBuf, AppError> {
    let profile = get_omp_active_profile()?;
    let config_dir = std::env::var("PI_CONFIG_DIR").ok();
    let agent_dir = std::env::var_os("PI_CODING_AGENT_DIR").map(PathBuf::from);
    let inherited_profile = normalize_profile(std::env::var("PI_PROFILE").ok().as_deref())
        .ok()
        .flatten();
    native_agent_dir(
        &get_home_dir(),
        config_dir.as_deref(),
        profile.as_deref(),
        inherited_profile.as_deref(),
        agent_dir.as_deref(),
    )
}

pub(crate) fn get_omp_data_dir() -> Result<PathBuf, AppError> {
    let agent = get_omp_agent_dir()?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let profile = get_omp_active_profile()?;
        let config_dir = std::env::var("PI_CONFIG_DIR").ok();
        let default_agent = native_agent_dir(
            &get_home_dir(),
            config_dir.as_deref(),
            profile.as_deref(),
            None,
            None,
        )?;
        let xdg = std::env::var_os("XDG_DATA_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        Ok(data_dir(
            &agent,
            &default_agent,
            profile.as_deref(),
            xdg.as_deref(),
        ))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    Ok(agent)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn data_dir(
    agent: &Path,
    default_agent: &Path,
    profile: Option<&str>,
    xdg: Option<&Path>,
) -> PathBuf {
    if agent == default_agent {
        if let Some(xdg) = xdg {
            let mut root = xdg.join("omp");
            if let Some(profile) = profile {
                root = root.join("profiles").join(profile);
            }
            if root.exists() {
                return root;
            }
        }
    }
    agent.to_path_buf()
}

/// Existing `models.yml` → `models.yaml` wins; greenfield defaults to `models.yml`.
pub(crate) fn get_omp_models_path() -> Result<PathBuf, AppError> {
    let agent = get_omp_agent_dir()?;
    for name in ["models.yml", "models.yaml"] {
        let path = agent.join(name);
        if path.exists() {
            return Ok(path);
        }
    }
    Ok(agent.join("models.yml"))
}

/// Existing `config.yml` → `config.yaml` wins; greenfield defaults to `config.yml`.
pub(crate) fn get_omp_settings_path() -> Result<PathBuf, AppError> {
    let agent = get_omp_agent_dir()?;
    for name in ["config.yml", "config.yaml"] {
        let path = agent.join(name);
        if path.exists() {
            return Ok(path);
        }
    }
    Ok(agent.join("config.yml"))
}

/// User-level MCP config path (`~/.omp/agent/mcp.json`).
pub(crate) fn get_omp_mcp_path() -> Result<PathBuf, AppError> {
    Ok(get_omp_agent_dir()?.join("mcp.json"))
}

// ============================================================================
// Byte-level I/O
// ============================================================================

pub(crate) fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>, AppError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(AppError::io(path, e)),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| AppError::io(path, e))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(AppError::Config("omp.errors.fileTooLarge".into()));
    }
    Ok(Some(bytes))
}

/// Exact bytes are the revision: compare again immediately before the atomic private write.
/// Hold the feature's mutex through this call, including the database commit and rollback.
pub(crate) fn write_and_commit<T>(
    path: &Path,
    before: Option<&[u8]>,
    after: &[u8],
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    if after.len() as u64 > MAX_FILE_BYTES {
        return Err(AppError::Config("omp.errors.fileTooLarge".into()));
    }
    if read_bytes(path)?.as_deref() != before {
        return Err(AppError::Conflict("omp.errors.externalChange".into()));
    }
    atomic_write_private(path, after)?;
    match commit() {
        Ok(result) => Ok(result),
        Err(error) => {
            if read_bytes(path)?.as_deref() != Some(after) {
                return Err(AppError::Conflict("omp.errors.rollbackConflict".into()));
            }
            match before {
                Some(bytes) => atomic_write_private(path, bytes)?,
                None => fs::remove_file(path).map_err(|e| AppError::io(path, e))?,
            }
            Err(error)
        }
    }
}

fn read_text(path: &Path) -> Result<(Option<Vec<u8>>, String), AppError> {
    let bytes = read_bytes(path)?;
    let text = std::str::from_utf8(bytes.as_deref().unwrap_or_default())
        .map_err(|_| AppError::Config("omp.errors.invalidYaml".into()))?
        .to_string();
    Ok((bytes, text))
}

fn parse(text: &str) -> Result<serde_yaml::Value, AppError> {
    let mut document: serde_yaml::Value = serde_yaml::from_str(text)
        .map_err(|_| AppError::Config("omp.errors.invalidYaml".into()))?;
    if document.is_null() {
        document = serde_yaml::Value::Mapping(Default::default());
    }
    if !document.is_mapping() {
        return Err(AppError::Config("omp.errors.invalidYaml".into()));
    }
    Ok(document)
}

fn providers(document: &serde_yaml::Value) -> Result<IndexMap<String, Value>, AppError> {
    match document.get("providers") {
        None | Some(serde_yaml::Value::Null) => Ok(IndexMap::new()),
        Some(value) => serde_yaml::from_value(value.clone())
            .map_err(|_| AppError::Config("omp.errors.invalidYaml".into())),
    }
}

// ============================================================================
// Providers (models.yml)
// ============================================================================

pub(crate) fn get_providers() -> Result<IndexMap<String, Value>, AppError> {
    let _guard = MODELS_LOCK.lock()?;
    providers(&parse(&read_text(&get_omp_models_path()?)?.1)?)
}

/// `api` on a provider node, as omp v18.12's models.yml schema enumerates it.
/// omp rejects the whole file — every custom provider — when one node is invalid,
/// so an unknown value is refused here rather than written.
const APIS: &[&str] = &[
    "openai-completions",
    "openai-responses",
    "openai-codex-responses",
    "azure-openai-responses",
    "anthropic-messages",
    "bedrock-converse-stream",
    "google-generative-ai",
    "google-gemini-cli",
    "google-vertex",
    "openrouter-decisions",
    "openai-decisions",
    "workers-ai-systemone",
    "typesafe",
];

/// Runner APIs a single model may also name (omp's `RUNNER_API_KINDS`, minus web search).
const MODEL_ONLY_APIS: &[&str] = &[
    "openai-images",
    "openrouter-images",
    "xai-tts",
    "openai-speech",
    "openai-embeddings",
    "openrouter-rerank",
    "openrouter-video",
    "openai-transcriptions",
];

fn invalid_provider() -> AppError {
    AppError::Config("omp.errors.invalidProvider".into())
}

fn check_type(node: &Value, keys: &[&str], check: impl Fn(&Value) -> bool) -> Result<(), AppError> {
    for key in keys {
        if node.get(key).is_some_and(|v| !check(v)) {
            return Err(invalid_provider());
        }
    }
    Ok(())
}

fn string_map(v: &Value) -> bool {
    v.as_object()
        .is_some_and(|m| m.values().all(Value::is_string))
}

fn check_common(node: &Value, apis: &[&[&str]]) -> Result<(), AppError> {
    if !node.is_object() {
        return Err(invalid_provider());
    }
    check_type(node, &["baseUrl", "apiKey"], |v| {
        v.as_str().is_some_and(|s| !s.is_empty())
    })?;
    check_type(node, &["api"], |v| {
        v.as_str()
            .is_some_and(|s| apis.iter().any(|list| list.contains(&s)))
    })?;
    check_type(node, &["headers"], string_map)?;
    if let Some(compat) = node.get("compat") {
        check_compat(compat)?;
        if let Some(when_thinking) = compat.get("whenThinking") {
            check_compat(when_thinking)?;
        }
    }
    Ok(())
}

/// Enumerated `compat` fields, as omp v18.12's models.yml schema lists them. A Pi value
/// such as `thinkingFormat: deepseek` would make omp drop every custom provider.
const COMPAT_ENUMS: &[(&str, &[&str])] = &[
    ("maxTokensField", &["max_completion_tokens", "max_tokens"]),
    (
        "reasoningContentField",
        &["reasoning_content", "reasoning", "reasoning_text"],
    ),
    (
        "thinkingFormat",
        &["openai", "openrouter", "zai", "qwen", "qwen-chat-template"],
    ),
    ("cacheControlFormat", &["anthropic"]),
    ("toolStrictMode", &["all_strict", "none"]),
    (
        "streamMarkupHealingPattern",
        &["kimi", "dsml", "qwen", "thinking"],
    ),
    ("promptCacheMode", &["none", "automatic", "explicit"]),
];

fn check_compat(compat: &Value) -> Result<(), AppError> {
    if !compat.is_object() {
        return Err(invalid_provider());
    }
    check_type(
        compat,
        &[
            "supportsStore",
            "supportsDeveloperRole",
            "supportsReasoningEffort",
            "supportsUsageInStreaming",
            "requiresToolResultName",
            "requiresMistralToolIds",
            "requiresAssistantAfterToolResult",
            "requiresThinkingAsText",
            "supportsStrictMode",
        ],
        Value::is_boolean,
    )?;
    for (key, allowed) in COMPAT_ENUMS {
        check_type(compat, &[key], |v| {
            v.as_str().is_some_and(|s| allowed.contains(&s))
        })?;
    }
    if let Some(map) = compat.get("reasoningEffortMap") {
        if !map.is_object() {
            return Err(invalid_provider());
        }
        // omp declares these six levels and keeps any other key as is.
        check_type(
            map,
            &["minimal", "low", "medium", "high", "xhigh", "max"],
            Value::is_string,
        )?;
    }
    Ok(())
}

/// `discovery.type` values omp v18.12 accepts.
const DISCOVERY_TYPES: &[&str] = &[
    "ollama",
    "llama.cpp",
    "lm-studio",
    "openai-models-list",
    "proxy",
    "litellm",
    "apple-foundation-models",
];

/// Validate the one node CC Switch writes. Other providers in the file are never judged.
pub(crate) fn validate_provider(id: &str, node: &Value) -> Result<(), AppError> {
    if id.trim().is_empty() {
        return Err(invalid_provider());
    }
    check_common(node, &[APIS])?;
    check_type(node, &["auth"], |v| {
        matches!(v.as_str(), Some("apiKey" | "none" | "oauth"))
    })?;
    check_type(
        node,
        &["authHeader", "disableStrictTools"],
        Value::is_boolean,
    )?;
    let models = match node.get("models") {
        None => &[][..],
        Some(v) => v.as_array().ok_or_else(invalid_provider)?.as_slice(),
    };
    if models.is_empty() {
        // An override-only node must still override something (omp's own rule).
        let overrides = [
            "baseUrl",
            "headers",
            "apiKey",
            "compat",
            "disableStrictTools",
            "guardrailIdentifier",
            "requestMetadata",
            "remoteCompaction",
            "discovery",
        ];
        if !overrides.iter().any(|k| {
            node.get(k).is_some_and(|value| match value {
                Value::Bool(b) => *b,
                Value::String(s) => !s.is_empty(),
                _ => !value.is_null(),
            })
        }) && node.get("auth").and_then(Value::as_str) != Some("none")
            && node
                .get("modelOverrides")
                .and_then(Value::as_object)
                .is_none_or(|m| m.is_empty())
        {
            return Err(invalid_provider());
        }
    } else if node.get("baseUrl").is_none()
        || (node.get("apiKey").is_none()
            && !matches!(
                node.get("auth").and_then(Value::as_str),
                Some("none" | "oauth")
            ))
    {
        return Err(invalid_provider());
    }
    for model in models {
        check_common(model, &[APIS, MODEL_ONLY_APIS])?;
        if model
            .get("id")
            .and_then(Value::as_str)
            .is_none_or(|id| id.is_empty())
            || (model.get("api").is_none() && node.get("api").is_none())
        {
            return Err(invalid_provider());
        }
        check_type(model, &["input"], |v| {
            v.as_array().is_some_and(|a| {
                a.iter()
                    .all(|v| matches!(v.as_str(), Some("text" | "image")))
            })
        })?;
        check_type(model, &["contextWindow", "maxTokens"], |v| {
            v.as_f64().is_some_and(|n| n > 0.0)
        })?;
        if let Some(cost) = model.get("cost") {
            if !cost.is_object()
                || ["input", "output", "cacheRead", "cacheWrite"]
                    .iter()
                    .any(|k| cost.get(k).is_none_or(|v| !v.is_number()))
            {
                return Err(invalid_provider());
            }
        }
    }
    // Proxy discovery supplies its API at runtime, as in omp's native validator.
    if let Some(discovery) = node.get("discovery") {
        let kind = discovery.get("type").and_then(Value::as_str);
        if !kind.is_some_and(|kind| DISCOVERY_TYPES.contains(&kind))
            || (kind != Some("proxy") && node.get("api").is_none())
            || (discovery.get("injectV1").is_some() && kind != Some("openai-models-list"))
        {
            return Err(invalid_provider());
        }
        check_type(discovery, &["injectV1"], Value::is_boolean)?;
        check_type(discovery, &["timeoutMs"], |v| {
            v.as_f64().is_some_and(|n| n > 0.0 && n.is_finite())
        })?;
    }
    Ok(())
}

fn manual_edit() -> AppError {
    AppError::Config("omp.errors.manualEdit".into())
}

/// A block mapping only. Never reserialize the user's document. Blank lines and comments
/// at the end of a node belong to the surrounding text and survive replacement/removal.
fn edit_provider(text: &str, id: &str, replacement: Option<&Value>) -> Result<String, AppError> {
    if text.contains('\t') {
        return Err(manual_edit());
    }
    let document = parse(text).map_err(|_| manual_edit())?;
    let mut intended = providers(&document)?;
    if let Some(value) = replacement {
        validate_provider(id, value)?;
        intended.insert(id.into(), value.clone());
    } else {
        intended.shift_remove(id);
    }
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut offsets = Vec::new();
    let mut offset = 0;
    for line in &lines {
        offsets.push(offset);
        offset += line.len();
    }
    offsets.push(offset);
    let mut header = None;
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with(' ') || line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if line.trim() == "---" {
            continue;
        }
        if line.trim() == "..." {
            return Err(manual_edit());
        }
        if mapping_key(line)? == "providers" {
            header = Some(i);
        }
    }
    let mut output = text.to_string();
    let (start, end, indent) = if let Some(header) = header {
        let line = lines[header];
        let (_, tail) = line.split_once(':').ok_or_else(manual_edit)?;
        let tail = tail.split('#').next().unwrap_or_default().trim();
        if !matches!(tail, "" | "null" | "~") {
            return Err(manual_edit());
        }
        let end = ((header + 1)..lines.len())
            .find(|&i| {
                !lines[i].starts_with(' ')
                    && !lines[i].trim().is_empty()
                    && !lines[i].trim_start().starts_with('#')
            })
            .unwrap_or(lines.len());
        let mut indent = None;
        let mut children = Vec::new();
        for (i, line) in lines.iter().enumerate().take(end).skip(header + 1) {
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            reject_yaml_tokens(line)?;
            let spaces = line.len() - line.trim_start_matches(' ').len();
            let child_indent = *indent.get_or_insert(spaces);
            if spaces < child_indent {
                return Err(manual_edit());
            }
            if spaces == child_indent {
                if line.trim() == "{}" {
                    children.push((i, String::new()));
                    continue;
                }
                let key = mapping_key(line.trim_start())?;
                if key == "<<" {
                    return Err(manual_edit());
                }
                children.push((i, key));
            }
        }
        let indent = indent.unwrap_or(2);
        let mut start = end;
        while start > header + 1
            && (lines[start - 1].trim().is_empty()
                || lines[start - 1].trim_start().starts_with('#'))
        {
            start -= 1;
        }
        let mut stop = start;
        if let Some(index) = children
            .iter()
            .position(|(_, key)| key == id || key.is_empty())
        {
            start = children[index].0;
            stop = children.get(index + 1).map(|c| c.0).unwrap_or(end);
            while stop > start + 1
                && (lines[stop - 1].trim().is_empty()
                    || lines[stop - 1].trim_start().starts_with('#'))
            {
                stop -= 1;
            }
        }
        if !tail.is_empty() {
            // Normalize only the providers header when its value is null. Edit the
            // provider range first: the header rewrite changes its length.
            let colon = line.find(':').ok_or_else(manual_edit)?;
            let suffix = line
                .find('#')
                .map(|i| format!(" {}", &line[i..]))
                .unwrap_or_else(|| "\n".into());
            let mut original = text.to_string();
            let emitted = emit_provider(id, replacement, indent)?;
            original.replace_range(offsets[start]..offsets[stop], &emitted);
            original.replace_range(
                offsets[header]..offsets[header + 1],
                &format!("{}:{}", &line[..colon], suffix),
            );
            verify_edit(&original, &intended)?;
            return Ok(original);
        }
        (offsets[start], offsets[stop], indent)
    } else {
        if replacement.is_none() {
            return Ok(output);
        }
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str("providers:\n");
        (output.len(), output.len(), 2)
    };
    let mut emitted = if intended.is_empty() {
        format!("{}{{}}\n", " ".repeat(indent))
    } else {
        emit_provider(id, replacement, indent)?
    };
    if start == end && !output[..start].is_empty() && !output[..start].ends_with('\n') {
        emitted.insert(0, '\n');
    }
    output.replace_range(start..end, &emitted);
    verify_edit(&output, &intended)?;
    Ok(output)
}

fn emit_provider(id: &str, value: Option<&Value>, indent: usize) -> Result<String, AppError> {
    let Some(value) = value else {
        return Ok(String::new());
    };
    let yaml =
        serde_yaml::to_string(&IndexMap::from([(id, value)])).map_err(|_| invalid_provider())?;
    Ok(yaml
        .lines()
        .map(|line| format!("{}{line}\n", " ".repeat(indent)))
        .collect())
}

fn verify_edit(text: &str, intended: &IndexMap<String, Value>) -> Result<(), AppError> {
    if providers(&parse(text)?)? != *intended {
        return Err(manual_edit());
    }
    Ok(())
}

fn mapping_key(line: &str) -> Result<String, AppError> {
    // Find the mapping delimiter outside a quoted key (provider IDs are opaque).
    let mut quote = None;
    let mut escaped = false;
    let mut colon = None;
    for (index, c) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote == Some('"') && c == '\\' {
            escaped = true;
            continue;
        }
        if quote == Some(c) {
            quote = None;
            continue;
        }
        if quote.is_some() {
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            ':' => {
                colon = Some(index);
                break;
            }
            _ => {}
        }
    }
    let colon = colon.ok_or_else(manual_edit)?;
    let header = format!("{}: null", &line[..colon]);
    let mapping: serde_yaml::Mapping = serde_yaml::from_str(&header).map_err(|_| manual_edit())?;
    mapping
        .keys()
        .next()
        .and_then(serde_yaml::Value::as_str)
        .map(str::to_string)
        .ok_or_else(manual_edit)
}

/// Anchors, aliases, tags and merge keys can't be edited as text without breaking a
/// reference elsewhere; refuse them before writing.
fn reject_yaml_tokens(line: &str) -> Result<(), AppError> {
    let mut quote = None;
    let mut escaped = false;
    let mut boundary = true;
    let mut previous = [' '; 2];
    for c in line.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote == Some('"') && c == '\\' {
            escaped = true;
            continue;
        }
        if quote == Some(c) {
            quote = None;
            boundary = false;
            continue;
        }
        if quote.is_some() {
            continue;
        }
        if c == ':' && previous == ['<', '<'] {
            return Err(manual_edit());
        }
        match c {
            '"' | '\'' if boundary => quote = Some(c),
            '#' if boundary => break,
            '&' | '*' | '!' if boundary => return Err(manual_edit()),
            _ => {}
        }
        previous = [previous[1], c];
        boundary = c.is_whitespace() || matches!(c, ':' | '[' | '{' | ',');
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(crate) enum Change {
    Insert,
    ReplaceIfPresent,
    Remove,
}

/// Apply one provider change to models.yml and commit the database in the same critical
/// section. `commit` receives the provider's previous native value.
pub(crate) fn change_and_commit<T>(
    id: &str,
    config: Option<&Value>,
    change: Change,
    commit: impl FnOnce(Option<Value>) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = MODELS_LOCK.lock()?;
    let path = get_omp_models_path()?;
    let (before, text) = read_text(&path)?;
    let native = providers(&parse(&text).map_err(|_| manual_edit())?)?;
    let old = native.get(id).cloned();
    match change {
        Change::Insert if old.is_some() => {
            return Err(AppError::Config("omp.errors.duplicateProvider".into()))
        }
        Change::ReplaceIfPresent | Change::Remove if old.is_none() => return commit(None),
        _ => {}
    }
    // With neither YAML file present, omp migrates a legacy models.json itself. Creating
    // models.yml first would make omp skip that migration and drop the user's providers.
    if before.is_none() && path.with_file_name("models.json").exists() {
        return Err(AppError::Config("omp.errors.legacyModelsJson".into()));
    }
    let after = edit_provider(&text, id, config)?;
    write_and_commit(&path, before.as_deref(), after.as_bytes(), || commit(old))
}

pub(crate) fn provider_base_url(config: &Value) -> Result<String, AppError> {
    fn nonempty(value: Option<&Value>) -> Option<&str> {
        value.and_then(Value::as_str).filter(|s| !s.is_empty())
    }
    let provider = config.as_object().ok_or_else(invalid_provider)?;
    nonempty(provider.get("baseUrl"))
        .or_else(|| {
            provider
                .get("models")
                .and_then(Value::as_array)
                .and_then(|models| {
                    models
                        .iter()
                        .find_map(|model| nonempty(model.get("baseUrl")))
                })
        })
        .map(str::to_string)
        .ok_or_else(|| AppError::InvalidInput("Oh My Pi provider has no request URL".to_string()))
}

/// The key CC Switch's own usage queries should send for an omp `apiKey` value.
///
/// omp reads the value as an environment variable name first and falls back to the literal
/// string when that variable is unset or empty; unlike Pi there is no `$NAME` syntax. Besides
/// the process environment it reads `.env` in the agent dir, the config root and home, in that
/// order. Values starting with `!` are shell commands, which CC Switch never runs, so they give
/// an empty key instead of being sent as the credential.
pub(crate) fn resolve_usage_api_key(value: &str) -> String {
    let dotenv_files: Vec<PathBuf> = [get_omp_agent_dir(), get_omp_config_root()]
        .into_iter()
        .flatten()
        .map(|dir| dir.join(".env"))
        .chain([get_home_dir().join(".env")])
        .collect();
    resolve_usage_api_key_with(value, |name| {
        // omp matches names exactly; Windows' own lookup would ignore case.
        std::env::vars_os()
            .find(|(key, _)| key == name)
            .and_then(|(_, value)| value.into_string().ok())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                // Only shell identifiers are imported from dotenv files.
                let identifier = name.starts_with(|c: char| c == '_' || c.is_ascii_alphabetic())
                    && name.chars().all(|c| c == '_' || c.is_ascii_alphanumeric());
                identifier
                    .then(|| {
                        dotenv_files
                            .iter()
                            .find_map(|path| dotenv_value(path, name))
                    })
                    .flatten()
            })
    })
}

fn resolve_usage_api_key_with(value: &str, env: impl Fn(&str) -> Option<String>) -> String {
    if value.starts_with('!') {
        return String::new();
    }
    env(value).unwrap_or_else(|| value.to_string())
}

/// omp's config root, `~/<PI_CONFIG_DIR or .omp>` or a profile's directory under it, where it
/// reads a second `.env`. Agent-dir overrides do not move it.
fn get_omp_config_root() -> Result<PathBuf, AppError> {
    let config_dir = std::env::var("PI_CONFIG_DIR").ok();
    let profile = get_omp_active_profile()?;
    let agent = native_agent_dir(
        &get_home_dir(),
        config_dir.as_deref(),
        profile.as_deref(),
        None,
        None,
    )?;
    Ok(agent.parent().map_or(agent.clone(), Path::to_path_buf))
}

/// One variable from a dotenv file: `[export ]NAME=value`. The last assignment wins, and an empty
/// one leaves the variable unset for this file. Quotes are stripped and an unquoted value ends at
/// `#`. Multi-line values are not read.
fn dotenv_value(path: &Path, name: &str) -> Option<String> {
    let text = String::from_utf8(read_bytes(path).ok()??).ok()?;
    let raw = text.lines().rev().find_map(|line| {
        let line = line.trim();
        let (key, raw) = line
            .strip_prefix("export ")
            .unwrap_or(line)
            .split_once('=')?;
        (key.trim() == name).then(|| raw.trim())
    })?;
    let value = match raw.chars().next() {
        Some(quote @ ('"' | '\'' | '`')) => raw[1..].split(quote).next().unwrap_or_default(),
        _ => raw.split('#').next().unwrap_or_default().trim(),
    };
    (!value.is_empty()).then(|| value.to_string())
}

// ============================================================================
// Settings (config.yml), read only
// ============================================================================

/// The provider part of `modelRoles.default` (`<provider>/<model>`). CC Switch never writes it.
pub(crate) fn read_default_provider_id() -> Result<Option<String>, AppError> {
    let document = parse(&read_text(&get_omp_settings_path()?)?.1)?;
    Ok(document
        .get("modelRoles")
        .and_then(|roles| roles.get("default"))
        .and_then(serde_yaml::Value::as_str)
        .filter(|selector| !selector.is_empty())
        .map(|selector| {
            selector
                .split_once('/')
                .map_or(selector, |(provider, _)| provider)
                .to_string()
        }))
}

// ============================================================================
// MCP servers (mcp.json)
// ============================================================================

/// Mutate the native MCP document. Other root keys are kept; an external write between the
/// read and the write is refused.
pub(crate) fn update_omp_mcp_document(
    update: impl FnOnce(&mut Value) -> Result<bool, AppError>,
) -> Result<(), AppError> {
    let _guard = MCP_LOCK.lock()?;
    let path = get_omp_mcp_path()?;
    let before = read_bytes(&path)?;
    let mut document: Value = match &before {
        None => Value::Object(Default::default()),
        Some(bytes) => serde_json::from_slice(bytes)
            .map_err(|_| AppError::Config("omp.errors.invalidMcp".into()))?,
    };
    if !document.is_object() || document.get("mcpServers").is_some_and(|v| !v.is_object()) {
        return Err(AppError::Config("omp.errors.invalidMcp".into()));
    }
    if update(&mut document)? {
        let mut after = serde_json::to_vec_pretty(&document)
            .map_err(|e| AppError::JsonSerialize { source: e })?;
        after.push(b'\n');
        write_and_commit(&path, before.as_deref(), &after, || Ok(()))?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};

    pub(crate) struct TestAgentDir {
        _dir: Option<tempfile::TempDir>,
        previous: Option<PathBuf>,
    }

    impl TestAgentDir {
        pub(crate) fn new() -> Self {
            let dir = tempfile::tempdir().expect("create Oh My Pi test directory");
            let agent_dir = dir.path().join("agent");
            std::fs::create_dir_all(&agent_dir).expect("create Oh My Pi agent directory");
            Self::set(agent_dir, Some(dir))
        }

        #[allow(dead_code)] // used only by tests; compiled-in outside cfg(test) via shared test_support
        pub(crate) fn at(agent_dir: &Path) -> Self {
            Self::set(agent_dir.to_path_buf(), None)
        }

        fn set(agent_dir: PathBuf, dir: Option<tempfile::TempDir>) -> Self {
            let previous = super::TEST_AGENT_DIR
                .lock()
                .expect("lock Oh My Pi test directory")
                .replace(agent_dir);
            Self {
                _dir: dir,
                previous,
            }
        }
    }

    impl Drop for TestAgentDir {
        fn drop(&mut self) {
            *super::TEST_AGENT_DIR
                .lock()
                .expect("lock Oh My Pi test directory") = self.previous.take();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider() -> Value {
        json!({"baseUrl":"https://example.com/v1", "apiKey":"LITERAL_KEY", "api":"openai-completions", "models":[{"id":"model-a"}]})
    }

    #[test]
    fn native_profile_and_config_root_resolution() {
        let home = tempfile::tempdir().expect("home");
        assert_eq!(normalize_profile(Some("").or(Some("work"))).unwrap(), None);
        assert_eq!(
            normalize_profile(Some("default").or(Some("work"))).unwrap(),
            None
        );
        assert_eq!(
            normalize_profile(None.or(Some("work"))).unwrap().as_deref(),
            Some("work")
        );
        assert_eq!(
            native_agent_dir(home.path(), Some(".custom"), None, None, None).unwrap(),
            home.path().join(".custom/agent")
        );
        assert_eq!(
            native_agent_dir(
                home.path(),
                Some(".custom"),
                Some("work"),
                None,
                Some(Path::new("ignored"))
            )
            .unwrap(),
            home.path().join(".custom/profiles/work/agent")
        );
        let inherited = home.path().join(".custom/profiles/work/agent");
        assert_eq!(
            native_agent_dir(
                home.path(),
                Some(".custom"),
                None,
                Some("work"),
                Some(&inherited)
            )
            .unwrap(),
            home.path().join(".custom/agent")
        );
        let custom = home.path().join("custom-agent");
        assert_eq!(
            native_agent_dir(
                home.path(),
                Some(".custom"),
                None,
                Some("work"),
                Some(&custom)
            )
            .unwrap(),
            custom
        );
        for invalid in ["../other", "UPPER", "con", "nul.txt", "trailing."] {
            assert!(normalize_profile(Some(invalid)).is_err(), "{invalid}");
        }
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn xdg_data_root_follows_existing_profile_and_agent_override() {
        let home = tempfile::tempdir().expect("home");
        let xdg = tempfile::tempdir().expect("xdg");
        let default = home.path().join(".omp/agent");
        std::fs::create_dir_all(xdg.path().join("omp")).unwrap();
        assert_eq!(
            data_dir(&default, &default, None, Some(xdg.path())),
            xdg.path().join("omp")
        );
        let custom = home.path().join("custom-agent");
        assert_eq!(data_dir(&custom, &default, None, Some(xdg.path())), custom);
        let work = home.path().join(".omp/profiles/work/agent");
        assert_eq!(data_dir(&work, &work, Some("work"), Some(xdg.path())), work);
        std::fs::create_dir_all(xdg.path().join("omp/profiles/work")).unwrap();
        assert_eq!(
            data_dir(&work, &work, Some("work"), Some(xdg.path())),
            xdg.path().join("omp/profiles/work")
        );
    }

    #[test]
    fn surgical_edits_preserve_every_untouched_byte() {
        let before = "# before\nother: 'quoted' # keep\nproviders: # catalog\n    first:\n        baseUrl: https://first.example\n\n    # comment for target\n    target:\n        apiKey: old # replaced\n\n    # comment for last\n    last:\n        headers: {X-Test: 'verbatim'}\n\n# after\nother2: [1, 2]\n";
        let replacement = provider();
        let after = edit_provider(before, "target", Some(&replacement)).unwrap();
        let prefix = before.split("    target:").next().unwrap();
        let suffix = &before[before.find("\n    # comment for last").unwrap()..];
        assert!(after.starts_with(prefix));
        assert!(after.ends_with(suffix));
        assert_eq!(
            providers(&parse(&after).unwrap()).unwrap()["target"],
            replacement
        );
        let removed = edit_provider(&after, "target", None).unwrap();
        assert!(removed.starts_with(prefix) && removed.ends_with(suffix));
        let added = edit_provider(&removed, "new", Some(&provider())).unwrap();
        assert!(added.starts_with(prefix));
        assert!(added.ends_with("# after\nother2: [1, 2]\n"));
    }

    #[test]
    fn refuses_unsafe_yaml_without_reemitting_it() {
        for text in [
            "providers: {p: {apiKey: key}}\n",
            "providers:\n  p: &anchor\n    apiKey: key\n",
            "providers:\n  p:\n    headers: *anchor\n",
            "providers:\n  p: !custom {apiKey: key}\n",
            "providers:\n  <<: {p: {apiKey: key}}\n",
            "providers:\n  p:\n    apiKey: key\n    <<: {headers: {}}\n",
            "providers:\n  p: {apiKey: a}\n  p: {apiKey: b}\n",
            "providers:\n  p:\n    apiKey: a\n    apiKey: b\n",
            "providers:\n\tp: {}\n",
            "[]\n",
            "providers: 42\n",
            "---\nproviders:\n...\n",
        ] {
            assert!(
                edit_provider(text, "p", Some(&provider())).is_err(),
                "{text}"
            );
        }
    }

    #[test]
    fn handles_empty_null_and_last_removal_without_invalid_native_null() {
        for text in [
            "",
            "# keep\n",
            "providers:\n",
            "providers: null # keep\n",
            "providers: ~\n",
        ] {
            let added = edit_provider(text, "p", Some(&provider())).unwrap();
            assert_eq!(providers(&parse(&added).unwrap()).unwrap().len(), 1);
            let removed = edit_provider(&added, "p", None).unwrap();
            assert!(
                parse(&removed).unwrap()["providers"].is_mapping(),
                "{removed}"
            );
            assert!(edit_provider(&removed, "p", Some(&provider())).is_ok());
        }
    }

    #[test]
    fn validates_only_the_replacement_node() {
        for node in [
            json!({}),
            json!({"baseUrl":1}),
            json!({"apiKey":""}),
            json!({"apiKey":"k","api":"not-an-api"}),
            json!({"baseUrl":"url","models":[{"id":"x"}]}),
            json!({"apiKey":"k","models":{}}),
            json!({"baseUrl":"url", "apiKey":"k", "models":[{"id":"x"}]}),
            json!({"apiKey":"k", "compat": []}),
            json!({"apiKey":"k", "compat": {"supportsStore":"yes"}}),
            json!({"headers":{"X":1}}),
        ] {
            assert!(validate_provider("p", &node).is_err(), "{node}");
        }
        for (key, value) in [
            ("headers", json!({"X": 1})),
            ("input", json!(["audio"])),
            ("contextWindow", json!("large")),
            ("maxTokens", json!(0)),
            ("cost", json!({"input": 1})),
            (
                "cost",
                json!({"input": "free", "output": 0, "cacheRead": 0, "cacheWrite": 0}),
            ),
            ("api", json!("future-api")),
            ("compat", json!({"supportsStore": "yes"})),
            ("id", json!("")),
        ] {
            let mut invalid = provider();
            invalid["models"][0][key] = value;
            assert!(validate_provider("p", &invalid).is_err(), "{key}");
        }
        assert!(validate_provider(
            "p",
            &json!({"baseUrl":"url", "discovery":{"type":"future"}})
        )
        .is_err());
        assert!(validate_provider("p", &provider()).is_ok());
        assert!(validate_provider("builtin", &json!({"baseUrl":"https://example.com"})).is_ok());
        let mut no_auth = provider();
        no_auth.as_object_mut().unwrap().remove("apiKey");
        no_auth["auth"] = json!("none");
        assert!(validate_provider("p", &no_auth).is_ok());
        no_auth["auth"] = json!("oauth");
        assert!(validate_provider("p", &no_auth).is_ok());
        let mut per_model_api = provider();
        let api = per_model_api
            .as_object_mut()
            .unwrap()
            .remove("api")
            .unwrap();
        per_model_api["models"][0]["api"] = api;
        assert!(validate_provider("p", &per_model_api).is_ok());
        let before = "providers:\n  bad: {models: [{id: x}]}\n  future: {api: future-api, kind: future-kind, discovery: {type: future}, thinking: {mode: future}}\n";
        let edited = edit_provider(before, "p", Some(&provider())).unwrap();
        assert!(edited.starts_with(before));
        let mut replacement = provider();
        replacement["apiKey"] = json!("next-key");
        assert!(edit_provider(&edited, "p", Some(&replacement))
            .unwrap()
            .starts_with(before));
        assert!(edit_provider(before, "bad", None).is_ok());
    }

    #[test]
    fn revisions_size_limit_private_write_and_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.yml");
        fs::write(&path, "external").unwrap();
        assert!(write_and_commit(&path, Some(b"expected"), b"ours", || Ok(())).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"external");
        let result: Result<(), AppError> =
            write_and_commit(&path, Some(b"external"), b"ours", || {
                Err(AppError::Database("read only".into()))
            });
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"external");
        fs::remove_file(&path).unwrap();
        let result: Result<(), AppError> = write_and_commit(&path, None, b"ours", || {
            Err(AppError::Database("read only".into()))
        });
        assert!(result.is_err() && !path.exists());
        write_and_commit(&path, None, b"ours", || Ok(())).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::write(&path, vec![b' '; MAX_FILE_BYTES as usize + 1]).unwrap();
        assert!(read_bytes(&path).is_err());
    }

    #[test]
    fn rejects_enum_and_discovery_values_omp_would_refuse() {
        // Each of these makes omp disable every custom provider in models.yml.
        for compat in [
            json!({"thinkingFormat": "deepseek"}),
            json!({"maxTokensField": "max_output_tokens"}),
            json!({"reasoningContentField": 1}),
            json!({"whenThinking": {"thinkingFormat": "deepseek"}}),
            json!({"reasoningEffortMap": {"high": 1}}),
        ] {
            let mut invalid = provider();
            invalid["models"][0]["compat"] = compat.clone();
            assert!(validate_provider("p", &invalid).is_err(), "{compat}");
        }
        for (key, value) in [
            ("auth", json!("bearer")),
            ("authHeader", json!("yes")),
            ("discovery", json!({"type": "ollama", "injectV1": false})),
            (
                "discovery",
                json!({"type": "openai-models-list", "injectV1": "no"}),
            ),
            ("discovery", json!({"type": "ollama", "timeoutMs": 0})),
            ("discovery", json!({})),
        ] {
            let mut invalid = provider();
            invalid[key] = value.clone();
            assert!(validate_provider("p", &invalid).is_err(), "{key}: {value}");
        }
        let mut valid = provider();
        valid["compat"] =
            json!({"thinkingFormat": "qwen", "whenThinking": {"maxTokensField": "max_tokens"}});
        valid["discovery"] =
            json!({"type": "openai-models-list", "injectV1": false, "timeoutMs": 500});
        valid["auth"] = json!("apiKey");
        valid["models"][0]["compat"] =
            json!({"reasoningEffortMap": {"high": "high", "none": null}});
        assert!(validate_provider("p", &valid).is_ok());
    }

    #[test]
    #[serial_test::serial]
    fn leaves_a_legacy_models_json_for_omp_to_migrate() {
        let _agent = test_support::TestAgentDir::new();
        let dir = get_omp_agent_dir().unwrap();
        fs::write(dir.join("models.json"), r#"{"providers":{}}"#).unwrap();
        let result = change_and_commit("p", Some(&provider()), Change::Insert, |_| Ok(()));
        assert!(
            matches!(result, Err(AppError::Config(key)) if key == "omp.errors.legacyModelsJson")
        );
        assert!(!dir.join("models.yml").exists());
        // Removing a provider that is not there writes nothing, so it still goes through.
        change_and_commit("p", None, Change::Remove, |_| Ok(())).unwrap();
        // Once omp has migrated, edits work as usual.
        fs::write(dir.join("models.yml"), "# migrated by omp\n").unwrap();
        change_and_commit("p", Some(&provider()), Change::Insert, |_| Ok(())).unwrap();
    }

    #[test]
    fn usage_api_key_follows_omp_resolution() {
        let env = |name: &str| (name == "DS_KEY").then(|| "sk-env".to_string());
        let resolve = |value: &str| resolve_usage_api_key_with(value, env);
        assert_eq!(resolve("DS_KEY"), "sk-env");
        // Unset names fall back to the literal string, as in omp.
        assert_eq!(resolve("UNSET_NAME"), "UNSET_NAME");
        assert_eq!(resolve("sk-literal"), "sk-literal");
        // omp has no `$NAME` syntax; the value is a literal.
        assert_eq!(resolve("$DS_KEY"), "$DS_KEY");
        assert_eq!(resolve("!op read op://dev/key"), "");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        fs::write(
            &path,
            "# DS_KEY=commented\nexport DS_KEY=\"sk-first\"\nOTHER=x\nDS_KEY=sk-last # note\nQUOTED='a # b'\nEMPTY=\nINLINE=sk-good#comment\nCLEARED=sk-old\nCLEARED=\n",
        )
        .unwrap();
        assert_eq!(dotenv_value(&path, "DS_KEY").as_deref(), Some("sk-last"));
        assert_eq!(dotenv_value(&path, "QUOTED").as_deref(), Some("a # b"));
        assert_eq!(dotenv_value(&path, "EMPTY"), None);
        assert_eq!(dotenv_value(&path, "INLINE").as_deref(), Some("sk-good"));
        // A later empty assignment clears an earlier key in the same file.
        assert_eq!(dotenv_value(&path, "CLEARED"), None);
        assert_eq!(dotenv_value(&path, "MISSING"), None);
        assert_eq!(dotenv_value(&dir.path().join("none"), "DS_KEY"), None);
    }

    #[test]
    #[serial_test::serial]
    fn yaml_extension_follows_native_precedence() {
        let _agent = test_support::TestAgentDir::new();
        let dir = get_omp_agent_dir().unwrap();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("models.yaml"), "providers: {}\n").unwrap();
        assert_eq!(get_omp_models_path().unwrap(), dir.join("models.yaml"));
        fs::write(dir.join("models.yml"), "providers: {}\n").unwrap();
        assert_eq!(get_omp_models_path().unwrap(), dir.join("models.yml"));
    }
}
