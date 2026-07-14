use std::path::PathBuf;

pub fn default_codex_home_dir() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .filter(|path| codex_home_env_dir_is_valid(path))
        .unwrap_or_else(default_user_codex_home_dir)
}

fn codex_home_env_dir_is_valid(path: &PathBuf) -> bool {
    !path.as_os_str().is_empty() && !path.to_string_lossy().trim().is_empty() && path.is_dir()
}

fn default_user_codex_home_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().join(".codex"))
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::Path;
    use std::sync::Mutex;

    static CODEX_HOME_ENV_LOCK: Mutex<()> = Mutex::new(());

    struct CodexHomeEnvGuard {
        previous: Option<OsString>,
    }

    impl CodexHomeEnvGuard {
        fn set(path: &Path) -> Self {
            let previous = std::env::var_os("CODEX_HOME");
            unsafe {
                std::env::set_var("CODEX_HOME", path);
            }
            Self { previous }
        }

        fn set_raw(value: &str) -> Self {
            let previous = std::env::var_os("CODEX_HOME");
            unsafe {
                std::env::set_var("CODEX_HOME", value);
            }
            Self { previous }
        }
    }

    impl Drop for CodexHomeEnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.previous {
                    Some(value) => std::env::set_var("CODEX_HOME", value),
                    None => std::env::remove_var("CODEX_HOME"),
                }
            }
        }
    }

    #[test]
    fn default_codex_home_dir_uses_existing_codex_home_env_dir() {
        let _lock = CODEX_HOME_ENV_LOCK.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let codex_home = temp.path().join("custom-codex-home");
        std::fs::create_dir_all(&codex_home).unwrap();
        let _guard = CodexHomeEnvGuard::set(&codex_home);

        assert_eq!(default_codex_home_dir(), codex_home);
        assert_eq!(crate::relay_config::default_codex_home_dir(), codex_home);
        assert_eq!(crate::codex_sqlite::default_codex_home_dir(), codex_home);
    }

    #[test]
    fn default_codex_home_dir_ignores_empty_or_missing_codex_home_env() {
        let _lock = CODEX_HOME_ENV_LOCK.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing-codex-home");
        let expected = default_user_codex_home_dir();

        {
            let _guard = CodexHomeEnvGuard::set_raw("   ");
            assert_eq!(default_codex_home_dir(), expected);
            assert_eq!(crate::relay_config::default_codex_home_dir(), expected);
            assert_eq!(crate::codex_sqlite::default_codex_home_dir(), expected);
        }

        {
            let _guard = CodexHomeEnvGuard::set(&missing);
            assert_eq!(default_codex_home_dir(), expected);
            assert_eq!(crate::relay_config::default_codex_home_dir(), expected);
            assert_eq!(crate::codex_sqlite::default_codex_home_dir(), expected);
        }
    }
}
