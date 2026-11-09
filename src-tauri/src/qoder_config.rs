//! Qoder editions keep independent settings, authentication and session stores.
use crate::{
    app_config::AppType,
    config::{atomic_write_private, get_home_dir},
    error::AppError,
    jsonc_document::JsoncDocument,
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

static GLOBAL_SETTINGS_LOCK: Mutex<()> = Mutex::new(());
static CN_SETTINGS_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn settings_write_lock(app: &AppType) -> &'static Mutex<()> {
    if *app == AppType::QoderCn {
        &CN_SETTINGS_LOCK
    } else {
        &GLOBAL_SETTINGS_LOCK
    }
}

pub fn is_qoder(app: &AppType) -> bool {
    matches!(app, AppType::Qoder | AppType::QoderCn)
}

pub fn data_dir(app: &AppType) -> PathBuf {
    let (variable, directory) = if *app == AppType::QoderCn {
        ("QODERCN_CONFIG_DIR", ".qoder-cn")
    } else {
        ("QODER_CONFIG_DIR", ".qoder")
    };
    std::env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| get_home_dir().join(directory))
}

pub fn command(app: &AppType) -> &'static str {
    if *app == AppType::QoderCn {
        "qodercn"
    } else {
        "qoder"
    }
}

pub(crate) fn resolve_credential(value: &str) -> String {
    value
        .strip_prefix("${")
        .and_then(|value| value.strip_suffix('}'))
        .map(|name| std::env::var(name).unwrap_or_default())
        .unwrap_or_else(|| value.to_string())
}

/// Every native settings write opts out of usage statistics, including edits
/// that would otherwise leave the file unchanged. Callers hold the edition lock
/// and retain responsibility for rolling back a failed transaction.
pub(crate) fn write_settings(
    path: &Path,
    document: &mut JsoncDocument,
    next: &mut Value,
) -> Result<(), AppError> {
    if document.root_key_count("privacy")? > 1 {
        return Err(AppError::Config(
            "Qoder settings contain duplicate 'privacy' keys".into(),
        ));
    }
    let privacy = next
        .as_object_mut()
        .ok_or_else(|| AppError::Config("Qoder settings must be an object".into()))?
        .entry("privacy")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| AppError::Config("Qoder privacy settings must be an object".into()))?;
    privacy.insert("usageStatisticsEnabled".into(), json!(false));
    if document.apply(next)? {
        atomic_write_private(path, document.validated_source()?.as_bytes())?;
    }
    Ok(())
}

pub(crate) fn ensure_privacy(app: &AppType) -> Result<(), AppError> {
    let _guard = settings_write_lock(app)
        .lock()
        .map_err(|error| AppError::Message(error.to_string()))?;
    ensure_privacy_at(&data_dir(app))
}

pub(crate) fn ensure_privacy_at(root: &Path) -> Result<(), AppError> {
    let path = root.join("settings.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "{}".into(),
        Err(error) => return Err(AppError::io(&path, error)),
    };
    let mut document = JsoncDocument::parse(text.trim_start_matches('\u{feff}'))?;
    let mut next = document.value().clone();
    write_settings(&path, &mut document, &mut next)
}

/// Qoder can disable a skill in settings even when its directory exists.
/// Clear only this skill's user-level disable markers when CC Switch enables it,
/// and roll settings back if the directory/database update fails.
pub(crate) fn enable_skill_and_commit<T>(
    app: &AppType,
    directory: &str,
    commit: impl FnOnce() -> Result<T, crate::error::AppError>,
) -> Result<T, crate::error::AppError> {
    let _guard = settings_write_lock(app)
        .lock()
        .map_err(|error| AppError::Message(error.to_string()))?;
    let path = data_dir(app).join("settings.json");
    enable_skill_file_and_commit(&path, directory, commit)
}

fn enable_skill_file_and_commit<T>(
    path: &std::path::Path,
    directory: &str,
    commit: impl FnOnce() -> Result<T, crate::error::AppError>,
) -> Result<T, crate::error::AppError> {
    crate::mcode_config::write_and_commit(
        path,
        || {
            let text = match std::fs::read_to_string(path) {
                Ok(text) => text,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => "{}".into(),
                Err(error) => return Err(AppError::io(path, error)),
            };
            let mut document = JsoncDocument::parse(text.trim_start_matches('\u{feff}'))?;
            for key in ["skills", "skillOverrides"] {
                if document.root_key_count(key)? > 1
                    || document
                        .value()
                        .get(key)
                        .is_some_and(|value| !value.is_object())
                {
                    return Err(AppError::Config(format!("Invalid Qoder {key} settings")));
                }
            }
            if document
                .value()
                .pointer("/skills/disabled")
                .is_some_and(|value| !value.is_array())
            {
                return Err(AppError::Config(
                    "Qoder skills.disabled must be an array".into(),
                ));
            }
            let mut next = document.value().clone();
            let name = std::path::Path::new(directory)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(directory);
            if let Some(disabled) = next
                .pointer_mut("/skills/disabled")
                .and_then(serde_json::Value::as_array_mut)
            {
                disabled.retain(|item| {
                    item.as_str()
                        .is_none_or(|item| item != directory && item != name)
                });
            }
            if let Some(overrides) = next
                .get_mut("skillOverrides")
                .and_then(serde_json::Value::as_object_mut)
            {
                for key in [directory, name] {
                    if overrides.get(key).and_then(serde_json::Value::as_str) == Some("off") {
                        overrides.remove(key);
                    }
                }
            }
            write_settings(path, &mut document, &mut next)
        },
        commit,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qoder_settings_writes_disable_statistics_even_without_other_changes() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.json");
        for original in [
            "{}",
            "{ // keep comment\n privacy: { usageStatisticsEnabled: true, otherOption: 'keep', }, model: { name: 'auto' }, }",
            "{ // keep comment\n privacy: { usageStatisticsEnabled: false, otherOption: 'keep', }, model: { name: 'auto' }, }",
        ] {
            std::fs::write(&path, original).unwrap();
            let mut document = JsoncDocument::parse(original).unwrap();
            let mut next = document.value().clone();
            write_settings(&path, &mut document, &mut next).unwrap();
            let source = std::fs::read_to_string(&path).unwrap();
            let saved = JsoncDocument::parse(&source).unwrap();
            assert_eq!(saved.value()["privacy"]["usageStatisticsEnabled"], false);
            if original.contains("keep comment") {
                assert!(source.contains("// keep comment"));
                assert_eq!(saved.value()["privacy"]["otherOption"], "keep");
                assert_eq!(saved.value()["model"]["name"], "auto");
            }
            write_settings(&path, &mut document, &mut next).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
        }
    }

    #[test]
    fn qoder_settings_writes_keep_invalid_privacy_settings_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.json");
        for original in [
            r#"{"privacy":null}"#,
            r#"{"privacy":[],"model":{"name":"auto"}}"#,
            r#"{"privacy":{},"privacy":{"usageStatisticsEnabled":true}}"#,
        ] {
            std::fs::write(&path, original).unwrap();
            let mut document = JsoncDocument::parse(original).unwrap();
            let mut next = document.value().clone();
            assert!(write_settings(&path, &mut document, &mut next).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        }
    }

    #[test]
    fn qoder_skill_enable_preserves_jsonc_and_rolls_back_failed_directory_commits() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.json");
        let original="{ // native comment\n\"skills\":{\"disabled\":[\"managed\",\"other\"]},\"skillOverrides\":{\"managed\":\"off\",\"other\":\"off\"},\"model\":{\"name\":\"auto\"},\"privacy\":{\"usageStatisticsEnabled\":true,\"otherOption\":\"keep\"},}";
        std::fs::write(&path, original).unwrap();
        let result: Result<(), crate::error::AppError> =
            enable_skill_file_and_commit(&path, "managed", || {
                Err(crate::error::AppError::Database("fixture".into()))
            });
        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        enable_skill_file_and_commit(&path, "managed", || Ok(())).unwrap();
        let source = std::fs::read_to_string(&path).unwrap();
        assert!(source.contains("// native comment"));
        let value: serde_json::Value = json5::from_str(&source).unwrap();
        assert_eq!(value["skills"]["disabled"], serde_json::json!(["other"]));
        assert_eq!(value["skillOverrides"], serde_json::json!({"other":"off"}));
        assert_eq!(value["model"]["name"], "auto");
        assert_eq!(value["privacy"]["usageStatisticsEnabled"], false);
        assert_eq!(value["privacy"]["otherOption"], "keep");
    }
}
