use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::get_claude_config_dir;
use crate::database::Database;
use crate::error::AppError;
use crate::live::engine::{read_current, LiveFile};
use crate::live::patch::json::{parse, JsonPatch};
use crate::live::patch::{KeyPath, LivePatch, LiveWriteError};
use crate::mode::operation::{AppWrite, FileChange};
use crate::mode::state::{op, PendingTarget};

const MAX_DAYS: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaudeHistoryRetention {
    pub config_path: String,
    pub days: Option<u64>,
}

fn days_at(path: &Path, pre: Option<&[u8]>) -> Result<Option<u64>, LiveWriteError> {
    let (doc, _) = parse(path, pre)?;
    match doc.get("cleanupPeriodDays") {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|days| *days <= MAX_DAYS)
            .map(Some)
            .ok_or_else(|| LiveWriteError::Shape {
                path: path.to_path_buf(),
                key_path: KeyPath::new(&["cleanupPeriodDays"]),
                expected: "JavaScript 可精确表示的非负整数",
            }),
    }
}

fn read_at(path: &Path) -> Result<ClaudeHistoryRetention, AppError> {
    let pre = read_current(path)?;
    Ok(ClaudeHistoryRetention {
        config_path: path.to_string_lossy().into_owned(),
        days: days_at(path, pre.as_deref())?,
    })
}

pub fn get() -> Result<ClaudeHistoryRetention, AppError> {
    read_at(&get_claude_config_dir().join("settings.json"))
}

struct RetentionPatch {
    expected: Option<u64>,
    days: Option<u64>,
}

impl LivePatch for RetentionPatch {
    fn apply(&self, path: &Path, pre: Option<&[u8]>) -> Result<Vec<u8>, LiveWriteError> {
        Ok(self.apply_file(path, pre)?.unwrap_or_default())
    }

    fn apply_file(
        &self,
        path: &Path,
        pre: Option<&[u8]>,
    ) -> Result<Option<Vec<u8>>, LiveWriteError> {
        let current = days_at(path, pre)?;
        if current != self.expected {
            return Err(LiveWriteError::Conflict { path: path.into() });
        }
        if current == self.days {
            return Ok(pre.map(<[u8]>::to_vec));
        }
        let key = KeyPath::new(&["cleanupPeriodDays"]);
        let patch = match self.days {
            Some(days) => JsonPatch {
                set: vec![(key, Value::from(days))],
                ..JsonPatch::default()
            },
            None => JsonPatch {
                remove: vec![key],
                ..JsonPatch::default()
            },
        };
        patch.apply_file(path, pre)
    }
}

pub fn set(
    db: &Database,
    expected: ClaudeHistoryRetention,
    days: Option<u64>,
) -> Result<ClaudeHistoryRetention, AppError> {
    if days.is_some_and(|days| days == 0 || days > MAX_DAYS) {
        return Err(AppError::InvalidInput(
            "cleanupPeriodDays must be a positive safe integer".into(),
        ));
    }
    let write = AppWrite::begin(db, "claude")?;
    let path = get_claude_config_dir().join("settings.json");
    if path.to_string_lossy() != expected.config_path {
        return Err(AppError::Conflict(
            "Claude configuration directory changed".into(),
        ));
    }
    let patch = RetentionPatch {
        expected: expected.days,
        days,
    };
    write.run(
        op::APPLY,
        &[FileChange {
            file: LiveFile::private(&path),
            patch: &patch,
        }],
        PendingTarget::default(),
    )?;
    read_at(&path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::engine::{lock_app, DeviceStore};
    use crate::mode::operation::{self, failpoint};
    use serde_json::json;
    use serial_test::serial;
    use std::fs;

    struct TestHome(Option<std::ffi::OsString>);

    impl TestHome {
        fn at(home: &Path) -> Self {
            let guard = Self(std::env::var_os("CC_SWITCH_TEST_HOME"));
            std::env::set_var("CC_SWITCH_TEST_HOME", home);
            crate::settings::reload_settings().unwrap();
            guard
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            match &self.0 {
                Some(previous) => std::env::set_var("CC_SWITCH_TEST_HOME", previous),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            let _ = crate::settings::reload_settings();
        }
    }

    #[test]
    #[serial]
    fn public_service_uses_standard_settings_and_preserves_legacy_file() {
        let home = tempfile::tempdir().unwrap();
        let _home = TestHome::at(home.path());
        let dir = get_claude_config_dir();
        fs::create_dir_all(&dir).unwrap();
        let legacy = dir.join("claude.json");
        let legacy_bytes = b"{\"cleanupPeriodDays\":730,\"custom\":true}";
        fs::write(&legacy, legacy_bytes).unwrap();
        let path = dir.join("settings.json");
        let expected = get().unwrap();
        assert_eq!(expected.config_path, path.to_string_lossy());
        assert_eq!(expected.days, None);
        assert!(!path.exists());

        let db = Database::memory().unwrap();
        let saved = set(&db, expected, Some(90)).unwrap();
        assert_eq!(saved.days, Some(90));
        assert_eq!(get().unwrap(), saved);
        assert_eq!(fs::read(&legacy).unwrap(), legacy_bytes);
        let reset = set(&db, saved, None).unwrap();
        assert_eq!(reset.days, None);
        assert_eq!(fs::read(&legacy).unwrap(), legacy_bytes);
        assert!(serde_json::from_slice::<Value>(&fs::read(path).unwrap())
            .unwrap()
            .get("cleanupPeriodDays")
            .is_none());
    }

    #[test]
    #[serial]
    fn public_service_rejects_saved_path_after_directory_changes() {
        let home = tempfile::tempdir().unwrap();
        let _home = TestHome::at(home.path());
        let a = home.path().join("a");
        let b = home.path().join("b");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        let original_a = b"{\"cleanupPeriodDays\":730,\"custom\":true}";
        let original_b = b"{\"cleanupPeriodDays\":365,\"hooks\":{}}";
        fs::write(a.join("settings.json"), original_a).unwrap();
        fs::write(b.join("settings.json"), original_b).unwrap();
        crate::settings::update_settings(crate::settings::AppSettings {
            claude_config_dir: Some(a.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .unwrap();
        let stale = get().unwrap();
        crate::settings::update_settings(crate::settings::AppSettings {
            claude_config_dir: Some(b.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .unwrap();

        let db = Database::memory().unwrap();
        assert!(matches!(
            set(&db, stale, Some(90)),
            Err(AppError::Conflict(_))
        ));
        assert_eq!(fs::read(a.join("settings.json")).unwrap(), original_a);
        assert_eq!(fs::read(b.join("settings.json")).unwrap(), original_b);
        assert_eq!(get().unwrap().days, Some(365));
    }

    fn update(path: &Path, expected: Option<u64>, days: Option<u64>) -> Result<(), AppError> {
        let store = DeviceStore::at(path.parent().unwrap().join("device"));
        let guard = lock_app(&path.to_string_lossy());
        operation::run(
            &store,
            &guard,
            op::APPLY,
            &[FileChange {
                file: LiveFile::private(path),
                patch: &RetentionPatch { expected, days },
            }],
            PendingTarget::default(),
            &|_| Ok(()),
        )?;
        Ok(())
    }

    #[test]
    fn reading_custom_policies_never_changes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        for days in [90, 730, 99999] {
            let bytes = format!("{{ \"cleanupPeriodDays\": {days}, \"hooks\": {{}} }}\n");
            fs::write(&path, &bytes).unwrap();
            assert_eq!(read_at(&path).unwrap().days, Some(days));
            assert_eq!(fs::read_to_string(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn absent_field_and_explicit_default_remain_distinct() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(read_at(&path).unwrap().days, None);
        update(&path, None, None).unwrap();
        assert!(!path.exists(), "reset must not create a missing file");
        update(&path, None, Some(30)).unwrap();
        assert_eq!(read_at(&path).unwrap().days, Some(30));
        update(&path, Some(30), None).unwrap();
        assert_eq!(read_at(&path).unwrap().days, None);
    }

    #[test]
    fn update_and_reset_preserve_unrelated_values_and_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let original = "{\r\n\t\"env\": {\"EXAMPLE_KEY\": \"example\"},\r\n\t\"cleanupPeriodDays\": 730,\r\n\t\"hooks\": {\"Stop\": []},\r\n\t\"custom\": [1, true]\r\n}\r\n";
        fs::write(&path, original).unwrap();
        update(&path, Some(730), Some(90)).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\r\n\t\"env\""));
        assert!(text.ends_with("\r\n"));
        assert!(text.find("\"env\"").unwrap() < text.find("\"cleanupPeriodDays\"").unwrap());
        assert!(text.find("\"cleanupPeriodDays\"").unwrap() < text.find("\"hooks\"").unwrap());
        update(&path, Some(90), None).unwrap();
        let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            value,
            json!({"env": {"EXAMPLE_KEY": "example"}, "hooks": {"Stop": []}, "custom": [1, true]})
        );
        assert_eq!(
            value
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["env", "hooks", "custom"]
        );
    }

    #[test]
    fn identical_policy_is_a_byte_for_byte_noop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let original = b"{  \"cleanupPeriodDays\":730, \"custom\":\"\\u00e9\" }";
        fs::write(&path, original).unwrap();
        update(&path, Some(730), Some(730)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!dir.path().join("device").exists());
    }

    #[test]
    fn malformed_files_and_invalid_field_types_are_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        for original in [
            "{ broken",
            "[]",
            "null",
            "{\"cleanupPeriodDays\":null}",
            "{\"cleanupPeriodDays\":\"90\"}",
            "{\"cleanupPeriodDays\":-1}",
            "{\"cleanupPeriodDays\":1.5}",
            "{\"cleanupPeriodDays\":9007199254740992}",
        ] {
            fs::write(&path, original).unwrap();
            assert!(read_at(&path).is_err(), "{original}");
            assert!(update(&path, None, Some(90)).is_err(), "{original}");
            assert_eq!(fs::read_to_string(&path).unwrap(), original);
        }
    }

    #[test]
    fn stale_read_cannot_replace_an_external_retention_edit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{\"cleanupPeriodDays\":365}").unwrap();
        assert!(update(&path, Some(730), Some(90)).is_err());
        assert_eq!(read_at(&path).unwrap().days, Some(365));
        assert!(update(&path, None, None).is_err());
        assert_eq!(read_at(&path).unwrap().days, Some(365));
    }

    #[test]
    fn replan_preserves_external_edits_to_other_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{\"cleanupPeriodDays\":730}").unwrap();
        let target = path.clone();
        let mut fired = false;
        failpoint::on_before_publish(Some(Box::new(move |_, _| {
            if !fired {
                fired = true;
                fs::write(&target, "{\"cleanupPeriodDays\":730,\"custom\":true}").unwrap();
            }
        })));
        let result = update(&path, Some(730), Some(90));
        failpoint::on_before_publish(None);
        result.unwrap();
        let value: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(value, json!({"cleanupPeriodDays":90,"custom":true}));
    }

    #[test]
    fn replan_rejects_external_retention_changes_during_publish() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{\"cleanupPeriodDays\":730}").unwrap();
        let target = path.clone();
        failpoint::on_before_publish(Some(Box::new(move |_, _| {
            fs::write(&target, "{\"cleanupPeriodDays\":365}").unwrap();
        })));
        let result = update(&path, Some(730), Some(90));
        failpoint::on_before_publish(None);
        assert!(result.is_err());
        assert_eq!(read_at(&path).unwrap().days, Some(365));
    }

    #[test]
    fn invalid_requested_days_are_rejected_before_io() {
        let db = Database::memory().unwrap();
        let expected = ClaudeHistoryRetention {
            config_path: "/unused/settings.json".into(),
            days: None,
        };
        for days in [0, MAX_DAYS + 1] {
            assert!(matches!(
                set(&db, expected.clone(), Some(days)),
                Err(AppError::InvalidInput(_))
            ));
        }
    }
}
