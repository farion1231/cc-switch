#[cfg(windows)]
use crate::config::derive_wsl_home_dir;
use crate::config::{atomic_write, get_home_dir, write_json_file_with_contents};
use crate::error::AppError;
use crate::jsonc_document::JsoncDocument;
use crate::opencode_config::get_opencode_dir;
use crate::provider::Provider;
use crate::store::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OmoLocalFileData {
    pub agents: Option<Value>,
    pub categories: Option<Value>,
    pub other_fields: Option<Value>,
    pub file_path: String,
    pub last_modified: Option<String>,
}

type OmoProfileData = (Option<Value>, Option<Value>, Option<Value>);
type OptionalConfigFileVersions = Option<(Vec<u8>, Vec<u8>)>;

const UNIFIED_CONFIG_FILENAMES: [&str; 2] = ["omo.jsonc", "omo.json"];
const OPENCODE_SECTION_KEY: &str = "[opencode]";

#[derive(Debug, PartialEq)]
enum OmoConfigLocation {
    Unified(PathBuf),
    Legacy(PathBuf),
}

impl OmoConfigLocation {
    fn path(&self) -> &Path {
        match self {
            Self::Unified(path) | Self::Legacy(path) => path,
        }
    }
}

struct UnifiedConfigDocument {
    path: PathBuf,
    document: JsoncDocument,
}

impl UnifiedConfigDocument {
    fn load(path: &Path) -> Result<Self, AppError> {
        let source = std::fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
        let document = JsoncDocument::parse(&source)?;
        if document.root_key_count(OPENCODE_SECTION_KEY)? > 1 {
            return Err(AppError::Config(
                "OMO config contains duplicate [opencode] sections".to_string(),
            ));
        }
        Ok(Self {
            path: path.to_path_buf(),
            document,
        })
    }

    fn set_opencode_section(&mut self, value: &Value) -> Result<bool, AppError> {
        if !value.is_object()
            || self
                .document
                .value()
                .get(OPENCODE_SECTION_KEY)
                .is_some_and(|v| !v.is_object())
        {
            return Err(AppError::Config(format!(
                "OMO [opencode] section must be an object: {}",
                self.path.display()
            )));
        }
        let mut desired = self.document.value().clone();
        desired[OPENCODE_SECTION_KEY] = value.clone();
        self.document.apply(&desired)
    }

    fn remove_opencode_section(&mut self) -> Result<bool, AppError> {
        let mut desired = self.document.value().clone();
        desired
            .as_object_mut()
            .expect("validated object root")
            .remove(OPENCODE_SECTION_KEY);
        self.document.apply(&desired)
    }

    fn save(self) -> Result<Vec<u8>, AppError> {
        let _guard = omo_write_lock().lock()?;
        let next_source = self.document.validated_source()?;
        let current_source =
            std::fs::read_to_string(&self.path).map_err(|e| AppError::io(&self.path, e))?;
        if current_source != self.document.original_source() {
            return Err(AppError::Config(
                "OMO config changed on disk. Please reload and try again.".to_string(),
            ));
        }
        if next_source != current_source {
            atomic_write(&self.path, next_source.as_bytes())?;
        }
        Ok(next_source.into_bytes())
    }
}

fn omo_write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn omo_operation_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

// ── Variant descriptor ─────────────────────────────────────────

pub struct OmoVariant {
    pub preferred_filename: &'static str,
    pub config_candidates: &'static [&'static str],
    pub category: &'static str,
    pub provider_prefix: &'static str,
    pub plugin_name: &'static str,
    pub plugin_prefixes: &'static [&'static str],
    pub has_categories: bool,
    pub label: &'static str,
    pub import_label: &'static str,
}

pub const STANDARD: OmoVariant = OmoVariant {
    preferred_filename: "oh-my-openagent.jsonc",
    config_candidates: &[
        "oh-my-openagent.jsonc",
        "oh-my-openagent.json",
        "oh-my-opencode.jsonc",
        "oh-my-opencode.json",
    ],
    category: "omo",
    provider_prefix: "omo-",
    plugin_name: "oh-my-openagent@latest",
    plugin_prefixes: &["oh-my-openagent", "oh-my-opencode"],
    has_categories: true,
    label: "OMO",
    import_label: "Imported",
};

pub const SLIM: OmoVariant = OmoVariant {
    preferred_filename: "oh-my-opencode-slim.jsonc",
    config_candidates: &["oh-my-opencode-slim.jsonc", "oh-my-opencode-slim.json"],
    category: "omo-slim",
    provider_prefix: "omo-slim-",
    plugin_name: "oh-my-opencode-slim@latest",
    plugin_prefixes: &["oh-my-opencode-slim"],
    has_categories: false,
    label: "OMO Slim",
    import_label: "Imported Slim",
};

// ── Service ────────────────────────────────────────────────────

pub struct OmoService;

impl OmoService {
    // ── Path helpers ────────────────────────────────────────

    fn config_candidates(v: &OmoVariant, base_dir: &Path) -> Vec<PathBuf> {
        v.config_candidates
            .iter()
            .map(|name| base_dir.join(name))
            .collect()
    }

    fn find_existing_config_path(v: &OmoVariant, base_dir: &Path) -> Option<PathBuf> {
        Self::config_candidates(v, base_dir)
            .into_iter()
            .find(|path| path.exists())
    }

    fn find_unified_config_path(
        v: &OmoVariant,
        home_dirs: &[PathBuf],
    ) -> Result<Option<PathBuf>, AppError> {
        if v.category != STANDARD.category {
            return Ok(None);
        }

        for home_dir in home_dirs {
            let config_dir = home_dir.join(".omo");
            for filename in UNIFIED_CONFIG_FILENAMES {
                let path = config_dir.join(filename);
                if path.try_exists().map_err(|e| AppError::io(&path, e))? {
                    UnifiedConfigDocument::load(&path)?;
                    return Ok(Some(path));
                }
            }
        }

        Ok(None)
    }

    /// Home-directory candidates for unified-config detection, most specific
    /// first.
    ///
    /// `get_home_dir()` is always the OS home of this (Windows) process. When
    /// the effective OpenCode config directory points into WSL
    /// (`\\wsl$\<distro>\home\<user>\...`), the WSL-resident OMO keeps its
    /// unified config in the *WSL* home, so derive that home and probe it
    /// first — otherwise detection never sees it and silently falls back to
    /// the legacy per-plugin file that OMO >= 4.19.3 no longer reads (#7363).
    fn unified_home_candidates(home_dir: &Path, _opencode_dir: &Path) -> Vec<PathBuf> {
        #[cfg(windows)]
        let wsl_home = derive_wsl_home_dir(_opencode_dir).filter(|wsl_home| wsl_home != home_dir);
        #[cfg(not(windows))]
        let wsl_home: Option<PathBuf> = None;

        wsl_home
            .into_iter()
            .chain(std::iter::once(home_dir.to_path_buf()))
            .collect()
    }

    fn find_config_location(
        v: &OmoVariant,
        home_dirs: &[PathBuf],
        legacy_dir: &Path,
    ) -> Result<Option<OmoConfigLocation>, AppError> {
        if let Some(path) = Self::find_unified_config_path(v, home_dirs)? {
            return Ok(Some(OmoConfigLocation::Unified(path)));
        }

        Ok(Self::find_existing_config_path(v, legacy_dir).map(OmoConfigLocation::Legacy))
    }

    fn config_location(
        v: &OmoVariant,
        home_dirs: &[PathBuf],
        legacy_dir: &Path,
    ) -> Result<OmoConfigLocation, AppError> {
        Ok(Self::find_config_location(v, home_dirs, legacy_dir)?
            .unwrap_or_else(|| OmoConfigLocation::Legacy(legacy_dir.join(v.preferred_filename))))
    }

    fn resolve_local_config_location(v: &OmoVariant) -> Result<OmoConfigLocation, AppError> {
        let opencode_dir = get_opencode_dir();
        let home_dirs = Self::unified_home_candidates(&get_home_dir(), &opencode_dir);
        Self::find_config_location(v, &home_dirs, &opencode_dir)?.ok_or(AppError::OmoConfigNotFound)
    }

    fn read_jsonc_object(path: &Path) -> Result<Map<String, Value>, AppError> {
        let content = std::fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
        let parsed: Value = json5::from_str(&content)
            .map_err(|e| AppError::Config(format!("Failed to parse OMO config: {e}")))?;
        parsed
            .as_object()
            .cloned()
            .ok_or_else(|| AppError::Config("Expected JSON object".to_string()))
    }

    fn read_config_object(location: &OmoConfigLocation) -> Result<Map<String, Value>, AppError> {
        let mut root = Self::read_jsonc_object(location.path())?;
        match location {
            OmoConfigLocation::Unified(_) => match root.remove(OPENCODE_SECTION_KEY) {
                None => Err(AppError::OmoConfigNotFound),
                Some(Value::Object(section)) => Ok(section),
                Some(_) => Err(AppError::Config(format!(
                    "OMO [opencode] section must be an object: {}",
                    location.path().display()
                ))),
            },
            OmoConfigLocation::Legacy(_) => Ok(root),
        }
    }

    fn remove_unified_config_section(path: &Path) -> Result<OptionalConfigFileVersions, AppError> {
        let mut document = UnifiedConfigDocument::load(path)?;
        let previous_contents = document.document.original_source().as_bytes().to_vec();
        if !document.remove_opencode_section()? {
            return Ok(None);
        }
        let expected_contents = document.save()?;
        Ok(Some((previous_contents, expected_contents)))
    }

    // ── Field extraction ───────────────────────────────────

    fn extract_other_fields_with_keys(
        obj: &Map<String, Value>,
        known: &[&str],
    ) -> Map<String, Value> {
        let mut other = Map::new();
        for (k, v) in obj {
            if !known.contains(&k.as_str()) {
                other.insert(k.clone(), v.clone());
            }
        }
        other
    }

    // ── Merge helpers ──────────────────────────────────────

    fn insert_opt_value(result: &mut Map<String, Value>, key: &str, value: &Option<Value>) {
        if let Some(v) = value {
            result.insert(key.to_string(), v.clone());
        }
    }

    fn insert_object_entries(result: &mut Map<String, Value>, value: Option<&Value>) {
        if let Some(Value::Object(map)) = value {
            for (k, v) in map {
                result.insert(k.clone(), v.clone());
            }
        }
    }

    fn profile_data_from_provider(provider: &Provider, v: &OmoVariant) -> OmoProfileData {
        let agents = provider.settings_config.get("agents").cloned();
        let categories = if v.has_categories {
            provider.settings_config.get("categories").cloned()
        } else {
            None
        };
        let other_fields = provider.settings_config.get("otherFields").cloned();
        (agents, categories, other_fields)
    }

    fn snapshot_config_file(path: &Path) -> Result<Option<Vec<u8>>, AppError> {
        match std::fs::read(path) {
            Ok(contents) => Ok(Some(contents)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(AppError::io(path, err)),
        }
    }

    fn restore_config_file(path: &Path, snapshot: Option<&[u8]>) -> Result<(), AppError> {
        match snapshot {
            Some(bytes) => atomic_write(path, bytes),
            None => match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(AppError::io(path, err)),
            },
        }
    }

    fn restore_config_file_if_unchanged(
        path: &Path,
        expected_contents: Option<&[u8]>,
        snapshot: Option<&[u8]>,
    ) -> Result<(), AppError> {
        let _guard = omo_write_lock().lock()?;
        let current_contents = Self::snapshot_config_file(path)?;
        if current_contents.as_deref() != expected_contents {
            return Err(AppError::Config(format!(
                "Config changed after CC Switch wrote it; refusing to roll back {}",
                path.display()
            )));
        }
        Self::restore_config_file(path, snapshot)
    }

    fn write_profile_config(
        v: &OmoVariant,
        profile_data: Option<&OmoProfileData>,
    ) -> Result<(), AppError> {
        let _operation_guard = omo_operation_lock().lock()?;
        let plugin_config_path = crate::opencode_config::get_opencode_config_path()?;
        let legacy_dir = plugin_config_path.parent().ok_or_else(|| {
            AppError::Config("OpenCode config path has no parent directory".to_string())
        })?;
        let home_dirs = Self::unified_home_candidates(&get_home_dir(), legacy_dir);
        let merged = Self::build_config(v, profile_data);
        let location = Self::config_location(v, &home_dirs, legacy_dir)?;
        let config_path = location.path().to_path_buf();

        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }

        let (previous_contents, expected_contents) = match &location {
            OmoConfigLocation::Unified(path) => {
                let mut document = UnifiedConfigDocument::load(path)?;
                if document.set_opencode_section(&merged)? {
                    let previous_contents =
                        Some(document.document.original_source().as_bytes().to_vec());
                    let expected_contents = Some(document.save()?);
                    (previous_contents, expected_contents)
                } else {
                    (None, None)
                }
            }
            OmoConfigLocation::Legacy(path) => {
                let _guard = omo_write_lock().lock()?;
                let previous_contents = Self::snapshot_config_file(path)?;
                let expected_contents = Some(write_json_file_with_contents(path, &merged)?);
                (previous_contents, expected_contents)
            }
        };
        if let Err(err) = crate::opencode_config::add_plugin(v.plugin_name) {
            if expected_contents.is_some() {
                if let Err(rollback_err) = Self::restore_config_file_if_unchanged(
                    &config_path,
                    expected_contents.as_deref(),
                    previous_contents.as_deref(),
                ) {
                    log::warn!(
                        "Failed to roll back {} config after plugin sync error: {}",
                        v.label,
                        rollback_err
                    );
                }
            }
            return Err(err);
        }
        if expected_contents.is_some() {
            log::info!("{} config written to {config_path:?}", v.label);
        }
        Ok(())
    }

    // ── Public API (variant-parameterized) ─────────────────

    pub fn delete_config_file(v: &OmoVariant) -> Result<(), AppError> {
        let _operation_guard = omo_operation_lock().lock()?;
        let plugin_config_path = crate::opencode_config::get_opencode_config_path()?;
        let base_dir = plugin_config_path
            .parent()
            .ok_or_else(|| {
                AppError::Config("OpenCode config path has no parent directory".to_string())
            })?
            .to_path_buf();
        let home_dirs = Self::unified_home_candidates(&get_home_dir(), &base_dir);
        let unified_path = Self::find_unified_config_path(v, &home_dirs)?;
        let mut legacy_paths = Vec::new();
        for path in Self::config_candidates(v, &base_dir) {
            if path.try_exists().map_err(|e| AppError::io(&path, e))? {
                legacy_paths.push(path);
            }
        }

        let mut applied_changes = Vec::new();

        let result = (|| -> Result<(), AppError> {
            if let Some(path) = &unified_path {
                if let Some((snapshot, expected_contents)) =
                    Self::remove_unified_config_section(path)?
                {
                    applied_changes.push((path.clone(), Some(snapshot), Some(expected_contents)));
                }
            }
            for path in &legacy_paths {
                let _guard = omo_write_lock().lock()?;
                let snapshot = Self::snapshot_config_file(path)?;
                if snapshot.is_none() {
                    continue;
                }
                std::fs::remove_file(path).map_err(|e| AppError::io(path, e))?;
                applied_changes.push((path.clone(), snapshot, None));
            }
            crate::opencode_config::remove_plugins_by_prefixes(v.plugin_prefixes)?;
            Ok(())
        })();

        if let Err(err) = result {
            for (path, snapshot, expected_contents) in applied_changes.iter().rev() {
                if let Err(rollback_err) = Self::restore_config_file_if_unchanged(
                    path,
                    expected_contents.as_deref(),
                    snapshot.as_deref(),
                ) {
                    log::warn!(
                        "Failed to roll back OMO disable change at {path:?}: {rollback_err}"
                    );
                }
            }
            return Err(err);
        }

        let changed_paths: Vec<_> = applied_changes
            .iter()
            .map(|(path, _, _)| path.clone())
            .collect();
        if !changed_paths.is_empty() {
            log::info!(
                "{} config files updated or deleted: {changed_paths:?}",
                v.label
            );
        }
        Ok(())
    }

    pub fn write_config_to_file(state: &AppState, v: &OmoVariant) -> Result<(), AppError> {
        let current_omo = state.db.get_current_omo_provider("opencode", v.category)?;
        let profile_data = current_omo
            .as_ref()
            .map(|provider| Self::profile_data_from_provider(provider, v));
        Self::write_profile_config(v, profile_data.as_ref())
    }

    pub fn write_provider_config_to_file(
        provider: &Provider,
        v: &OmoVariant,
    ) -> Result<(), AppError> {
        let profile_data = Self::profile_data_from_provider(provider, v);
        Self::write_profile_config(v, Some(&profile_data))
    }

    fn build_config(v: &OmoVariant, profile_data: Option<&OmoProfileData>) -> Value {
        let mut result = Map::new();
        if let Some((agents, categories, other_fields)) = profile_data {
            Self::insert_object_entries(&mut result, other_fields.as_ref());
            Self::insert_opt_value(&mut result, "agents", agents);
            if v.has_categories {
                Self::insert_opt_value(&mut result, "categories", categories);
            }
        }
        Value::Object(result)
    }

    pub fn import_from_local(
        state: &AppState,
        v: &OmoVariant,
    ) -> Result<crate::provider::Provider, AppError> {
        let location = Self::resolve_local_config_location(v)?;
        let obj = Self::read_config_object(&location)?;

        let mut settings = Map::new();
        if let Some(agents) = obj.get("agents") {
            settings.insert("agents".to_string(), agents.clone());
        }
        if v.has_categories {
            if let Some(categories) = obj.get("categories") {
                settings.insert("categories".to_string(), categories.clone());
            }
        }

        let other = Self::extract_other_fields_with_keys(&obj, &["agents", "categories"]);
        if !other.is_empty() {
            settings.insert("otherFields".to_string(), Value::Object(other));
        }

        let provider_id = format!("{}{}", v.provider_prefix, uuid::Uuid::new_v4());
        let name = format!(
            "{} {}",
            v.import_label,
            chrono::Local::now().format("%Y-%m-%d %H:%M")
        );
        let settings_config =
            serde_json::to_value(&settings).unwrap_or_else(|_| serde_json::json!({}));

        let provider = crate::provider::Provider {
            id: provider_id,
            name,
            settings_config,
            website_url: None,
            category: Some(v.category.to_string()),
            created_at: Some(chrono::Utc::now().timestamp_millis()),
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        state.db.save_provider("opencode", &provider)?;
        state
            .db
            .set_omo_provider_current("opencode", &provider.id, v.category)?;
        Self::write_config_to_file(state, v)?;
        Ok(provider)
    }

    pub fn read_local_file(v: &OmoVariant) -> Result<OmoLocalFileData, AppError> {
        let location = Self::resolve_local_config_location(v)?;
        let actual_path = location.path().to_path_buf();
        let metadata = std::fs::metadata(&actual_path).ok();
        let last_modified = metadata
            .and_then(|m| m.modified().ok())
            .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());

        let obj = Self::read_config_object(&location)?;

        Ok(Self::build_local_file_data(
            v,
            &obj,
            actual_path.to_string_lossy().to_string(),
            last_modified,
        ))
    }

    fn build_local_file_data(
        v: &OmoVariant,
        obj: &Map<String, Value>,
        file_path: String,
        last_modified: Option<String>,
    ) -> OmoLocalFileData {
        let agents = obj.get("agents").cloned();
        let categories = if v.has_categories {
            obj.get("categories").cloned()
        } else {
            None
        };

        let other = Self::extract_other_fields_with_keys(obj, &["agents", "categories"]);
        let other_fields = if other.is_empty() {
            None
        } else {
            Some(Value::Object(other))
        };

        OmoLocalFileData {
            agents,
            categories,
            other_fields,
            file_path,
            last_modified,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_jsonc_object_supports_comments_and_trailing_commas() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        std::fs::write(
            &path,
            r#"{
  // This is a comment
  "key": "value",
  "key2": "val//ue",
}"#,
        )
        .unwrap();

        let parsed = OmoService::read_jsonc_object(&path).unwrap();
        assert_eq!(parsed["key"], "value");
        assert_eq!(parsed["key2"], "val//ue");
    }

    #[test]
    fn test_find_config_location_prefers_unified_opencode_block() {
        let home = tempfile::tempdir().unwrap();
        let unified_path = home.path().join(".omo").join("omo.jsonc");
        let legacy_dir = home.path().join(".config").join("opencode");
        std::fs::create_dir_all(unified_path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(
            &unified_path,
            r#"{"[opencode]":{"agents":{}},"[codex]":{"agents":{}}}"#,
        )
        .unwrap();
        std::fs::write(
            legacy_dir.join(STANDARD.preferred_filename),
            r#"{"agents":{}}"#,
        )
        .unwrap();

        let found =
            OmoService::find_config_location(&STANDARD, &[home.path().to_path_buf()], &legacy_dir)
                .unwrap();

        assert_eq!(found, Some(OmoConfigLocation::Unified(unified_path)));
    }

    #[test]
    fn test_find_config_location_uses_unified_when_opencode_block_is_missing() {
        let home = tempfile::tempdir().unwrap();
        let unified_path = home.path().join(".omo").join("omo.jsonc");
        let legacy_dir = home.path().join(".config").join("opencode");
        std::fs::create_dir_all(unified_path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(&unified_path, r#"{"[codex]":{"agents":{}}}"#).unwrap();
        std::fs::write(
            legacy_dir.join(STANDARD.preferred_filename),
            r#"{"agents":{}}"#,
        )
        .unwrap();

        let found =
            OmoService::find_config_location(&STANDARD, &[home.path().to_path_buf()], &legacy_dir)
                .unwrap();

        assert_eq!(found, Some(OmoConfigLocation::Unified(unified_path)));
    }

    #[test]
    fn test_find_config_location_uses_unified_json_fallback() {
        let home = tempfile::tempdir().unwrap();
        let unified_path = home.path().join(".omo").join("omo.json");
        let legacy_dir = home.path().join(".config").join("opencode");
        std::fs::create_dir_all(unified_path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(&unified_path, r#"{"[opencode]":{"agents":{}}}"#).unwrap();
        std::fs::write(
            legacy_dir.join(STANDARD.preferred_filename),
            r#"{"agents":{}}"#,
        )
        .unwrap();

        let found =
            OmoService::find_config_location(&STANDARD, &[home.path().to_path_buf()], &legacy_dir)
                .unwrap();

        assert_eq!(found, Some(OmoConfigLocation::Unified(unified_path)));
    }

    #[test]
    fn test_find_config_location_probes_wsl_side_home_candidate() {
        // #7363: with the OpenCode config dir inside WSL, the OMO unified
        // config lives in the WSL-side home, not the Windows home. Detection
        // must probe the derived WSL home candidate, otherwise it misses the
        // unified file and falls back to the legacy path OMO ignores.
        let windows_home = tempfile::tempdir().unwrap();
        let wsl_home = tempfile::tempdir().unwrap();
        let legacy_dir = windows_home.path().join(".config").join("opencode");
        std::fs::create_dir_all(&legacy_dir).unwrap();

        // The unified config exists only on the WSL side.
        let unified_path = wsl_home.path().join(".omo").join("omo.jsonc");
        std::fs::create_dir_all(unified_path.parent().unwrap()).unwrap();
        std::fs::write(&unified_path, r#"{"[opencode]":{"agents":{}}}"#).unwrap();
        std::fs::write(
            legacy_dir.join(STANDARD.preferred_filename),
            r#"{"agents":{}}"#,
        )
        .unwrap();

        let candidates = vec![
            wsl_home.path().to_path_buf(),
            windows_home.path().to_path_buf(),
        ];
        let found = OmoService::find_config_location(&STANDARD, &candidates, &legacy_dir).unwrap();

        assert_eq!(found, Some(OmoConfigLocation::Unified(unified_path)));
    }

    #[cfg(windows)]
    #[test]
    fn test_unified_home_candidates_prefers_wsl_side_home() {
        let os_home = PathBuf::from(r"C:\Users\travis");
        let opencode_dir =
            PathBuf::from(r"\\wsl.localhost\Ubuntu-26.04\home\travis\.config\opencode");

        let candidates = OmoService::unified_home_candidates(&os_home, &opencode_dir);

        assert_eq!(
            candidates,
            vec![
                PathBuf::from(r"\\wsl.localhost\Ubuntu-26.04\home\travis"),
                os_home
            ]
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_unified_home_candidates_keeps_os_home_for_local_dirs() {
        let os_home = PathBuf::from(r"C:\Users\travis");
        let opencode_dir = PathBuf::from(r"C:\Users\travis\.config\opencode");

        let candidates = OmoService::unified_home_candidates(&os_home, &opencode_dir);

        assert_eq!(candidates, vec![os_home]);
    }

    #[test]
    fn test_unified_config_reads_only_opencode_block() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        std::fs::write(
            &path,
            r#"{
  "models": {"shared": {"model": "shared/model"}},
  "[opencode]": {"agents": {"sisyphus": {"model": "openai/gpt-5.3"}}},
  "[codex]": {"agents": {"reviewer": {"model": "openai/gpt-5.4"}}}
}"#,
        )
        .unwrap();

        let obj = OmoService::read_config_object(&OmoConfigLocation::Unified(path)).unwrap();

        assert_eq!(obj["agents"]["sisyphus"]["model"], "openai/gpt-5.3");
        assert!(!obj.contains_key("models"));
        assert!(!obj.contains_key("[codex]"));
    }

    #[test]
    fn test_unified_config_rejects_non_object_opencode_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        let original = r#"{"[opencode]":[],"[codex]":{"agents":{}}}"#;
        std::fs::write(&path, original).unwrap();

        let read_result = OmoService::read_config_object(&OmoConfigLocation::Unified(path.clone()));
        let mut document = UnifiedConfigDocument::load(&path).unwrap();
        let write_result = document.set_opencode_section(&serde_json::json!({"agents": {}}));

        assert!(read_result.is_err());
        assert!(write_result.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_unified_config_write_preserves_other_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        std::fs::write(
            &path,
            r#"{
  // Migration state belongs to the shared document.
  "_migrations": ["2026-07-opencode-config-unification"],
  "[opencode]": {"agents": {"old": {"model": "old/model"}}},
  // Codex settings must survive OpenCode profile switches.
  "[codex]": {"agents": {"reviewer": {"model": "openai/gpt-5.4"}}}
}"#,
        )
        .unwrap();
        let profile_data = (
            Some(serde_json::json!({"sisyphus": {"model": "openai/gpt-5.3"}})),
            None,
            None,
        );
        let config = OmoService::build_config(&STANDARD, Some(&profile_data));

        let mut document = UnifiedConfigDocument::load(&path).unwrap();
        document.set_opencode_section(&config).unwrap();
        let _written_contents = document.save().unwrap();
        let source = std::fs::read_to_string(&path).unwrap();
        let document: Value = json5::from_str(&source).unwrap();

        assert_eq!(
            document["_migrations"],
            serde_json::json!(["2026-07-opencode-config-unification"])
        );
        assert_eq!(
            document["[codex]"]["agents"]["reviewer"]["model"],
            "openai/gpt-5.4"
        );
        assert_eq!(
            document["[opencode]"]["agents"]["sisyphus"]["model"],
            "openai/gpt-5.3"
        );
        assert!(source.contains("// Migration state belongs to the shared document."));
        assert!(source.contains("// Codex settings must survive OpenCode profile switches."));
    }

    #[test]
    fn test_unified_config_semantic_noop_preserves_source() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        let original = r#"{
  "[opencode]": {
    // Keep this agent explanation and the original quoting.
    agents: {'sisyphus': {model: 'openai/gpt-5.3'}},
  },
}"#;
        std::fs::write(&path, original).unwrap();
        let root: Value = json5::from_str(original).unwrap();
        let desired = root[OPENCODE_SECTION_KEY].clone();

        let mut document = UnifiedConfigDocument::load(&path).unwrap();
        let changed = document.set_opencode_section(&desired).unwrap();

        assert!(!changed);
        assert_eq!(document.document.validated_source().unwrap(), original);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_unified_config_merge_preserves_nested_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        let original = r#"{
  "[opencode]": {
    // Keep the agents explanation.
    "agents": {
      "sisyphus": {
        // Keep the model explanation.
        "model": "old/model",
        "temperature": 0.2
      }
    },
    "disabled_agents": [
      "one", // Keep the first array item explanation.
      "two"
    ]
  }
}"#;
        std::fs::write(&path, original).unwrap();
        let desired = serde_json::json!({
            "agents": {
                "sisyphus": {
                    "model": "new/model",
                    "temperature": 0.2
                }
            },
            "disabled_agents": ["one", "two", "three"]
        });

        let mut document = UnifiedConfigDocument::load(&path).unwrap();
        assert!(document.set_opencode_section(&desired).unwrap());
        document.save().unwrap();

        let source = std::fs::read_to_string(&path).unwrap();
        let root = OmoService::read_jsonc_object(&path).unwrap();
        assert_eq!(root[OPENCODE_SECTION_KEY], desired);
        assert!(source.contains("// Keep the agents explanation."));
        assert!(source.contains("// Keep the model explanation."));
        assert!(source.contains("// Keep the first array item explanation."));
    }

    #[test]
    fn test_unified_config_write_inserts_missing_opencode_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        std::fs::write(
            &path,
            r#"{
  "_migrations": ["2026-07-opencode-config-unification"],
  // Keep the Codex section in place.
  "[codex]": {"agents": {"reviewer": {"model": "openai/gpt-5.4"}}}
}"#,
        )
        .unwrap();

        let mut document = UnifiedConfigDocument::load(&path).unwrap();
        document
            .set_opencode_section(&serde_json::json!({
                "agents": {"sisyphus": {"model": "openai/gpt-5.3"}}
            }))
            .unwrap();
        document.save().unwrap();

        let source = std::fs::read_to_string(&path).unwrap();
        let root = OmoService::read_jsonc_object(&path).unwrap();
        assert_eq!(
            root["[opencode]"]["agents"]["sisyphus"]["model"],
            "openai/gpt-5.3"
        );
        assert_eq!(
            root["[codex]"]["agents"]["reviewer"]["model"],
            "openai/gpt-5.4"
        );
        assert!(source.contains("// Keep the Codex section in place."));
    }

    #[test]
    fn test_disable_then_reenable_stays_on_unified_path() {
        let home = tempfile::tempdir().unwrap();
        let unified_path = home.path().join(".omo").join("omo.jsonc");
        let legacy_dir = home.path().join(".config").join("opencode");
        std::fs::create_dir_all(unified_path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(
            &unified_path,
            r#"{
  "_migrations": ["2026-07-opencode-config-unification"],
  "[opencode]": {"agents": {"old": {}}},
  "[codex]": {"agents": {"reviewer": {}}}
}"#,
        )
        .unwrap();

        OmoService::remove_unified_config_section(&unified_path).unwrap();
        let location =
            OmoService::find_config_location(&STANDARD, &[home.path().to_path_buf()], &legacy_dir)
                .unwrap();
        assert_eq!(
            location,
            Some(OmoConfigLocation::Unified(unified_path.clone()))
        );

        let mut document = UnifiedConfigDocument::load(&unified_path).unwrap();
        document
            .set_opencode_section(&serde_json::json!({"agents": {"new": {}}}))
            .unwrap();
        document.save().unwrap();

        let root = OmoService::read_jsonc_object(&unified_path).unwrap();
        assert!(root["[opencode]"]["agents"].get("new").is_some());
        assert!(root.contains_key("[codex]"));
        assert_eq!(
            root["_migrations"],
            serde_json::json!(["2026-07-opencode-config-unification"])
        );
    }

    #[test]
    fn test_unified_config_write_preserves_block_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        let original = r#"{ /* keep */ "[opencode]": {"agents": {}} }"#;
        std::fs::write(&path, original).unwrap();
        let mut document = UnifiedConfigDocument::load(&path).unwrap();
        document
            .set_opencode_section(&serde_json::json!({"agents": {"new": {}}}))
            .unwrap();

        document.save().unwrap();
        let source = std::fs::read_to_string(&path).unwrap();
        assert!(source.contains("/* keep */"));
        assert_eq!(
            OmoService::read_jsonc_object(&path).unwrap()["[opencode]"]["agents"],
            serde_json::json!({"new": {}})
        );
    }

    #[test]
    fn test_unified_config_write_preserves_fields_between_comment_markers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        let original = r#"{
  /* block */
  "[codex]": {"agents": {"reviewer": {}}},
  // tail */
  "[opencode]": {"agents": {}}
}"#;
        std::fs::write(&path, original).unwrap();
        let mut document = UnifiedConfigDocument::load(&path).unwrap();
        document
            .set_opencode_section(&serde_json::json!({"agents": {"new": {}}}))
            .unwrap();

        document.save().unwrap();
        let source = std::fs::read_to_string(&path).unwrap();
        assert!(source.contains("/* block */"));
        assert!(source.contains("// tail */"));
        assert_eq!(
            OmoService::read_jsonc_object(&path).unwrap()["[codex]"],
            serde_json::json!({"agents":{"reviewer":{}}})
        );
    }

    #[test]
    fn test_unified_config_write_rejects_corrupted_output() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        let original = r#"{"[opencode]":{"agents":{}}}"#;
        std::fs::write(&path, original).unwrap();
        for parseable in [false, true] {
            let mut document = UnifiedConfigDocument::load(&path).unwrap();
            document
                .set_opencode_section(&serde_json::json!({"agents":{"new":{}}}))
                .unwrap();
            document.document.corrupt_output_for_test(parseable);
            assert!(document.save().is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        }
    }

    #[test]
    fn test_unified_config_write_preserves_crlf_line_endings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        std::fs::write(&path, "{\r\n  \"[codex]\": {\"agents\": {}}\r\n}\r\n").unwrap();
        let mut document = UnifiedConfigDocument::load(&path).unwrap();
        document
            .set_opencode_section(&serde_json::json!({
                "agents": {"sisyphus": {"model": "openai/gpt-5.3"}}
            }))
            .unwrap();

        document.save().unwrap();

        let source = std::fs::read_to_string(&path).unwrap();
        assert!(!source.replace("\r\n", "").contains('\n'));
        assert_eq!(
            OmoService::read_jsonc_object(&path).unwrap()["[opencode]"]["agents"]["sisyphus"]
                ["model"],
            "openai/gpt-5.3"
        );
    }

    #[test]
    fn test_remove_unified_config_section_preserves_other_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        std::fs::write(
            &path,
            r#"{
  "[opencode]": {"agents": {}},
  // Keep this Codex explanation when disabling OpenCode OMO.
  "[codex]": {"agents": {}},
  "_migrations": ["done"]
}"#,
        )
        .unwrap();

        assert!(OmoService::remove_unified_config_section(&path)
            .unwrap()
            .is_some());

        let source = std::fs::read_to_string(&path).unwrap();
        let root = OmoService::read_jsonc_object(&path).unwrap();
        assert!(!root.contains_key(OPENCODE_SECTION_KEY));
        assert!(root.contains_key("[codex]"));
        assert_eq!(root["_migrations"], serde_json::json!(["done"]));
        assert!(source.contains("// Keep this Codex explanation when disabling OpenCode OMO."));

        assert!(OmoService::remove_unified_config_section(&path)
            .unwrap()
            .is_none());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
    }

    #[test]
    fn test_unified_config_write_rejects_concurrent_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        std::fs::write(&path, r#"{"[opencode]":{"agents":{}}}"#).unwrap();
        let mut document = UnifiedConfigDocument::load(&path).unwrap();
        document
            .set_opencode_section(&serde_json::json!({"agents": {"new": {}}}))
            .unwrap();
        let concurrent_source = r#"{"[opencode]":{"agents":{}},"[codex]":{"changed":true}}"#;
        std::fs::write(&path, concurrent_source).unwrap();

        let result = document.save();

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), concurrent_source);
    }

    #[test]
    fn test_unified_config_rejects_duplicate_opencode_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        std::fs::write(
            &path,
            r#"{"[opencode]":{"agents":{"first":{}}},"[open\u0063ode]":{"agents":{"second":{}}}}"#,
        )
        .unwrap();

        let result = UnifiedConfigDocument::load(&path);

        assert!(result.is_err());
    }

    #[test]
    fn test_rollback_refuses_to_overwrite_newer_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        let original = br#"{"[opencode]":{"agents":{"old":{}}}}"#;
        let written = br#"{"[opencode]":{"agents":{"ours":{}}}}"#;
        let concurrent = br#"{"[opencode]":{"agents":{"theirs":{}}}}"#;
        std::fs::write(&path, concurrent).unwrap();

        let result =
            OmoService::restore_config_file_if_unchanged(&path, Some(written), Some(original));

        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), concurrent);
    }

    #[test]
    fn test_build_config_empty() {
        let merged = OmoService::build_config(&STANDARD, None);
        assert!(merged.is_object());
        assert!(merged.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_build_config_with_profile() {
        let agents = Some(serde_json::json!({
            "sisyphus": { "model": "claude-opus-4-5" }
        }));
        let categories = None;
        let other_fields = Some(serde_json::json!({
            "$schema": "https://example.com/schema.json",
            "disabled_agents": ["explore"]
        }));
        let profile_data = (agents, categories, other_fields);
        let merged = OmoService::build_config(&STANDARD, Some(&profile_data));
        let obj = merged.as_object().unwrap();

        assert_eq!(obj["$schema"], "https://example.com/schema.json");
        assert_eq!(obj["disabled_agents"], serde_json::json!(["explore"]));
        assert!(obj.contains_key("agents"));
        assert_eq!(obj["agents"]["sisyphus"]["model"], "claude-opus-4-5");
    }

    #[test]
    fn test_build_local_file_data_keeps_all_non_agent_category_fields_in_other() {
        let obj = serde_json::json!({
            "$schema": "https://example.com/schema.json",
            "disabled_agents": ["oracle"],
            "agents": {
                "sisyphus": { "model": "claude-opus-4-6" }
            },
            "categories": {
                "code": { "model": "gpt-5.3" }
            },
            "custom_top_level": {
                "enabled": true
            }
        });
        let obj_map = obj.as_object().unwrap().clone();

        let data = OmoService::build_local_file_data(
            &STANDARD,
            &obj_map,
            "/tmp/oh-my-opencode.jsonc".to_string(),
            None,
        );

        // All non-agents/categories fields should be in other_fields
        let other = data.other_fields.unwrap();
        let other_obj = other.as_object().unwrap();
        assert_eq!(
            other_obj.get("$schema").unwrap(),
            "https://example.com/schema.json"
        );
        assert_eq!(
            other_obj.get("disabled_agents").unwrap(),
            &serde_json::json!(["oracle"])
        );
        assert_eq!(
            other_obj.get("custom_top_level").unwrap(),
            &serde_json::json!({"enabled": true})
        );
        // agents and categories should NOT be in other_fields
        assert!(!other_obj.contains_key("agents"));
        assert!(!other_obj.contains_key("categories"));
    }

    #[test]
    fn test_build_config_ignores_non_object_other_fields() {
        let agents = None;
        let categories = None;
        let other_fields = Some(serde_json::json!("profile_non_object"));
        let profile_data = (agents, categories, other_fields);

        let merged = OmoService::build_config(&STANDARD, Some(&profile_data));
        let obj = merged.as_object().unwrap();

        assert!(!obj.contains_key("profile_non_object"));
    }

    #[test]
    fn test_build_config_slim_excludes_categories() {
        let agents = Some(serde_json::json!({"orchestrator": {"model": "k2"}}));
        let categories = Some(serde_json::json!({"code": {"model": "gpt"}}));
        let other_fields = Some(serde_json::json!({
            "$schema": "https://slim.schema",
            "disabled_agents": ["oracle"]
        }));
        let profile_data = (agents, categories, other_fields);

        let merged = OmoService::build_config(&SLIM, Some(&profile_data));
        let obj = merged.as_object().unwrap();

        // Slim should NOT include categories
        assert!(!obj.contains_key("categories"));

        // Slim SHOULD include these
        assert_eq!(obj["$schema"], "https://slim.schema");
        assert!(obj.contains_key("agents"));
        assert!(obj.contains_key("disabled_agents"));
    }

    #[test]
    fn test_find_existing_config_prefers_new_name_over_old() {
        let dir = tempfile::tempdir().unwrap();
        let old_path = dir.path().join("oh-my-opencode.jsonc");
        let new_path = dir.path().join("oh-my-openagent.jsonc");

        // Create both old and new files
        std::fs::write(&old_path, r#"{"agents":{}}"#).unwrap();
        std::fs::write(&new_path, r#"{"agents":{}}"#).unwrap();

        let found = OmoService::find_existing_config_path(&STANDARD, dir.path());
        assert_eq!(
            found.unwrap(),
            new_path,
            "When both old and new config files exist, the new name (oh-my-openagent) must be preferred"
        );
    }

    #[test]
    fn test_find_existing_config_falls_back_to_old_name() {
        let dir = tempfile::tempdir().unwrap();
        let old_path = dir.path().join("oh-my-opencode.jsonc");

        // Only old file exists
        std::fs::write(&old_path, r#"{"agents":{}}"#).unwrap();

        let found = OmoService::find_existing_config_path(&STANDARD, dir.path());
        assert_eq!(
            found.unwrap(),
            old_path,
            "When only the old config file exists, it should still be found"
        );
    }
}
