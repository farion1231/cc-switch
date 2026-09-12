//! OMP native `models.yml` adapter.
use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use indexmap::IndexMap;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

const MAX_FILE_BYTES: u64 = 1024 * 1024;
static FILE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

pub(crate) fn get_omp_agent_dir() -> Result<PathBuf, AppError> {
    let path = std::env::var_os("PI_CODING_AGENT_DIR")
        .filter(|v| !v.is_empty())
        .map(|v| crate::settings::resolve_override_path(&v.to_string_lossy()))
        .unwrap_or_else(|| get_home_dir().join(".omp").join("agent"));
    if !path.is_absolute() {
        return Err(AppError::InvalidInput(format!(
            "OMP agent directory must be absolute: {}",
            path.display()
        )));
    }
    Ok(path)
}

pub(crate) fn get_omp_models_path() -> Result<PathBuf, AppError> {
    Ok(get_omp_agent_dir()?.join("models.yml"))
}

pub(crate) fn read_omp_native_providers() -> Result<IndexMap<String, Value>, AppError> {
    let _guard = FILE_LOCK
        .lock()
        .map_err(|e| AppError::Config(e.to_string()))?;
    let document = read_document(&get_omp_models_path()?)?;
    let providers = providers(&document)?;
    Ok(providers
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect())
}

pub(crate) fn read_omp_native_provider(key: &str) -> Result<Option<Value>, AppError> {
    Ok(read_omp_native_providers()?.shift_remove(key))
}
pub(crate) fn omp_provider_exists(key: &str) -> Result<bool, AppError> {
    Ok(read_omp_native_provider(key)?.is_some())
}

pub(crate) fn insert_omp_provider(key: &str, config: &Value) -> Result<bool, AppError> {
    validate(key, config)?;
    let _g = FILE_LOCK
        .lock()
        .map_err(|e| AppError::Config(e.to_string()))?;
    let path = get_omp_models_path()?;
    let (mut doc, rev) = read_with_revision(&path)?;
    let p = providers_mut(&mut doc)?;
    if p.contains_key(key) {
        return Err(AppError::InvalidInput(format!(
            "OMP provider key '{key}' already exists in models.yml"
        )));
    }
    p.insert(key.into(), config.clone());
    write_document(&path, &doc, &rev)?;
    Ok(true)
}

pub(crate) fn replace_omp_provider_if_present(
    key: &str,
    replacement: &Value,
) -> Result<Option<Value>, AppError> {
    validate(key, replacement)?;
    let _g = FILE_LOCK
        .lock()
        .map_err(|e| AppError::Config(e.to_string()))?;
    let path = get_omp_models_path()?;
    let (mut doc, rev) = read_with_revision(&path)?;
    let p = providers_mut(&mut doc)?;
    let old = p.get(key).cloned();
    // A missing key means the provider is disabled in models.yml; an edit must
    // not re-insert it (that would re-enable it and leak on DB failure).
    if old.is_some() && old.as_ref() != Some(replacement) {
        p.insert(key.into(), replacement.clone());
        write_document(&path, &doc, &rev)?;
    }
    Ok(old)
}
pub(crate) fn replace_omp_provider(
    key: &str,
    expected: &Value,
    replacement: &Value,
) -> Result<(), AppError> {
    validate(key, replacement)?;
    let _g = FILE_LOCK
        .lock()
        .map_err(|e| AppError::Config(e.to_string()))?;
    let path = get_omp_models_path()?;
    let (mut doc, rev) = read_with_revision(&path)?;
    let p = providers_mut(&mut doc)?;
    let old = p
        .get(key)
        .ok_or_else(|| AppError::Conflict(format!("OMP provider '{key}' is missing")))?;
    if old != expected {
        return Err(AppError::Conflict(format!(
            "OMP provider '{key}' changed outside CC Switch"
        )));
    }
    p.insert(key.into(), replacement.clone());
    write_document(&path, &doc, &rev)
}
pub(crate) fn remove_omp_provider(key: &str) -> Result<Option<Value>, AppError> {
    remove_inner(key, None)
}
pub(crate) fn remove_omp_provider_if_matches(
    key: &str,
    expected: &Value,
) -> Result<bool, AppError> {
    Ok(remove_inner(key, Some(expected))?.is_some())
}
pub(crate) fn restore_omp_provider_if_missing(key: &str, config: &Value) -> Result<(), AppError> {
    if omp_provider_exists(key)? {
        return Ok(());
    }
    insert_omp_provider(key, config).map(|_| ())
}
fn remove_inner(key: &str, expected: Option<&Value>) -> Result<Option<Value>, AppError> {
    let _g = FILE_LOCK
        .lock()
        .map_err(|e| AppError::Config(e.to_string()))?;
    let path = get_omp_models_path()?;
    let (mut doc, rev) = read_with_revision(&path)?;
    let p = providers_mut(&mut doc)?;
    let old = p.get(key).cloned();
    if let (Some(a), Some(b)) = (old.as_ref(), expected) {
        if a != b {
            return Err(AppError::Conflict(
                "OMP provider changed outside CC Switch".into(),
            ));
        }
    }
    if old.is_some() {
        p.remove(key);
        write_document(&path, &doc, &rev)?;
    }
    Ok(old)
}
fn validate(key: &str, v: &Value) -> Result<(), AppError> {
    if key.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "OMP provider key cannot be empty".into(),
        ));
    }
    if !v.is_object() {
        return Err(AppError::InvalidInput(
            "OMP provider configuration must be an object".into(),
        ));
    }
    Ok(())
}
fn read_document(path: &Path) -> Result<Value, AppError> {
    Ok(read_with_revision(path)?.0)
}
fn read_with_revision(path: &Path) -> Result<(Value, String), AppError> {
    if !path.exists() {
        return Ok((Value::Object(Map::new()), "missing".into()));
    }
    let bytes = fs::read(path).map_err(|e| AppError::io(path, e))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(AppError::InvalidInput(
            "OMP models.yml exceeds 1 MiB".into(),
        ));
    }
    let rev = revision(&bytes);
    let text = String::from_utf8(bytes).map_err(|e| AppError::Config(e.to_string()))?;
    let y: serde_yaml::Value = serde_yaml::from_str(&text)
        .map_err(|e| AppError::Config(format!("invalid OMP YAML: {e}")))?;
    Ok((
        serde_json::to_value(y).map_err(|e| AppError::Config(e.to_string()))?,
        rev,
    ))
}
fn providers(v: &Value) -> Result<&Map<String, Value>, AppError> {
    static EMPTY: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
    match v.get("providers") {
        None => Ok(EMPTY.get_or_init(Map::new)),
        Some(value) => value
            .as_object()
            .ok_or_else(|| AppError::Config("OMP models.yml providers must be an object".into())),
    }
}
fn providers_mut(v: &mut Value) -> Result<&mut Map<String, Value>, AppError> {
    let r = v
        .as_object_mut()
        .ok_or_else(|| AppError::Config("OMP models.yml root must be an object".into()))?;
    let entry = r
        .entry("providers")
        .or_insert_with(|| Value::Object(Map::new()));
    entry
        .as_object_mut()
        .ok_or_else(|| AppError::Config("OMP models.yml providers must be an object".into()))
}
fn write_document(path: &Path, v: &Value, expected: &str) -> Result<(), AppError> {
    let actual = if path.exists() {
        revision(&fs::read(path).map_err(|e| AppError::io(path, e))?)
    } else {
        "missing".into()
    };
    if actual != expected {
        return Err(AppError::Conflict(format!(
            "OMP models.yml changed: {}",
            path.display()
        )));
    }
    let y: serde_yaml::Value =
        serde_json::from_value(v.clone()).map_err(|e| AppError::Config(e.to_string()))?;
    let bytes = serde_yaml::to_string(&y).map_err(|e| AppError::Config(e.to_string()))?;
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).map_err(|e| AppError::io(p, e))?;
    }
    atomic_write_private(path, bytes.as_bytes())
}
fn revision(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use serial_test::serial;

    /// Points `PI_CODING_AGENT_DIR` at an isolated agent directory for the
    /// duration of a test (same guard pattern as `pi_config::test_support`).
    struct AgentDirGuard {
        _dir: tempfile::TempDir,
        previous: Option<std::ffi::OsString>,
    }

    impl AgentDirGuard {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("temp OMP agent dir");
            let previous = std::env::var_os("PI_CODING_AGENT_DIR");
            std::env::set_var("PI_CODING_AGENT_DIR", dir.path().join("agent"));
            Self {
                _dir: dir,
                previous,
            }
        }
    }

    impl Drop for AgentDirGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var("PI_CODING_AGENT_DIR", value),
                None => std::env::remove_var("PI_CODING_AGENT_DIR"),
            }
        }
    }

    fn write_models(contents: &str) -> PathBuf {
        let path = get_omp_models_path().expect("models path");
        fs::create_dir_all(path.parent().expect("agent dir")).expect("create agent dir");
        fs::write(&path, contents).expect("write models.yml");
        path
    }

    fn replacement() -> Value {
        json!({ "baseUrl": "https://api.example.com", "apiKey": "sk-test" })
    }

    #[test]
    #[serial]
    fn editing_an_absent_provider_preserves_disabled_state() {
        let _agent = AgentDirGuard::new();
        let path = write_models("providers:\n  kept:\n    baseUrl: https://kept.example\n");

        // A provider missing from models.yml is disabled; editing it must not
        // re-insert (enable) it.
        let old = replace_omp_provider_if_present("disabled-one", &replacement())
            .expect("replace absent provider");
        assert!(
            old.is_none(),
            "absent provider edit must report no previous value"
        );
        let doc = read_document(&path).expect("re-read models.yml");
        assert!(
            !providers(&doc)
                .expect("providers")
                .contains_key("disabled-one"),
            "absent provider must stay out of models.yml"
        );

        // Present providers are still replaced in place.
        let kept = providers(&read_document(&path).expect("read"))
            .expect("providers")
            .get("kept")
            .cloned()
            .expect("kept provider");
        let old = replace_omp_provider_if_present("kept", &replacement())
            .expect("replace present provider");
        assert_eq!(old, Some(kept));
        assert_eq!(
            read_omp_native_provider("kept")
                .expect("read kept")
                .as_ref(),
            Some(&replacement())
        );
    }

    #[test]
    #[serial]
    fn non_object_providers_errors_without_poisoning_the_lock() {
        let _agent = AgentDirGuard::new();
        write_models("providers:\n  - id: array-entry\n");

        // Mutation must fail cleanly instead of panicking under FILE_LOCK.
        let err = insert_omp_provider("new-one", &replacement())
            .expect_err("array providers must be rejected");
        assert!(err.to_string().contains("providers"), "got: {err}");
        // Existence check still runs (lock not poisoned) and fails cleanly on
        // the malformed document rather than panicking again.
        assert!(omp_provider_exists("new-one").is_err());

        // FILE_LOCK must not be poisoned: a following operation still runs
        // (and fails with a real config error, not a mutex poison error).
        let err = replace_omp_provider_if_present("any", &replacement())
            .expect_err("still non-object providers");
        assert!(err.to_string().contains("providers"), "got: {err}");
    }
}
