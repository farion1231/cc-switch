//! Locating Codex's per-thread state SQLite databases.
//!
//! Codex stores thread metadata in versioned `state_*.sqlite` files. Depending
//! on the Codex version and configuration, they can live in the config dir, its
//! `sqlite` child, or a directory selected by `sqlite_home` / `CODEX_SQLITE_HOME`.
//! History migration and title lookup need the same resolution, so it lives here
//! once.

use std::fs;
use std::path::{Path, PathBuf};

use toml_edit::DocumentMut;

use crate::config::get_home_dir;

/// Current fallback filename. Discovery also includes every existing
/// `state_*.sqlite`, so a Codex schema-version bump does not require a CC Switch
/// release.
pub(crate) const CODEX_STATE_DB_FILENAME: &str = "state_5.sqlite";

/// Env var that overrides the Codex SQLite state directory.
const CODEX_SQLITE_HOME_ENV: &str = "CODEX_SQLITE_HOME";

/// Resolve every candidate versioned state database path.
///
/// `config_dir` is the Codex config dir (`~/.codex`); `config_text` is the raw
/// `config.toml` contents, used to detect a `sqlite_home` override.
pub(crate) fn codex_state_db_paths(config_dir: &Path, config_text: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    push_unique_path(&mut paths, config_dir.join(CODEX_STATE_DB_FILENAME));
    collect_state_databases(config_dir, &mut paths);
    collect_state_databases(&config_dir.join("sqlite"), &mut paths);

    // Codex lets SQLite state move away from CODEX_HOME; config takes precedence.
    if let Some(sqlite_home) = sqlite_home_from_codex_config(config_text) {
        push_unique_path(&mut paths, sqlite_home.join(CODEX_STATE_DB_FILENAME));
        collect_state_databases(&sqlite_home, &mut paths);
    } else if let Some(sqlite_home) = sqlite_home_from_env() {
        push_unique_path(&mut paths, sqlite_home.join(CODEX_STATE_DB_FILENAME));
        collect_state_databases(&sqlite_home, &mut paths);
    }
    paths
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    let path = fs::canonicalize(&path).unwrap_or(path);
    if !paths.contains(&path) {
        paths.push(path);
    }
}

fn collect_state_databases(dir: &Path, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut candidates: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("state_") && name.ends_with(".sqlite"))
        })
        .collect();
    candidates.sort();
    for path in candidates {
        push_unique_path(paths, path);
    }
}

fn sqlite_home_from_codex_config(config_text: &str) -> Option<PathBuf> {
    let doc = config_text.parse::<DocumentMut>().ok()?;
    let raw = doc.get("sqlite_home")?.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    Some(resolve_user_path(raw))
}

fn sqlite_home_from_env() -> Option<PathBuf> {
    let raw = std::env::var(CODEX_SQLITE_HOME_ENV).ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    Some(resolve_user_path(raw))
}

fn resolve_user_path(raw: &str) -> PathBuf {
    if raw == "~" {
        return get_home_dir();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return get_home_dir().join(rest);
    }
    if let Some(rest) = raw.strip_prefix("~\\") {
        return get_home_dir().join(rest);
    }
    PathBuf::from(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create state DB parent");
        }
        fs::File::create(path).expect("create state DB");
    }

    #[test]
    fn discovers_versioned_root_and_nested_state_databases() {
        let temp = tempdir().expect("tempdir");
        touch(&temp.path().join("state_4.sqlite"));
        touch(&temp.path().join("state_5.sqlite"));
        touch(&temp.path().join("sqlite").join("state_6.sqlite"));

        let paths = codex_state_db_paths(temp.path(), "");

        assert_eq!(paths.len(), 3, "unexpected paths: {paths:?}");
        assert!(paths.iter().any(|path| path.ends_with("state_4.sqlite")));
        assert!(paths.iter().any(|path| path.ends_with("state_5.sqlite")));
        assert!(paths.iter().any(|path| path.ends_with("state_6.sqlite")));
    }

    #[test]
    fn includes_config_sqlite_home() {
        let temp = tempdir().expect("tempdir");
        let sqlite_home = temp.path().join("sqlite-home");
        // 用 TOML 字面量字符串(单引号)承载路径：Windows 路径含反斜杠，basic string(双引号)
        // 会把 `\U`/`\s` 等当作非法转义导致解析失败。
        let config_text = format!("sqlite_home = '{}'\n", sqlite_home.display());

        let paths = codex_state_db_paths(temp.path(), &config_text);

        assert_eq!(
            paths,
            vec![
                temp.path().join(CODEX_STATE_DB_FILENAME),
                sqlite_home.join(CODEX_STATE_DB_FILENAME),
            ]
        );
    }
}
