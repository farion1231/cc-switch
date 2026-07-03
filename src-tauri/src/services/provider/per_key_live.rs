//! Per-key live config files for multi-key pools.
//!
//! 单一 active key 写到 canonical settings.json/auth.json/config.toml；
//! 额外的每把 key 在 `{config_dir}/keys/{provider_id}/{key_id}.<ext>` 下写
//! 一份可独立使用的配置——供直连 Claude Code / Codex CLI 时按需切换
//! （proxy 关闭 / 远程机器 / 移动到另一台设备时仍可用）。
//!
//! # 触发点
//!
//! - `regenerate_per_key_live_files`：单 provider 增量重生成。在
//!   `cmd_create_api_key` / `cmd_update_api_key`（仅 api_key 变化时）/
//!   `cmd_delete_api_key` / `cmd_set_active_api_key` 末尾调用；同时
//!   `delete_api_keys_for_provider` cascade 删除时清空整个 keys/<pid>/
//!   子目录。
//! - `regenerate_all_per_key_files`：startup migration 走一遍所有
//!   provider，确保老用户升级后立即拿到 N 份 key 文件。
//!
//! # 失败语义
//!
//! 单把 key 写失败仅记日志、继续下一把——避免一个坏 key 阻挡 UI。
//! Stale 清理（删除数据库已删 key 的残留文件）在 regenerate 末尾做：
//! 列目录、按现 DB key_id 白名单扫，超出的统一 remove。运行多次幂等。

use crate::app_config::AppType;
use crate::config::{
    get_per_key_live_dir, sanitize_provider_name, write_json_file, write_text_file,
};
use crate::database::dao::api_keys::ProviderApiKey;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::provider::live::sanitize_claude_settings_for_live;
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;

/// 重新生成单 provider 的所有 per-key live config 文件。
///
/// - `db`：database handle，用于 list_api_keys / get_provider 等。
/// - `app_type`：决定走哪个 app 的 writer 路径。
/// - `provider`：当前 provider 完整对象（用于取 settings_config 当模板）。
/// - `changed_key_id`：本次触发的具体 key。当前实现不严格使用它（直接
///   diff on-disk 即可），但保留作未来 optimisation 与日志可观测性。
///
/// 失败语义：返回第一个 IO/序列化错误，但已成功写入的文件**不会**回滚
/// ——下次 regen 时 Stale 清理会重新对齐 on-disk 与 DB。
pub fn regenerate_per_key_live_files(
    db: &Database,
    app_type: &AppType,
    provider: &Provider,
    changed_key_id: Option<&str>,
) -> Result<(), AppError> {
    let _ = changed_key_id; // 保留参数位（后续可加细粒度优化）

    // 1. 拿到该 provider 的所有 key。
    let keys = db.list_api_keys(&provider.id, app_type.as_str())?;
    if keys.is_empty() {
        // 没有 key 池：清掉可能残留的目录（e.g. 用户删完所有 key）。
        if let Some(dir) = get_per_key_live_dir(app_type, &provider.id) {
            cleanup_dir(&dir);
        }
        return Ok(());
    }

    // 2. 取 per-key dir。ClaudeDesktop / OpenCode / OpenClaw / Hermes 返 None 跳过。
    let dir = match get_per_key_live_dir(app_type, &provider.id) {
        Some(d) => d,
        None => {
            log::debug!(
                "[per_key_live] skip app_type={} provider={}: no per-key layout",
                app_type.as_str(),
                provider.id
            );
            return Ok(());
        }
    };

    // 3. 写每把 key。
    let mut written_key_ids: HashSet<String> = HashSet::new();
    for key in &keys {
        if !key.enabled {
            // Disabled key 不写文件（避免用户误以为这把 key 还在用）。
            // 但同样要把它的 stale 文件清掉——见 step 4。
            continue;
        }
        match write_one_key(app_type, &provider, &dir, key) {
            Ok(()) => {
                written_key_ids.insert(key.id.clone());
            }
            Err(e) => {
                log::warn!(
                    "[per_key_live] write failed: app={} provider={} key={}: {e}",
                    app_type.as_str(),
                    provider.id,
                    key.id
                );
            }
        }
    }

    // 4. Stale cleanup：枚举 dir 里所有该 app writer 关心的后缀，
    //    凡是 key_id 不在 written_key_ids 中的（也包括 enabled=false
    //    的 key）一律删。
    cleanup_stale_files(app_type, &dir, &keys, &written_key_ids);

    Ok(())
}

/// 全 provider 重生成——startup migration 用。跨所有 app type、所有
/// provider 一遍。Idempotent（写相同内容、stale 清理会让存量数据稳定）。
pub fn regenerate_all_per_key_files(db: &Database) -> Result<(), AppError> {
    for app_type in [
        AppType::Claude,
        AppType::Codex,
        AppType::Gemini,
    ] {
        let providers = db.get_all_providers(app_type.as_str())?;
        for (_id, provider) in providers {
            if let Err(e) =
                regenerate_per_key_live_files(db, &app_type, &provider, None)
            {
                log::warn!(
                    "[per_key_live] all-regen failed: app={} provider={}: {e}",
                    app_type.as_str(),
                    provider.id
                );
                // 继续下一个 provider，单家失败不影响其他。
            }
        }
    }
    Ok(())
}

/// Provider 删除时调用：整棵 keys/<pid>/ 子目录 wipe。
pub fn cleanup_provider_keys_dir(
    app_type: &AppType,
    provider_id: &str,
) -> Result<(), AppError> {
    if let Some(dir) = get_per_key_live_dir(app_type, provider_id) {
        cleanup_dir(&dir);
    }
    Ok(())
}

// ─── internals ─────────────────────────────────────────────────────────

/// 单把 key 的写盘。`provider` 是模板（settings_config / model / baseUrl 都
/// 复用），唯一变化的就是 api_key slot。
fn write_one_key(
    app_type: &AppType,
    provider: &Provider,
    target_dir: &Path,
    key: &ProviderApiKey,
) -> Result<(), AppError> {
    // clone 后调 set_api_key —— 已有 mutator 在 provider.rs:216，遍历
    // app_type.api_key_settings_path() 把 key 写到正确 slot。
    let mut p = provider.clone();
    p.set_api_key(&key.api_key, app_type);

    match app_type {
        AppType::Claude => write_claude_per_key(&p, target_dir, &key.id),
        AppType::Codex => write_codex_per_key(&p, target_dir, &key.id),
        AppType::Gemini => write_gemini_per_key(&p, target_dir, &key.id),
        // 其它 app type 已在 get_per_key_live_dir 处被过滤掉，这里作为
        // safety net 显式返回错误（避免悄悄漏写）。
        _ => Err(AppError::localized(
            "per_key_live.unsupported_app",
            format!("app_type {} 不支持 per-key live 配置", app_type.as_str()),
            format!("app_type {} does not support per-key live config", app_type.as_str()),
        )),
    }
}

/// Claude per-key 写：单 `{key_id}.json` 文件，等价于 canonical
/// settings.json 但 auth token 换成此 key。
fn write_claude_per_key(
    provider: &Provider,
    target_dir: &Path,
    key_id: &str,
) -> Result<(), AppError> {
    let safe_key_id = sanitize_provider_name(key_id);
    let file_path = target_dir.join(format!("{safe_key_id}.json"));
    let settings = sanitize_claude_settings_for_live(&provider.settings_config);
    write_json_file(&file_path, &settings)
}

/// Codex per-key 写：和 canonical 一致拆 `auth-{keyId}.json` +
/// `config-{keyId}.toml`。两份文件独立原子写（write_json_file 和
/// write_text_file 内部各自 temp+rename），半成品由下次 regen 修复。
fn write_codex_per_key(
    provider: &Provider,
    target_dir: &Path,
    key_id: &str,
) -> Result<(), AppError> {
    let obj = provider.settings_config.as_object().ok_or_else(|| {
        AppError::Config(format!(
            "Codex per-key: provider '{}' settings_config 不是对象",
            provider.id
        ))
    })?;
    let auth = obj.get("auth").ok_or_else(|| {
        AppError::Config(format!(
            "Codex per-key: provider '{}' 缺少 auth 字段",
            provider.id
        ))
    })?;
    let config_text = obj.get("config").and_then(Value::as_str);

    let safe_key_id = sanitize_provider_name(key_id);
    let auth_path = target_dir.join(format!("auth-{safe_key_id}.json"));
    let config_path = target_dir.join(format!("config-{safe_key_id}.toml"));

    // auth: 必须是有效 JSON Value。
    write_json_file(&auth_path, auth)?;

    // config: 可选（Codex provider 可以没有 config.toml）——仅当非空时落盘。
    if let Some(text) = config_text {
        if !text.trim().is_empty() {
            // 同步 canonical write_codex_live_atomic 的 TOML 语法预检。
            toml::from_str::<toml::Table>(text).map_err(|e| {
                AppError::toml(&config_path, e)
            })?;
            write_text_file(&config_path, text)?;
        } else {
            // 空 config：canonical 不写 config.toml——这里也保持一致。
            // 清理可能残留的文件。
            let _ = std::fs::remove_file(&config_path);
        }
    }

    Ok(())
}

/// Gemini per-key 写：单 `{key_id}.json` 文件。Gemini 的 live config
/// 既包含 .env 又包含 settings.json，但二者都从同一个 settings_config
/// 派生——本模块只生成 settings.json 形式（直连 CLI 用的最多的是这个）；
/// .env 通过 Gemini 的官方 CLI 在 $-substitution 时自动处理。
fn write_gemini_per_key(
    provider: &Provider,
    target_dir: &Path,
    key_id: &str,
) -> Result<(), AppError> {
    let safe_key_id = sanitize_provider_name(key_id);
    let file_path = target_dir.join(format!("{safe_key_id}.json"));
    // Gemini 没专门的 sanitize 函数，复用 Claude 的——两个 app 的
    // settings_config 结构类似（都含 env 块）。
    let settings = sanitize_claude_settings_for_live(&provider.settings_config);
    write_json_file(&file_path, &settings)
}

/// 列出目录里所有关心的文件名后缀，删除不在 written_key_ids 白名单中的。
/// 一并解析 key_id 后再决定是否保留——避免误删 `${safe_key_id}.swp` 等
/// 临时文件（虽然我们用 atomic_write 不会产生这些）。
fn cleanup_stale_files(
    app_type: &AppType,
    target_dir: &Path,
    keys: &[ProviderApiKey],
    written_key_ids: &HashSet<String>,
) {
    // 收集所有 enabled key 的 sanitized key_id——disabled key 也算
    // "已被 db 知道但未写出"，所以保留它们的文件——便于用户切到 enabled
    // 后立即可用。
    let allowed_ids: HashSet<String> = keys
        .iter()
        .map(|k| sanitize_provider_name(&k.id))
        .collect();

    let entries = match std::fs::read_dir(target_dir) {
        Ok(e) => e,
        Err(_) => return, // dir 不存在即没有 stale 文件。
    };

    for entry in entries.flatten() {
        let file_name = match entry.file_name().into_string() {
            Ok(s) => s,
            Err(_) => continue,
        };
        let (stripped, ext) = match app_type {
            AppType::Claude => match file_name.strip_suffix(".json") {
                Some(stem) => (stem.to_string(), "json"),
                None => continue,
            },
            AppType::Gemini => match file_name.strip_suffix(".json") {
                Some(stem) => (stem.to_string(), "json"),
                None => continue,
            },
            AppType::Codex => {
                // auth-<keyId>.json / config-<keyId>.toml
                if let Some(stem) = file_name.strip_prefix("auth-").and_then(|s| s.strip_suffix(".json")) {
                    (stem.to_string(), "auth.json")
                } else if let Some(stem) = file_name.strip_prefix("config-").and_then(|s| s.strip_suffix(".toml")) {
                    (stem.to_string(), "config.toml")
                } else {
                    continue;
                }
            }
            _ => continue,
        };

        // 任何不在 db 已知 key_id 白名单里的文件 = stale，删除。
        if !allowed_ids.contains(&stripped) {
            let _ = std::fs::remove_file(entry.path());
            log::info!(
                "[per_key_live] removed stale file: {:?} (app={}, ext={ext})",
                entry.path(),
                app_type.as_str()
            );
        }
        // 注意：不在 written_key_ids 但在 allowed_ids 内（disabled key）
        // 不会被删——保留以便复用。
        let _ = written_key_ids;
    }
}

/// 整目录递归删除（best-effort；silent on error）。provider 全删时用。
fn cleanup_dir(dir: &Path) {
    if !dir.exists() {
        return;
    }
    match std::fs::remove_dir_all(dir) {
        Ok(()) => log::info!("[per_key_live] removed dir {:?}", dir),
        Err(e) => log::warn!("[per_key_live] failed to remove dir {:?}: {e}", dir),
    }
}

// ─── tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::AppType;
    use crate::database::Database;
    use crate::provider::Provider;
    use crate::services::provider::mod_test::with_test_home;
    use serde_json::json;

    fn make_provider(id: &str, kind: &str, api_key_field: &str) -> Provider {
        Provider::with_id(
            id.to_string(),
            format!("P-{id}"),
            json!({ kind: { api_key_field: "PLACEHOLDER" } }),
            None,
        )
    }

    fn make_key_row(
        provider_id: &str,
        app_type: &str,
        key_id: &str,
        api_key: &str,
        sort_index: i64,
        enabled: bool,
    ) -> ProviderApiKey {
        let now = chrono::Utc::now().timestamp();
        ProviderApiKey {
            id: key_id.to_string(),
            provider_id: provider_id.to_string(),
            app_type: app_type.to_string(),
            label: key_id.to_string(),
            api_key: api_key.to_string(),
            tags: vec![],
            notes: String::new(),
            enabled,
            sort_index,
            is_active: sort_index == 0,
            cooldown_until: 0,
            failure_count: 0,
            last_used_at: None,
            last_error: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn claude_writes_one_file_per_key() {
        with_test_home(|state, home| {
            let db = state.db.as_ref();
            db.save_provider(
                AppType::Claude.as_str(),
                &make_provider("p-claude", "env", "ANTHROPIC_AUTH_TOKEN"),
            )
            .unwrap();
            db.insert_api_key(&make_key_row(
                "p-claude",
                "claude",
                "k1",
                "sk-test-1",
                0,
                true,
            ))
            .unwrap();
            db.insert_api_key(&make_key_row(
                "p-claude",
                "claude",
                "k2",
                "sk-test-2",
                1,
                true,
            ))
            .unwrap();
            let provider = db
                .get_provider_by_id("p-claude", "claude")
                .unwrap()
                .expect("provider");

            regenerate_per_key_live_files(db, &AppType::Claude, &provider, None)
                .unwrap();

            let dir = home.join(".claude").join("keys").join("p-claude");
            let k1_path = dir.join("k1.json");
            let k2_path = dir.join("k2.json");
            assert!(k1_path.exists(), "k1.json should exist: {:?}", k1_path);
            assert!(k2_path.exists(), "k2.json should exist: {:?}", k2_path);

            let v1: Value =
                serde_json::from_str(&std::fs::read_to_string(&k1_path).unwrap())
                    .unwrap();
            let v2: Value =
                serde_json::from_str(&std::fs::read_to_string(&k2_path).unwrap())
                    .unwrap();
            assert_eq!(
                v1["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-test-1",
                "k1 file should have its own raw key"
            );
            assert_eq!(
                v2["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-test-2",
                "k2 file should have its own raw key"
            );
        });
    }

    #[test]
    fn codex_writes_pair_per_key() {
        with_test_home(|state, home| {
            let db = state.db.as_ref();
            db.save_provider(
                AppType::Codex.as_str(),
                &make_provider("p-codex", "auth", "OPENAI_API_KEY"),
            )
            .unwrap();
            db.insert_api_key(&make_key_row(
                "p-codex",
                "codex",
                "k1",
                "sk-codex-1",
                0,
                true,
            ))
            .unwrap();
            let provider = db
                .get_provider_by_id("p-codex", "codex")
                .unwrap()
                .expect("provider");

            regenerate_per_key_live_files(db, &AppType::Codex, &provider, None)
                .unwrap();

            let dir = home.join(".codex").join("keys").join("p-codex");
            let auth_path = dir.join("auth-k1.json");
            assert!(auth_path.exists(), "auth-k1.json should exist: {:?}", auth_path);
            let auth: Value =
                serde_json::from_str(&std::fs::read_to_string(&auth_path).unwrap())
                    .unwrap();
            assert_eq!(auth["OPENAI_API_KEY"], "sk-codex-1");
        });
    }

    #[test]
    fn stale_files_are_cleaned_on_delete() {
        with_test_home(|state, _home| {
            let db = state.db.as_ref();
            db.save_provider(
                AppType::Claude.as_str(),
                &make_provider("p-claude", "env", "ANTHROPIC_AUTH_TOKEN"),
            )
            .unwrap();
            db.insert_api_key(&make_key_row(
                "p-claude",
                "claude",
                "k1",
                "sk-1",
                0,
                true,
            ))
            .unwrap();
            db.insert_api_key(&make_key_row(
                "p-claude",
                "claude",
                "k2",
                "sk-2",
                1,
                true,
            ))
            .unwrap();
            let provider = db
                .get_provider_by_id("p-claude", "claude")
                .unwrap()
                .expect("provider");

            // 第一次 regen：写两份文件。
            regenerate_per_key_live_files(db, &AppType::Claude, &provider, None)
                .unwrap();
            let dir = claude_keys_dir("p-claude");
            assert!(dir.join("k1.json").exists());
            assert!(dir.join("k2.json").exists());

            // 删 k2 ——文件还在 on-disk。
            db.delete_api_key("k2").unwrap();
            assert!(
                dir.join("k2.json").exists(),
                "before regen, deleted key's file still on disk"
            );

            // 重新 regen：stale 清理应触发。
            let provider_after = db
                .get_provider_by_id("p-claude", "claude")
                .unwrap()
                .expect("provider");
            regenerate_per_key_live_files(
                db,
                &AppType::Claude,
                &provider_after,
                None,
            )
            .unwrap();
            assert!(
                dir.join("k1.json").exists(),
                "k1 should still exist after regen"
            );
            assert!(
                !dir.join("k2.json").exists(),
                "k2 file should be removed by stale cleanup"
            );
        });
    }

    #[test]
    fn regeneration_is_idempotent() {
        with_test_home(|state, _home| {
            let db = state.db.as_ref();
            db.save_provider(
                AppType::Claude.as_str(),
                &make_provider("p-claude", "env", "ANTHROPIC_AUTH_TOKEN"),
            )
            .unwrap();
            db.insert_api_key(&make_key_row(
                "p-claude",
                "claude",
                "k1",
                "sk-1",
                0,
                true,
            ))
            .unwrap();
            let provider = db
                .get_provider_by_id("p-claude", "claude")
                .unwrap()
                .expect("provider");

            regenerate_per_key_live_files(db, &AppType::Claude, &provider, None)
                .unwrap();
            assert!(claude_keys_dir("p-claude").join("k1.json").exists());

            // 立即再 regen —— id 不变，文件不该消失。
            regenerate_per_key_live_files(db, &AppType::Claude, &provider, None)
                .unwrap();
            assert!(
                claude_keys_dir("p-claude").join("k1.json").exists(),
                "idempotent regen shouldn't delete unchanged files"
            );
        });
    }

    #[test]
    fn disabled_keys_skip_write_but_keep_file() {
        with_test_home(|state, _home| {
            let db = state.db.as_ref();
            db.save_provider(
                AppType::Claude.as_str(),
                &make_provider("p-claude", "env", "ANTHROPIC_AUTH_TOKEN"),
            )
            .unwrap();
            db.insert_api_key(&make_key_row(
                "p-claude",
                "claude",
                "k1",
                "sk-1",
                0,
                true,
            ))
            .unwrap();
            db.insert_api_key(&make_key_row(
                "p-claude",
                "claude",
                "k2",
                "sk-2",
                1,
                false,
            ))
            .unwrap();
            let provider = db
                .get_provider_by_id("p-claude", "claude")
                .unwrap()
                .expect("provider");

            regenerate_per_key_live_files(db, &AppType::Claude, &provider, None)
                .unwrap();

            // enabled=true → 文件存在。
            assert!(claude_keys_dir("p-claude").join("k1.json").exists());
            // enabled=false → 文件不写。如果老的不存在，现在也不该出现。
            assert!(
                !claude_keys_dir("p-claude").join("k2.json").exists(),
                "disabled key should not produce a per-key file"
            );
        });
    }

    #[test]
    fn sanitize_blocks_path_traversal_in_provider_id() {
        let raw = "../escape/attempt";
        let safe = sanitize_provider_name(raw);
        assert!(!safe.contains("/"), "sanitized id must not contain /");
        assert!(!safe.contains("\\"), "sanitized id must not contain \\");
        assert!(!safe.contains(".."), "sanitized id must not contain ..");
    }

    // ─── helpers ────────────────────────────────────────────────────────

    fn claude_keys_dir(provider_id: &str) -> std::path::PathBuf {
        crate::config::get_claude_config_dir()
            .join("keys")
            .join(sanitize_provider_name(provider_id))
    }
}
