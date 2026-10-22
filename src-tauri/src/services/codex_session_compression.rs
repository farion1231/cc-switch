//! Codex 会话压缩开关：`config.toml` 的 `[features] local_thread_store_compression`。
//!
//! 开启后 Codex 每次启动在后台把 7 天没动过的 rollout 压成 `.jsonl.zst`
//! （见 `codex_rollout_file`），CC Switch 的会话管理与用量统计照常读取。
//! 开关状态以 `config.toml` 为准，不另存设置；写入经写入引擎（首写备份、崩溃恢复）。

use toml_edit::value;

use crate::app_config::AppType;
use crate::codex_config::{
    codex_config_feature_enabled, get_codex_config_path, read_codex_config_text,
};
use crate::database::Database;
use crate::error::AppError;
use crate::live::engine::LiveFile;
use crate::live::patch::toml::TomlPatch;
use crate::live::patch::KeyPath;
use crate::mode::operation::{AppWrite, FileChange};
use crate::mode::state::{op, PendingTarget};
use crate::session_manager::providers::codex::session_roots;

const FEATURE_KEY: &str = "local_thread_store_compression";

fn feature_path() -> KeyPath {
    KeyPath::new(&["features", FEATURE_KEY])
}

pub fn is_enabled() -> Result<bool, AppError> {
    Ok(codex_config_feature_enabled(
        &read_codex_config_text()?,
        FEATURE_KEY,
    ))
}

/// 开启写 `true`；关闭删掉这个键（回到 Codex 默认的关闭，也覆盖 `{ enabled = true }` 写法）。
/// 已经是目标状态时不写文件。返回写入后的状态。
pub fn set_enabled(db: &Database, enabled: bool) -> Result<bool, AppError> {
    if is_enabled()? == enabled {
        return Ok(enabled);
    }
    AppWrite::begin(db, AppType::Codex.as_str())?.run(
        op::APPLY,
        &[FileChange {
            file: LiveFile::private(get_codex_config_path()),
            patch: &patch(enabled),
        }],
        PendingTarget::default(),
    )?;
    is_enabled()
}

fn patch(enabled: bool) -> TomlPatch {
    if enabled {
        TomlPatch {
            set: vec![(feature_path(), value(true))],
            ..TomlPatch::default()
        }
    } else {
        TomlPatch {
            remove: vec![feature_path()],
            ..TomlPatch::default()
        }
    }
}

/// Codex 会话目录（`sessions/` + `archived_sessions/`）当前占用的字节数。
pub fn sessions_disk_usage() -> u64 {
    session_roots()
        .iter()
        .map(|root| crate::services::backup_storage::path_size(root))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::patch::LivePatch;
    use std::path::Path;

    fn apply(enabled: bool, before: &str) -> String {
        let bytes = patch(enabled)
            .apply(Path::new("config.toml"), Some(before.as_bytes()))
            .expect("apply patch");
        String::from_utf8(bytes).expect("utf8")
    }

    #[test]
    fn enabling_adds_key_and_keeps_layout() {
        let before = "# my config\nmodel = \"gpt-5\"\n\n[features]\nweb_search = true # keep\n";
        let after = apply(true, before);
        assert!(after.starts_with(
            "# my config\nmodel = \"gpt-5\"\n\n[features]\nweb_search = true # keep\n"
        ));
        assert!(codex_config_feature_enabled(&after, FEATURE_KEY));
        assert!(codex_config_feature_enabled(&after, "web_search"));
    }

    #[test]
    fn enabling_creates_features_table_in_empty_file() {
        let bytes = patch(true)
            .apply(Path::new("config.toml"), None)
            .expect("apply patch");
        let after = String::from_utf8(bytes).expect("utf8");
        assert!(codex_config_feature_enabled(&after, FEATURE_KEY));
    }

    #[test]
    fn disabling_removes_both_forms_and_keeps_other_features() {
        for before in [
            "[features]\nweb_search = true\nlocal_thread_store_compression = true\n",
            "[features]\nweb_search = true\nlocal_thread_store_compression = { enabled = true }\n",
        ] {
            let after = apply(false, before);
            assert!(
                !codex_config_feature_enabled(&after, FEATURE_KEY),
                "{after}"
            );
            assert!(!after.contains(FEATURE_KEY), "{after}");
            assert!(codex_config_feature_enabled(&after, "web_search"));
        }
    }

    #[test]
    fn table_form_counts_as_enabled() {
        assert!(codex_config_feature_enabled(
            "[features]\nlocal_thread_store_compression = { enabled = true }\n",
            FEATURE_KEY
        ));
        assert!(!codex_config_feature_enabled(
            "[features]\nlocal_thread_store_compression = false\n",
            FEATURE_KEY
        ));
    }
}
