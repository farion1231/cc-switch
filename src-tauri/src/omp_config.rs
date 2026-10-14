//! Oh My Pi owns defaults and login. Only explicit models.yml providers are managed here.

use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use indexmap::IndexMap;
use serde_json::Value;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
};

static MODELS_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
static TEST_AGENT_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

pub(crate) const MAX_FILE_BYTES: u64 = 1024 * 1024;

pub(crate) fn agent_dir() -> Result<PathBuf, AppError> {
    #[cfg(test)]
    if let Some(path) = TEST_AGENT_DIR.lock()?.clone() {
        return Ok(path);
    }
    resolve_agent_dir(
        crate::settings::get_omp_override_dir(),
        std::env::var_os("PI_CODING_AGENT_DIR"),
        std::env::var_os("PI_CONFIG_DIR"),
        get_home_dir(),
    )
}

fn resolve_agent_dir(
    settings: Option<PathBuf>,
    env: Option<std::ffi::OsString>,
    config_root: Option<std::ffi::OsString>,
    home: PathBuf,
) -> Result<PathBuf, AppError> {
    let path = settings
        .or_else(|| {
            env.filter(|v| !v.is_empty())
                .map(|v| crate::settings::resolve_override_path(&v.to_string_lossy()))
        })
        .unwrap_or_else(|| {
            home.join(
                config_root
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| ".omp".into()),
            )
            .join("agent")
        });
    if !path.is_absolute() {
        return Err(AppError::Config("omp.errors.directory".into()));
    }
    Ok(path)
}

pub(crate) fn models_path() -> Result<PathBuf, AppError> {
    let dir = agent_dir()?;
    let yml = dir.join("models.yml");
    Ok(if !yml.exists() && dir.join("models.yaml").exists() {
        dir.join("models.yaml")
    } else {
        yml
    })
}

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

pub(crate) fn get_providers() -> Result<IndexMap<String, Value>, AppError> {
    let _guard = MODELS_LOCK.lock()?;
    let bytes = read_bytes(&models_path()?)?.unwrap_or_default();
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| AppError::Config("omp.errors.invalidYaml".into()))?;
    providers(&parse(text)?)
}

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

fn check_common(node: &Value) -> Result<(), AppError> {
    if !node.is_object() {
        return Err(invalid_provider());
    }
    check_type(node, &["baseUrl", "apiKey"], |v| {
        v.as_str().is_some_and(|s| !s.is_empty())
    })?;
    check_type(node, &["api"], |v| {
        v.as_str().is_some_and(|s| APIS.contains(&s))
    })?;
    check_type(node, &["headers"], string_map)?;
    if let Some(compat) = node.get("compat") {
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
    }
    Ok(())
}

pub(crate) fn validate_provider(id: &str, node: &Value) -> Result<(), AppError> {
    if id.trim().is_empty() {
        return Err(invalid_provider());
    }
    check_common(node)?;
    let models = match node.get("models") {
        None => &[][..],
        Some(v) => v.as_array().ok_or_else(invalid_provider)?.as_slice(),
    };
    if models.is_empty() {
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
        check_common(model)?;
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
        if discovery.get("type").and_then(Value::as_str) != Some("proxy")
            && node.get("api").is_none()
        {
            return Err(invalid_provider());
        }
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
            // Normalize only the providers header when its value is null.
            let colon = line.find(':').ok_or_else(manual_edit)?;
            let suffix = line
                .find('#')
                .map(|i| format!(" {}", &line[i..]))
                .unwrap_or_else(|| "\n".into());
            // Header changed length; edit the original ranges first below instead.
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

pub(crate) fn change_and_commit<T>(
    id: &str,
    config: Option<&Value>,
    change: Change,
    commit: impl FnOnce(Option<Value>) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = MODELS_LOCK.lock()?;
    let path = models_path()?;
    let before = read_bytes(&path)?;
    let text = std::str::from_utf8(before.as_deref().unwrap_or_default())
        .map_err(|_| AppError::Config("omp.errors.invalidYaml".into()))?;
    let native = providers(&parse(text).map_err(|_| manual_edit())?)?;
    let old = native.get(id).cloned();
    match change {
        Change::Insert if old.is_some() => {
            return Err(AppError::Config("omp.errors.duplicateProvider".into()))
        }
        Change::ReplaceIfPresent if old.is_none() => return commit(None),
        Change::Remove if old.is_none() => return commit(None),
        _ => {}
    }
    let after = edit_provider(text, id, config)?;
    write_and_commit(&path, before.as_deref(), after.as_bytes(), || commit(old))
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    pub(crate) struct TestAgentDir {
        _dir: tempfile::TempDir,
        previous: Option<PathBuf>,
    }
    impl TestAgentDir {
        pub(crate) fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let previous = TEST_AGENT_DIR
                .lock()
                .unwrap()
                .replace(dir.path().to_path_buf());
            Self {
                _dir: dir,
                previous,
            }
        }
    }
    impl Drop for TestAgentDir {
        fn drop(&mut self) {
            *TEST_AGENT_DIR.lock().unwrap() = self.previous.take();
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
    fn directory_resolution_prefers_settings_then_env_then_config_root() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_path_buf();
        let settings = home.join("settings");
        let env = home.join("env");
        assert_eq!(
            resolve_agent_dir(
                Some(settings.clone()),
                Some(env.clone().into_os_string()),
                None,
                home.clone()
            )
            .unwrap(),
            settings
        );
        assert_eq!(
            resolve_agent_dir(None, Some(env.clone().into_os_string()), None, home.clone())
                .unwrap(),
            env
        );
        assert_eq!(
            resolve_agent_dir(None, None, Some(".custom-omp".into()), home.clone()).unwrap(),
            home.join(".custom-omp/agent")
        );
        assert_eq!(
            resolve_agent_dir(None, Some("".into()), None, home.clone()).unwrap(),
            home.join(".omp/agent")
        );
        assert!(resolve_agent_dir(None, Some("relative/agent".into()), None, home).is_err());
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
    #[serial_test::serial]
    fn yaml_extension_follows_native_precedence() {
        let _agent = test_support::TestAgentDir::new();
        let dir = agent_dir().unwrap();
        fs::write(dir.join("models.yaml"), "providers: {}\n").unwrap();
        assert_eq!(models_path().unwrap(), dir.join("models.yaml"));
        fs::write(dir.join("models.yml"), "providers: {}\n").unwrap();
        assert_eq!(models_path().unwrap(), dir.join("models.yml"));
    }
}
