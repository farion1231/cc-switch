//! Helpers shared by unit tests.

use std::ffi::{OsStr, OsString};
use std::path::Path;

/// Process environment changes that last as long as the guard. On drop every
/// variable goes back to what it was before (set again, or removed if it was
/// unset), last change first.
#[must_use]
pub(crate) struct EnvVars(Vec<(&'static str, Option<OsString>)>);

impl EnvVars {
    pub(crate) fn new() -> Self {
        Self(Vec::new())
    }

    pub(crate) fn set(mut self, key: &'static str, value: impl AsRef<OsStr>) -> Self {
        self.0.push((key, std::env::var_os(key)));
        std::env::set_var(key, value);
        self
    }

    pub(crate) fn remove(mut self, key: &'static str) -> Self {
        self.0.push((key, std::env::var_os(key)));
        std::env::remove_var(key);
        self
    }
}

impl Drop for EnvVars {
    fn drop(&mut self) {
        for (key, previous) in self.0.drain(..).rev() {
            match previous {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

/// A temporary directory as `CC_SWITCH_TEST_HOME`, with settings reloaded from
/// it. On drop the previous home comes back and settings are reloaded again.
pub(crate) struct TestHome {
    env: Option<EnvVars>,
    dir: tempfile::TempDir,
}

impl TestHome {
    pub(crate) fn new() -> Self {
        let dir = tempfile::tempdir().expect("create test home");
        let env = EnvVars::new().set("CC_SWITCH_TEST_HOME", dir.path());
        crate::settings::reload_settings().expect("reload settings for test home");
        Self {
            env: Some(env),
            dir,
        }
    }

    pub(crate) fn path(&self) -> &Path {
        self.dir.path()
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        drop(self.env.take());
        let _ = crate::settings::reload_settings();
    }
}

/// A temporary directory as `HOME`, `USERPROFILE` and `CC_SWITCH_TEST_HOME`.
/// All three come back on drop. Settings are not reloaded.
pub(crate) struct TempUserHome {
    _env: EnvVars,
    dir: tempfile::TempDir,
}

impl TempUserHome {
    pub(crate) fn new() -> Self {
        let dir = tempfile::tempdir().expect("create temp user home");
        let env = EnvVars::new()
            .set("HOME", dir.path())
            .set("USERPROFILE", dir.path())
            .set("CC_SWITCH_TEST_HOME", dir.path());
        Self { _env: env, dir }
    }

    pub(crate) fn path(&self) -> &Path {
        self.dir.path()
    }
}

/// Whether some file directly inside `dir` holds exactly `bytes`. Used to check that
/// data a user could still recover by hand was kept rather than overwritten.
pub(crate) fn dir_holds_file_with(dir: &Path, bytes: &[u8]) -> bool {
    std::fs::read_dir(dir)
        .expect("read dir")
        .flatten()
        .filter(|entry| entry.path().is_file())
        .any(|entry| std::fs::read(entry.path()).is_ok_and(|content| content == bytes))
}

mod tests {
    use super::EnvVars;

    // Each test uses its own variable names, so they need no #[serial].

    #[test]
    fn env_vars_restores_set_values_and_removes_new_ones() {
        std::env::set_var("CCS_ENV_VARS_TEST_SET", "before");
        std::env::set_var("CCS_ENV_VARS_TEST_REMOVED", "kept");
        std::env::remove_var("CCS_ENV_VARS_TEST_UNSET");
        let env = EnvVars::new()
            .set("CCS_ENV_VARS_TEST_SET", "during")
            .set("CCS_ENV_VARS_TEST_UNSET", "during")
            .remove("CCS_ENV_VARS_TEST_REMOVED");
        assert_eq!(
            std::env::var("CCS_ENV_VARS_TEST_SET").as_deref(),
            Ok("during")
        );
        assert_eq!(
            std::env::var("CCS_ENV_VARS_TEST_UNSET").as_deref(),
            Ok("during")
        );
        assert!(std::env::var_os("CCS_ENV_VARS_TEST_REMOVED").is_none());

        drop(env);
        assert_eq!(
            std::env::var("CCS_ENV_VARS_TEST_SET").as_deref(),
            Ok("before")
        );
        assert!(std::env::var_os("CCS_ENV_VARS_TEST_UNSET").is_none());
        assert_eq!(
            std::env::var("CCS_ENV_VARS_TEST_REMOVED").as_deref(),
            Ok("kept")
        );
    }

    #[test]
    fn env_vars_setting_one_variable_twice_restores_the_original() {
        std::env::set_var("CCS_ENV_VARS_TEST_TWICE", "original");
        let env = EnvVars::new()
            .set("CCS_ENV_VARS_TEST_TWICE", "first")
            .set("CCS_ENV_VARS_TEST_TWICE", "second");
        assert_eq!(
            std::env::var("CCS_ENV_VARS_TEST_TWICE").as_deref(),
            Ok("second")
        );

        drop(env);
        assert_eq!(
            std::env::var("CCS_ENV_VARS_TEST_TWICE").as_deref(),
            Ok("original")
        );
    }
}
