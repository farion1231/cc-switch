//! Shared test environment guard for tests that mutate HOME / CC_SWITCH_TEST_HOME.
//!
//! All tests that use this guard run serially because they mutate process-wide
//! environment variables. The guard restores the original values on drop and
//! cleans up the temporary directory automatically.

use std::ffi::OsString;
use std::sync::{Mutex, OnceLock};
use tempfile::TempDir;

static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub struct TestEnv {
    _lock: std::sync::MutexGuard<'static, ()>,
    _temp_dir: TempDir,
    original_home: Option<OsString>,
    original_test_home: Option<OsString>,
    home_path: std::path::PathBuf,
}

impl TestEnv {
    pub fn new() -> Self {
        let lock = TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|err| err.into_inner());

        let original_home = std::env::var_os("HOME");
        let original_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");

        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let home_path = temp_dir.path().to_path_buf();

        std::env::set_var("HOME", &home_path);
        std::env::set_var("CC_SWITCH_TEST_HOME", &home_path);

        Self {
            _lock: lock,
            _temp_dir: temp_dir,
            original_home,
            original_test_home,
            home_path,
        }
    }

    pub fn home_path(&self) -> &std::path::Path {
        &self.home_path
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        if let Some(ref v) = self.original_home {
            std::env::set_var("HOME", v);
        } else {
            std::env::remove_var("HOME");
        }
        if let Some(ref v) = self.original_test_home {
            std::env::set_var("CC_SWITCH_TEST_HOME", v);
        } else {
            std::env::remove_var("CC_SWITCH_TEST_HOME");
        }
    }
}
