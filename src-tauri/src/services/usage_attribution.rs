//! 用量多维归属：供应商激活时间线、会话项目索引与项目键规范化。
//!
//! 归属证据与统计结果分开存储：这里只负责写入/查询证据，聚合查询在
//! `usage_stats`。切换式客户端的供应商归属以激活时间线为准，路由模式以代理
//! 日志为准，累加式客户端优先使用原生供应商字段。

use crate::app_config::AppType;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::{params, Connection, OptionalExtension};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use super::sql_helpers::{fresh_input_sql, INPUT_TOKEN_SEMANTICS_FRESH};

/// 供应商激活记录（切换式客户端的直连归属证据）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderActivation {
    pub id: i64,
    pub app_type: String,
    pub provider_id: Option<String>,
    pub mode: String,
    pub effective_at_ms: i64,
    pub source: String,
    pub config_fingerprint: String,
}

/// 会话项目索引行：一个会话对应一个主项目。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionProject {
    pub app_type: String,
    pub session_key: String,
    pub project_key: String,
    pub project_path: String,
    pub project_name: String,
    pub source: String,
}

/// 一次归属决策的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribution {
    pub provider_id: String,
    pub source: String,
    pub confidence: String,
    pub reason: String,
}

impl Attribution {
    pub fn unknown(placeholder: &str, reason: impl Into<String>) -> Self {
        Self {
            provider_id: placeholder.to_string(),
            source: "unknown".to_string(),
            confidence: "unknown".to_string(),
            reason: reason.into(),
        }
    }

    pub fn exact(provider_id: impl Into<String>, source: &str) -> Self {
        Self {
            provider_id: provider_id.into(),
            source: source.to_string(),
            confidence: "exact".to_string(),
            reason: String::new(),
        }
    }
}

/// 规范化代理/会话 ID，保证代理行与会话行能对上同一个 session_key。
pub fn normalize_session_key(app_type: &str, raw: &str) -> String {
    let mut key = raw.trim().to_string();
    if key.is_empty() {
        return key;
    }
    // Codex 客户端有时带 `codex_` 前缀，代理侧与会话侧需要落到同一个键。
    if app_type == "codex" {
        if let Some(stripped) = key.strip_prefix("codex_") {
            key = stripped.to_string();
        }
    }
    // Claude 的 metadata.user_id 可能包成 `user_xxx_session_yyy`。
    if app_type == "claude" || app_type == "claude-desktop" {
        if let Some(idx) = key.find("_session_") {
            key = key[idx + "_session_".len()..].to_string();
        }
    }
    key
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn normalize_path_string(raw: &str) -> String {
    let mut path = raw.trim().to_string();
    if let Some(rest) = path.strip_prefix("file://") {
        path = percent_decode(rest);
    }
    path = path.replace('\\', "/");
    while path.len() > 1 && path.ends_with('/') {
        path.pop();
    }
    path
}

fn find_git_root(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .map(|ancestor| ancestor.to_path_buf())
}

/// Linked worktrees share a common Git directory. Keep submodules and unknown
/// layouts separate when that metadata cannot establish a common repository.
fn repository_project_root(root: &Path) -> PathBuf {
    let shared_root = (|| {
        let marker = std::fs::read_to_string(root.join(".git")).ok()?;
        let git_dir = root.join(marker.trim().strip_prefix("gitdir:")?.trim());
        let common = std::fs::read_to_string(git_dir.join("commondir")).ok()?;
        let common = strip_windows_verbatim_prefix(
            &std::fs::canonicalize(git_dir.join(common.trim())).ok()?,
        );
        if common.file_name()? != ".git" {
            return None;
        }
        Some(common.parent()?.to_path_buf())
    })();
    shared_root.unwrap_or_else(|| root.to_path_buf())
}

fn normalize_pathbuf(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('\\', "/");
    while value.len() > 1 && value.ends_with('/') {
        value.pop();
    }
    value
}

/// Windows `canonicalize` 会给本机盘符返回 `\\?\C:\...`；去掉这个 verbatim 前缀，
/// 保证存在路径与缺失路径回退到同一个项目键。UNC 前缀保持不变。
fn strip_windows_verbatim_prefix(path: &Path) -> PathBuf {
    let value = normalize_pathbuf(path);
    let Some(rest) = value.strip_prefix("//?/") else {
        return path.to_path_buf();
    };
    let bytes = rest.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/' {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

/// 从客户端 cwd 构造项目行。路径不存在时保留规范化字符串，不丢会话。
pub fn project_from_cwd(
    app_type: &str,
    session_key: &str,
    raw_cwd: &str,
    source: &str,
) -> Option<SessionProject> {
    let normalized = normalize_path_string(raw_cwd);
    if normalized.is_empty() {
        return None;
    }
    let path = Path::new(&normalized);
    let resolved = strip_windows_verbatim_prefix(
        &std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(&normalized)),
    );
    let root = find_git_root(&resolved)
        .map(|root| repository_project_root(&root))
        .unwrap_or_else(|| resolved.clone());
    let project_path = normalize_pathbuf(&resolved);
    let project_key = if project_path.is_empty() {
        "/".to_string()
    } else {
        normalize_pathbuf(&root)
    };
    // 项目名取 cwd 的最后一级目录名（比仓库根目录更贴近用户直觉）。
    let project_name = resolved
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| project_key.clone());

    Some(SessionProject {
        app_type: app_type.to_string(),
        session_key: session_key.to_string(),
        project_key,
        project_path,
        project_name,
        source: source.to_string(),
    })
}

/// 供应商配置指纹：只保存哈希，不落 Key/Token。
pub fn provider_config_fingerprint(settings_config: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"cc-switch-provider-config-v1");
    hasher.update(settings_config.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl Database {
    /// Repair retained project identities using current Git metadata, without
    /// inferring relationships from directory names or changing provider evidence.
    pub fn normalize_worktree_projects(&self) -> Result<usize, AppError> {
        let keys = {
            let conn = lock_conn!(self.conn);
            let mut stmt = conn.prepare(
                "SELECT project_key FROM session_projects WHERE project_key != ''
                 UNION SELECT project_key FROM proxy_request_logs WHERE project_key != ''
                 UNION SELECT project_key FROM usage_daily_rollups WHERE project_key != ''",
            )?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let aliases: Vec<_> = keys
            .into_iter()
            .filter_map(|key| {
                let path = strip_windows_verbatim_prefix(&std::fs::canonicalize(&key).ok()?);
                let root = find_git_root(&path)?;
                let canonical = normalize_pathbuf(&repository_project_root(&root));
                (canonical != key).then_some((key, canonical))
            })
            .collect();
        if aliases.is_empty() {
            return Ok(0);
        }
        let conn = lock_conn!(self.conn);
        conn.execute_batch("SAVEPOINT normalize_worktree_projects")?;
        let result = (|| {
            for (alias, canonical) in &aliases {
                Self::merge_project_rollups(&conn, alias, canonical)?;
                for table in ["session_projects", "proxy_request_logs"] {
                    conn.execute(
                        &format!("UPDATE {table} SET project_key=?1 WHERE project_key=?2"),
                        params![canonical, alias],
                    )?;
                }
            }
            Ok(aliases.len())
        })();
        match result {
            Ok(count) => {
                conn.execute_batch("RELEASE normalize_worktree_projects")?;
                Ok(count)
            }
            Err(error) => {
                let _ = conn.execute_batch(
                    "ROLLBACK TO normalize_worktree_projects; RELEASE normalize_worktree_projects",
                );
                Err(error)
            }
        }
    }

    fn merge_project_rollups(
        conn: &Connection,
        alias: &str,
        canonical: &str,
    ) -> Result<(), AppError> {
        let costs = {
            let mut stmt = conn.prepare(
                "SELECT r.rowid, r.total_cost_usd, target.total_cost_usd
                 FROM usage_daily_rollups r
                 LEFT JOIN usage_daily_rollups target
                   ON target.project_key=?1 AND target.date=r.date
                  AND target.app_type=r.app_type AND target.provider_id=r.provider_id
                  AND target.model=r.model AND target.request_model=r.request_model
                  AND target.pricing_model=r.pricing_model AND target.data_source=r.data_source
                 WHERE r.project_key=?2 ORDER BY r.rowid",
            )?;
            let rows = stmt.query_map(params![canonical, alias], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let source_fresh = fresh_input_sql("r");
        let target_fresh = fresh_input_sql("usage_daily_rollups");
        let sql = format!(
            "INSERT INTO usage_daily_rollups
                (date, app_type, provider_id, model, request_model, pricing_model, data_source,
                 project_key, project_name, project_path, provider_name_snapshot,
                 request_count, success_count, input_tokens, output_tokens,
                 cache_read_tokens, cache_creation_tokens, input_token_semantics,
                 total_cost_usd, avg_latency_ms)
             SELECT date, app_type, provider_id, model, request_model, pricing_model, data_source,
                    ?1, project_name, project_path, provider_name_snapshot,
                    request_count, success_count, {source_fresh}, output_tokens,
                    cache_read_tokens, cache_creation_tokens, {INPUT_TOKEN_SEMANTICS_FRESH},
                    ?2, avg_latency_ms
             FROM usage_daily_rollups r WHERE r.rowid=?3
             ON CONFLICT(date, app_type, provider_id, model, request_model, pricing_model, data_source, project_key)
             DO UPDATE SET
                avg_latency_ms = CASE WHEN usage_daily_rollups.request_count + excluded.request_count > 0
                    THEN (usage_daily_rollups.avg_latency_ms * usage_daily_rollups.request_count
                          + excluded.avg_latency_ms * excluded.request_count)
                         / (usage_daily_rollups.request_count + excluded.request_count) ELSE 0 END,
                request_count = usage_daily_rollups.request_count + excluded.request_count,
                success_count = usage_daily_rollups.success_count + excluded.success_count,
                input_tokens = ({target_fresh}) + excluded.input_tokens,
                output_tokens = usage_daily_rollups.output_tokens + excluded.output_tokens,
                cache_read_tokens = usage_daily_rollups.cache_read_tokens + excluded.cache_read_tokens,
                cache_creation_tokens = usage_daily_rollups.cache_creation_tokens + excluded.cache_creation_tokens,
                input_token_semantics = {INPUT_TOKEN_SEMANTICS_FRESH},
                total_cost_usd = excluded.total_cost_usd,
                project_name = COALESCE(NULLIF(usage_daily_rollups.project_name, ''), excluded.project_name),
                project_path = COALESCE(NULLIF(usage_daily_rollups.project_path, ''), excluded.project_path),
                provider_name_snapshot = COALESCE(NULLIF(usage_daily_rollups.provider_name_snapshot, ''), excluded.provider_name_snapshot)"
        );
        for (rowid, source_cost, target_cost) in costs {
            let cost = match target_cost {
                Some(target_cost) => {
                    let parse = |value: &str| {
                        Decimal::from_str(value).map_err(|error| {
                            AppError::Database(format!("Invalid project rollup cost: {error}"))
                        })
                    };
                    parse(&source_cost)?
                        .checked_add(parse(&target_cost)?)
                        .ok_or_else(|| {
                            AppError::Database("Project rollup cost overflow".to_string())
                        })?
                        .to_string()
                }
                None => source_cost,
            };
            conn.execute(&sql, params![canonical, cost, rowid])?;
        }
        conn.execute(
            "DELETE FROM usage_daily_rollups WHERE project_key=?1",
            [alias],
        )?;
        Ok(())
    }

    /// 追加一条激活记录。连续相同的 (mode, provider_id, fingerprint) 合并，避免
    /// 启动恢复/重复提交产生噪声。返回是否真正写入。
    pub fn record_provider_activation(
        &self,
        app_type: &str,
        provider_id: Option<&str>,
        mode: &str,
        source: &str,
        config_fingerprint: &str,
    ) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        Self::record_provider_activation_on_conn(
            &conn,
            app_type,
            provider_id,
            mode,
            source,
            config_fingerprint,
        )
    }

    pub fn record_provider_activation_on_conn(
        conn: &Connection,
        app_type: &str,
        provider_id: Option<&str>,
        mode: &str,
        source: &str,
        config_fingerprint: &str,
    ) -> Result<bool, AppError> {
        let last: Option<(Option<String>, String, String)> = conn
            .query_row(
                "SELECT provider_id, mode, config_fingerprint
                 FROM provider_activations
                 WHERE app_type = ?1
                 ORDER BY effective_at_ms DESC, id DESC
                 LIMIT 1",
                params![app_type],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|e| AppError::Database(format!("读取最后一条供应商激活失败: {e}")))?;

        if let Some((last_provider, last_mode, last_fingerprint)) = last {
            if last_provider.as_deref() == provider_id
                && last_mode == mode
                && last_fingerprint == config_fingerprint
            {
                return Ok(false);
            }
        }

        let now_ms = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "INSERT INTO provider_activations
                (app_type, provider_id, mode, effective_at_ms, source, config_fingerprint, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                app_type,
                provider_id,
                mode,
                now_ms,
                source,
                config_fingerprint,
                chrono::Utc::now().timestamp()
            ],
        )
        .map_err(|e| AppError::Database(format!("写入供应商激活失败: {e}")))?;
        Ok(true)
    }

    /// 查询 `at_ms` 之前最后一条激活记录。
    pub fn last_provider_activation(
        &self,
        app_type: &str,
        at_ms: i64,
    ) -> Result<Option<ProviderActivation>, AppError> {
        let conn = lock_conn!(self.conn);
        Self::last_provider_activation_on_conn(&conn, app_type, at_ms)
    }

    pub(crate) fn last_provider_activation_on_conn(
        conn: &Connection,
        app_type: &str,
        at_ms: i64,
    ) -> Result<Option<ProviderActivation>, AppError> {
        conn.query_row(
            "SELECT id, app_type, provider_id, mode, effective_at_ms, source, config_fingerprint
             FROM provider_activations
             WHERE app_type = ?1 AND effective_at_ms <= ?2
             ORDER BY effective_at_ms DESC, id DESC
             LIMIT 1",
            params![app_type, at_ms],
            |row| {
                Ok(ProviderActivation {
                    id: row.get(0)?,
                    app_type: row.get(1)?,
                    provider_id: row.get(2)?,
                    mode: row.get(3)?,
                    effective_at_ms: row.get(4)?,
                    source: row.get(5)?,
                    config_fingerprint: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(|e| AppError::Database(format!("查询供应商激活失败: {e}")))
    }

    /// 会话项目写入/更新（同一会话后出现的项目覆盖先前的值）。
    pub fn upsert_session_project(&self, project: &SessionProject) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::upsert_session_project_on_conn(&conn, project).map(|_| ())
    }

    /// 更新索引，并给同会话尚未归属项目的代理明细补证据（含失败请求）。
    /// 返回补齐的代理明细数；不改变已有项目或供应商归属。
    pub fn upsert_session_project_on_conn(
        conn: &Connection,
        project: &SessionProject,
    ) -> Result<usize, AppError> {
        conn.execute(
            "INSERT INTO session_projects
                (app_type, session_key, project_key, project_path, project_name, source, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(app_type, session_key) DO UPDATE SET
                project_key = excluded.project_key,
                project_path = excluded.project_path,
                project_name = excluded.project_name,
                source = excluded.source,
                updated_at = excluded.updated_at",
            params![
                project.app_type,
                project.session_key,
                project.project_key,
                project.project_path,
                project.project_name,
                project.source,
                chrono::Utc::now().timestamp()
            ],
        )
        .map_err(|e| AppError::Database(format!("写入会话项目索引失败: {e}")))?;
        conn.execute(
            "UPDATE proxy_request_logs SET
                project_key = ?3,
                project_path = CASE WHEN project_path = '' THEN ?4 ELSE project_path END,
                project_name = CASE WHEN project_name = '' THEN ?5 ELSE project_name END
             WHERE app_type = ?1 AND session_key = ?2
               AND ?2 <> '' AND ?3 <> '' AND project_key = ''
               AND COALESCE(data_source, 'proxy') = 'proxy'",
            params![
                project.app_type,
                project.session_key,
                project.project_key,
                project.project_path,
                project.project_name
            ],
        )
        .map_err(|e| AppError::Database(format!("补齐会话代理明细项目失败: {e}")))
    }

    pub fn session_project(
        &self,
        app_type: &str,
        session_key: &str,
    ) -> Result<Option<SessionProject>, AppError> {
        let conn = lock_conn!(self.conn);
        Self::session_project_on_conn(&conn, app_type, session_key)
    }

    pub(crate) fn session_project_on_conn(
        conn: &Connection,
        app_type: &str,
        session_key: &str,
    ) -> Result<Option<SessionProject>, AppError> {
        conn.query_row(
            "SELECT app_type, session_key, project_key, project_path, project_name, source
             FROM session_projects
             WHERE app_type = ?1 AND session_key = ?2",
            params![app_type, session_key],
            |row| {
                Ok(SessionProject {
                    app_type: row.get(0)?,
                    session_key: row.get(1)?,
                    project_key: row.get(2)?,
                    project_path: row.get(3)?,
                    project_name: row.get(4)?,
                    source: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(|e| AppError::Database(format!("读取会话项目索引失败: {e}")))
    }
}

/// 按请求区间解析直连供应商。`start_s`/`end_s` 是代理明细的秒级时间。
pub fn resolve_provider_attribution(
    conn: &Connection,
    app_type: &str,
    start_s: i64,
    end_s: i64,
    placeholder: &str,
) -> Result<Attribution, AppError> {
    let start_ms = start_s.saturating_mul(1000);
    let end_ms = end_s.saturating_mul(1000);
    let Some(activation) = Database::last_provider_activation_on_conn(conn, app_type, start_ms)?
    else {
        return Ok(Attribution::unknown(
            placeholder,
            "no_activation_before_request",
        ));
    };
    if activation.mode != "direct" || activation.provider_id.is_none() {
        return Ok(Attribution::unknown(
            placeholder,
            "request_not_in_direct_mode",
        ));
    }

    // 同一秒内或请求区间内再有激活，说明请求跨越了切换边界，不能武断归属。
    let boundary_end_ms = if start_s == end_s {
        start_ms.saturating_add(1000)
    } else {
        end_ms
    };
    let boundary: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM provider_activations
             WHERE app_type = ?1 AND effective_at_ms > ?2 AND effective_at_ms <= ?3
             LIMIT 1",
            params![app_type, start_ms, boundary_end_ms],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| AppError::Database(format!("检查切换边界失败: {e}")))?;
    if boundary.is_some() {
        return Ok(Attribution::unknown(
            placeholder,
            "activation_boundary_in_request",
        ));
    }

    Ok(Attribution {
        provider_id: activation.provider_id.unwrap_or_default(),
        source: "activation".to_string(),
        confidence: "high".to_string(),
        reason: String::new(),
    })
}

/// Codex 原生 `session_meta.model_provider` → 归属决策（确定性映射）。
///
/// - `openai` → 官方卡（Codex 内建 OpenAI 订阅）。
/// - 与 cc-switch 供应商卡 id 完全一致的值（`provider-a`、`volcengine` 等）→ 该卡。
/// - `custom` 是 cc-switch 的通用路由键，历史可指向不同卡 → 归入独立未识别桶
///   `_codex_custom`；`codex_local_access` 是本地服务 → 归入 `_codex_local_access`。
///   `custom` 与缺失时通过供应商激活时间线解析当时的卡；两者都无法唯一映射
///   时保留未识别桶，绝不用“当前选中供应商”回填历史。
pub fn codex_attribution_from_model_provider(
    conn: &Connection,
    model_provider: Option<&str>,
    start_s: i64,
    end_s: i64,
) -> Result<Attribution, AppError> {
    let Some(model_provider) = model_provider else {
        return Ok(Attribution::unknown(
            "_codex_session",
            "no_native_model_provider",
        ));
    };
    if model_provider == "openai" {
        return Ok(Attribution::exact(
            crate::database::CODEX_OFFICIAL_PROVIDER_ID,
            "session_meta",
        ));
    }
    if model_provider == "custom" {
        return resolve_provider_attribution(conn, "codex", start_s, end_s, "_codex_custom");
    }
    if model_provider == "codex_local_access" {
        return Ok(Attribution::unknown(
            "_codex_local_access",
            "codex_local_access",
        ));
    }
    if provider_name_snapshot_on_conn(conn, model_provider, "codex")?.is_some() {
        return Ok(Attribution::exact(model_provider, "session_meta"));
    }
    Ok(Attribution::unknown(
        "_codex_session",
        format!("unmapped_model_provider:{model_provider}"),
    ))
}

/// 按当前模式与供应商写入一条激活基线。非切换式客户端不记录。
pub fn record_activation_for_current_state(
    db: &Database,
    app_type: &AppType,
    source: &str,
) -> Result<bool, AppError> {
    if app_type.is_additive_mode() || !app_type.has_route_mode() {
        return Ok(false);
    }
    let mode_state = crate::mode::current::mode_state(app_type);
    let mode = if mode_state.is_proxy() {
        "proxy"
    } else {
        "direct"
    };
    let provider_id =
        crate::mode::current::provider_for(db, app_type, crate::mode::current::Purpose::InUse)?;
    let fingerprint = provider_id
        .as_deref()
        .and_then(|id| db.get_provider_by_id(id, app_type.as_str()).ok().flatten())
        .map(|provider| provider_config_fingerprint(&provider.settings_config.to_string()))
        .unwrap_or_default();
    db.record_provider_activation(
        app_type.as_str(),
        provider_id.as_deref(),
        mode,
        source,
        &fingerprint,
    )
}

/// 去重命中时把会话项目证据写回代理行（只填空值，不覆盖已有证据）。
/// 返回命中的代理行数。
pub fn backfill_project_for_dedup(
    conn: &Connection,
    key: &crate::services::usage_stats::DedupKey<'_>,
    project: &SessionProject,
) -> Result<usize, AppError> {
    let ids = crate::services::usage_stats::matching_proxy_request_ids(conn, key)?;
    for id in &ids {
        conn.execute(
            "UPDATE proxy_request_logs SET
                project_key = CASE WHEN project_key = '' THEN ?1 ELSE project_key END,
                project_name = CASE WHEN project_name = '' THEN ?2 ELSE project_name END,
                project_path = CASE WHEN project_path = '' THEN ?3 ELSE project_path END,
                session_key = CASE WHEN session_key = '' THEN ?4 ELSE session_key END
             WHERE request_id = ?5",
            params![
                project.project_key,
                project.project_name,
                project.project_path,
                project.session_key,
                id
            ],
        )
        .map_err(|e| AppError::Database(format!("回写代理行项目证据失败: {e}")))?;
    }
    Ok(ids.len())
}

/// 会话写入前的统一去重：命中代理行时先把项目证据回写，再跳过插入。
pub fn skip_if_duplicate_with_project(
    conn: &Connection,
    request_id: &str,
    key: &crate::services::usage_stats::DedupKey<'_>,
    project: Option<&SessionProject>,
) -> Result<bool, AppError> {
    if !crate::services::usage_stats::should_skip_session_insert(conn, request_id, key)? {
        return Ok(false);
    }
    if let Some(project) = project {
        backfill_project_for_dedup(conn, key, project)?;
    }
    Ok(true)
}

/// 只按指纹判断是否命中代理行；命中时回写项目证据。
pub fn backfill_if_proxy_duplicate(
    conn: &Connection,
    key: &crate::services::usage_stats::DedupKey<'_>,
    project: Option<&SessionProject>,
) -> Result<bool, AppError> {
    if !crate::services::usage_stats::has_matching_proxy_usage_log(conn, key)? {
        return Ok(false);
    }
    if let Some(project) = project {
        backfill_project_for_dedup(conn, key, project)?;
    }
    Ok(true)
}

/// 读取供应商当前显示名，用于写入名称快照。
pub fn provider_name_snapshot_on_conn(
    conn: &Connection,
    provider_id: &str,
    app_type: &str,
) -> Result<Option<String>, AppError> {
    if provider_id.is_empty() {
        return Ok(None);
    }
    conn.query_row(
        "SELECT name FROM providers WHERE id = ?1 AND app_type = ?2",
        params![provider_id, app_type],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|e| AppError::Database(format!("读取供应商名称快照失败: {e}")))
}

/// 启动时为所有切换式客户端补一条基线，保证升级后新产生的会话有归属依据。
pub fn ensure_startup_baselines(db: &Database) {
    for app_type in AppType::all() {
        if app_type.is_additive_mode() || !app_type.has_route_mode() {
            continue;
        }
        if let Err(err) = record_activation_for_current_state(db, &app_type, "startup") {
            log::warn!(
                "[USAGE-ATTR] 写入 {} 启动基线失败: {err}",
                app_type.as_str()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn worktree_fixture(base: &Path) -> (PathBuf, PathBuf) {
        let repo = base.join("main/demo");
        let worktree = base.join("worktrees/agent/demo");
        let git_dir = repo.join(".git/worktrees/agent");
        fs::create_dir_all(&git_dir).unwrap();
        fs::create_dir_all(worktree.join("src")).unwrap();
        fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", git_dir.display()),
        )
        .unwrap();
        fs::write(git_dir.join("commondir"), "../..\n").unwrap();
        (repo, worktree)
    }

    #[test]
    fn project_identity_groups_linked_worktrees_across_tools() {
        let temp = tempfile::tempdir().unwrap();
        let (repo, worktree) = worktree_fixture(temp.path());
        let main =
            project_from_cwd("codex", "s1", &repo.to_string_lossy(), "session_meta").unwrap();
        let linked = project_from_cwd(
            "claude",
            "s2",
            &worktree.join("src").to_string_lossy(),
            "session_meta",
        )
        .unwrap();
        assert_eq!(linked.project_key, main.project_key);
        assert_eq!(
            linked.project_path,
            normalize_pathbuf(&strip_windows_verbatim_prefix(
                &fs::canonicalize(worktree.join("src")).unwrap()
            ))
        );
        assert_eq!(linked.project_name, "src");
    }

    #[test]
    fn project_identity_keeps_same_named_independent_repositories_separate() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("a/demo");
        let second = temp.path().join("b/demo");
        fs::create_dir_all(first.join(".git")).unwrap();
        fs::create_dir_all(second.join(".git")).unwrap();
        let first =
            project_from_cwd("codex", "s1", &first.to_string_lossy(), "session_meta").unwrap();
        let second =
            project_from_cwd("claude", "s2", &second.to_string_lossy(), "session_meta").unwrap();
        assert_eq!(first.project_name, second.project_name);
        assert_ne!(first.project_key, second.project_key);
    }

    #[test]
    fn project_identity_handles_relative_git_links_without_merging_submodules() {
        let temp = tempfile::tempdir().unwrap();
        let (repo, worktree) = worktree_fixture(temp.path());
        fs::write(
            worktree.join(".git"),
            "gitdir: ../../../main/demo/.git/worktrees/agent\n",
        )
        .unwrap();
        let linked =
            project_from_cwd("claude", "s", &worktree.to_string_lossy(), "session_meta").unwrap();
        assert_eq!(
            linked.project_key,
            normalize_pathbuf(&strip_windows_verbatim_prefix(
                &fs::canonicalize(&repo).unwrap()
            ))
        );
        fs::remove_file(repo.join(".git/worktrees/agent/commondir")).unwrap();
        let separate =
            project_from_cwd("claude", "s", &worktree.to_string_lossy(), "session_meta").unwrap();
        assert_eq!(
            separate.project_key,
            normalize_pathbuf(&strip_windows_verbatim_prefix(
                &fs::canonicalize(&worktree).unwrap()
            ))
        );
    }

    #[test]
    fn worktree_history_repair_preserves_usage_and_provider_evidence() -> Result<(), AppError> {
        let temp = tempfile::tempdir().unwrap();
        let (repo, worktree) = worktree_fixture(temp.path());
        let main = normalize_pathbuf(&strip_windows_verbatim_prefix(
            &fs::canonicalize(&repo).unwrap(),
        ));
        let alias = normalize_pathbuf(&strip_windows_verbatim_prefix(
            &fs::canonicalize(&worktree).unwrap(),
        ));
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute("INSERT INTO session_projects(app_type,session_key,project_key,project_path,project_name,source,updated_at) VALUES ('claude','s',?1,?1,'demo','session_meta',1)", [&alias])?;
            for (id, app, key) in [("main", "codex", &main), ("linked", "claude", &alias)] {
                conn.execute("INSERT INTO proxy_request_logs(request_id,provider_id,app_type,model,latency_ms,status_code,created_at,project_key,project_path,project_name,session_key,provider_name_snapshot,attribution_source,attribution_confidence) VALUES (?1,'keep-provider',?2,'m',10,500,1,?3,?3,'demo','s','Original','proxy','exact')", params![id,app,key])?;
            }
            for (key, source, model, count, input, output, read, semantics, cost, latency) in [
                (
                    &main,
                    "codex_session",
                    "m",
                    1,
                    100,
                    5,
                    80,
                    1,
                    "0.123456789012345678",
                    100,
                ),
                (&alias, "codex_session", "m", 3, 10, 7, 20, 2, "0.2", 300),
                (&alias, "proxy", "m", 2, 12, 0, 0, 2, "0.04", 50),
                (
                    &alias,
                    "codex_session",
                    "other-model",
                    1,
                    2,
                    0,
                    0,
                    2,
                    "0.005",
                    10,
                ),
            ] {
                conn.execute("INSERT INTO usage_daily_rollups(date,app_type,provider_id,model,data_source,project_key,project_path,project_name,provider_name_snapshot,request_count,success_count,input_tokens,output_tokens,cache_read_tokens,input_token_semantics,total_cost_usd,avg_latency_ms) VALUES ('2026-01-01','codex','keep-provider',?1,?2,?3,?3,'demo','Original',?4,?4,?5,?6,?7,?8,?9,?10)", params![model,source,key,count,input,output,read,semantics,cost,latency])?;
            }
        }
        assert_eq!(db.normalize_worktree_projects()?, 1);
        assert_eq!(db.normalize_worktree_projects()?, 0);
        {
            let conn = lock_conn!(db.conn);
            let detail: (String,String,String,String,String) = conn.query_row("SELECT project_key,project_path,provider_id,provider_name_snapshot,attribution_confidence FROM proxy_request_logs WHERE request_id='linked'", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
            assert_eq!(
                detail,
                (
                    main.clone(),
                    alias.clone(),
                    "keep-provider".into(),
                    "Original".into(),
                    "exact".into()
                )
            );
            let indexed: String = conn.query_row(
                "SELECT project_key FROM session_projects WHERE session_key='s'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(indexed, main);
            let merged: (i64,i64,i64,i64,i64,i64,String) = conn.query_row("SELECT request_count,input_tokens,output_tokens,cache_read_tokens,input_token_semantics,avg_latency_ms,total_cost_usd FROM usage_daily_rollups WHERE project_key=?1 AND model='m' AND data_source='codex_session'", [&main], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?)))?;
            assert_eq!(
                merged,
                (4, 30, 12, 100, 2, 250, "0.323456789012345678".into())
            );
            let totals: (i64, i64) = conn.query_row(
                "SELECT COUNT(*),SUM(request_count) FROM usage_daily_rollups",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            assert_eq!(totals, (3, 7));
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |r| r
                    .get::<_, i64>(0))?,
                2
            );
            assert_eq!(
                conn.query_row(
                    "SELECT COUNT(*) FROM usage_daily_rollups WHERE project_key=?1",
                    [&alias],
                    |r| r.get::<_, i64>(0)
                )?,
                0
            );
        }
        let projects = db.get_project_stats(None, None, None, None, None, None, None, None)?;
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].app_count, 2);
        assert_eq!(projects[0].provider_count, 2);
        assert_eq!(
            db.get_usage_projects(None, None, None, None, None, None, None)?
                .len(),
            1
        );
        Ok(())
    }

    #[test]
    fn worktree_history_repair_rolls_back_on_invalid_rollup_cost() -> Result<(), AppError> {
        let temp = tempfile::tempdir().unwrap();
        let (repo, worktree) = worktree_fixture(temp.path());
        let main = normalize_pathbuf(&strip_windows_verbatim_prefix(
            &fs::canonicalize(&repo).unwrap(),
        ));
        let alias = normalize_pathbuf(&strip_windows_verbatim_prefix(
            &fs::canonicalize(&worktree).unwrap(),
        ));
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            for (model, key, cost) in [
                ("a", &main, "0.1"),
                ("a", &alias, "0.2"),
                ("b", &main, "0.1"),
                ("b", &alias, "invalid"),
            ] {
                conn.execute("INSERT INTO usage_daily_rollups(date,app_type,provider_id,model,project_key,request_count,total_cost_usd) VALUES ('2026-01-01','claude','p',?1,?2,1,?3)",params![model,key,cost])?;
            }
        }
        assert!(db.normalize_worktree_projects().is_err());
        let conn = lock_conn!(db.conn);
        let counts: (i64, i64) = conn.query_row(
            "SELECT COUNT(*),SUM(request_count) FROM usage_daily_rollups",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(counts, (4, 4));
        let retained: String = conn.query_row(
            "SELECT total_cost_usd FROM usage_daily_rollups WHERE project_key=?1 AND model='a'",
            [&main],
            |r| r.get(0),
        )?;
        assert_eq!(retained, "0.1");
        Ok(())
    }

    #[test]
    #[ignore = "requires a disposable database copy via CC_SWITCH_WORKTREE_REPAIR_TEST_DB"]
    fn worktree_history_repair_on_database_copy() -> Result<(), AppError> {
        let path = std::env::var("CC_SWITCH_WORKTREE_REPAIR_TEST_DB")
            .map_err(|e| AppError::Database(e.to_string()))?;
        let db = Database::memory()?;
        *lock_conn!(db.conn) =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        let snapshot = || -> Result<(i64, i64, Decimal), AppError> {
            let conn = lock_conn!(db.conn);
            let mut stmt = conn.prepare(&format!(
                "SELECT 1, {}, total_cost_usd FROM proxy_request_logs l
                 UNION ALL SELECT request_count, {}, total_cost_usd FROM usage_daily_rollups r",
                super::super::sql_helpers::real_total_tokens_sql("l"),
                super::super::sql_helpers::real_total_tokens_sql("r")
            ))?;
            let mut requests = 0;
            let mut tokens = 0;
            let mut cost = Decimal::ZERO;
            for row in stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })? {
                let (count, token_count, value) = row?;
                requests += count;
                tokens += token_count;
                cost += Decimal::from_str(&value).map_err(|e| AppError::Database(e.to_string()))?;
            }
            Ok((requests, tokens, cost))
        };
        let before = snapshot()?;
        let merged = db.normalize_worktree_projects()?;
        assert_eq!(snapshot()?, before);
        assert_eq!(db.normalize_worktree_projects()?, 0);
        println!(
            "repaired project keys={merged}; requests={}, real_tokens={}, cost={} preserved",
            before.0, before.1, before.2
        );
        Ok(())
    }

    #[test]
    fn normalize_session_key_strips_codex_prefix_and_claude_wrapper() {
        assert_eq!(normalize_session_key("codex", "codex_019a-abc"), "019a-abc");
        assert_eq!(
            normalize_session_key("claude", "user_abc_session_def-123"),
            "def-123"
        );
        assert_eq!(normalize_session_key("gemini", "  s-1  "), "s-1");
    }

    #[test]
    fn session_project_backfills_existing_proxy_rows_without_changing_evidence(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            for (id, app, session, project, status) in [
                ("success", "codex", "s1", "", 200),
                ("failure", "codex", "s1", "", 500),
                ("known", "codex", "s1", "/workspace/original", 200),
                ("other-app", "claude", "s1", "", 200),
                ("other-session", "codex", "s2", "", 200),
            ] {
                conn.execute(
                    "INSERT INTO proxy_request_logs
                     (request_id, provider_id, provider_name_snapshot, app_type, model,
                      latency_ms, status_code, created_at, session_id, session_key, project_key,
                      attribution_source, attribution_confidence, attribution_reason)
                     VALUES (?1, 'proxy-provider', 'Original Provider', ?2, 'gpt-5.5',
                             1, ?5, 100, ?3, ?3, ?4, 'proxy', 'exact', 'routed')",
                    params![id, app, session, project, status],
                )?;
            }
        }
        let project = project_from_cwd("codex", "s1", "/workspace/demo", "session_meta").unwrap();
        db.upsert_session_project(&project)?;
        // Repeated evidence must not rewrite a previously recorded project.
        db.upsert_session_project(&project)?;

        let conn = lock_conn!(db.conn);
        for (id, expected) in [
            ("success", "/workspace/demo"),
            ("failure", "/workspace/demo"),
            ("known", "/workspace/original"),
            ("other-app", ""),
            ("other-session", ""),
        ] {
            let row: (String, String, String, String, String, String) = conn.query_row(
                "SELECT project_key, provider_id, provider_name_snapshot,
                        attribution_source, attribution_confidence, attribution_reason
                 FROM proxy_request_logs WHERE request_id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )?;
            assert_eq!(
                row,
                (
                    expected.into(),
                    "proxy-provider".into(),
                    "Original Provider".into(),
                    "proxy".into(),
                    "exact".into(),
                    "routed".into()
                ),
                "{id}"
            );
        }
        Ok(())
    }

    #[test]
    fn project_from_cwd_uses_git_root_and_keeps_missing_paths() {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        let nested = repo.join("src/deep");
        fs::create_dir_all(&nested).expect("nested");
        fs::create_dir_all(repo.join(".git")).expect(".git");

        let project = project_from_cwd("codex", "s1", &nested.to_string_lossy(), "session_meta")
            .expect("project");
        assert_eq!(
            project.project_key,
            normalize_pathbuf(&strip_windows_verbatim_prefix(
                &fs::canonicalize(&repo).unwrap()
            ))
        );
        // 项目名取 cwd 末级目录名（用户直觉），project_key 仍为 git 根目录。
        assert_eq!(project.project_name, "deep");

        let missing = project_from_cwd("codex", "s2", "/definitely/not/here/", "session_meta")
            .expect("missing path still yields a project");
        assert_eq!(missing.project_key, "/definitely/not/here");
        assert_eq!(missing.project_name, "here");

        let uri = project_from_cwd("codex", "s3", "file:///definitely/not/here", "session_meta")
            .expect("file uri");
        assert_eq!(uri.project_key, "/definitely/not/here");
    }

    #[test]
    fn resolve_provider_attribution_respects_mode_and_boundaries() -> Result<(), AppError> {
        let db = Database::memory()?;
        let conn = lock_conn!(db.conn);

        let unknown = resolve_provider_attribution(&conn, "codex", 1_000, 1_000, "_codex_session")?;
        assert_eq!(unknown.confidence, "unknown");
        assert_eq!(unknown.reason, "no_activation_before_request");

        // 直接插入固定时间的激活，避免依赖墙钟。
        conn.execute(
            "INSERT INTO provider_activations
                (app_type, provider_id, mode, effective_at_ms, source, config_fingerprint, created_at)
             VALUES ('codex', 'p1', 'direct', 1000000, 'switch', 'f1', 1)",
            [],
        )?;
        let attributed =
            resolve_provider_attribution(&conn, "codex", 1_000, 1_000, "_codex_session")?;
        assert_eq!(attributed.provider_id, "p1");
        assert_eq!(attributed.confidence, "high");
        assert_eq!(attributed.source, "activation");

        // 请求区间内出现新激活 → 歧义。
        conn.execute(
            "INSERT INTO provider_activations
                (app_type, provider_id, mode, effective_at_ms, source, config_fingerprint, created_at)
             VALUES ('codex', 'p2', 'direct', 1500000, 'switch', 'f2', 1)",
            [],
        )?;
        let boundary =
            resolve_provider_attribution(&conn, "codex", 1_000, 2_000, "_codex_session")?;
        assert_eq!(boundary.confidence, "unknown");
        assert_eq!(boundary.reason, "activation_boundary_in_request");

        // 代理模式下不查直连时间线。
        conn.execute(
            "INSERT INTO provider_activations
                (app_type, provider_id, mode, effective_at_ms, source, config_fingerprint, created_at)
             VALUES ('codex', 'p3', 'proxy', 2500000, 'mode_change', 'f3', 1)",
            [],
        )?;
        let proxy = resolve_provider_attribution(&conn, "codex", 3_000, 3_000, "_codex_session")?;
        assert_eq!(proxy.confidence, "unknown");
        assert_eq!(proxy.reason, "request_not_in_direct_mode");
        Ok(())
    }

    #[test]
    fn activation_records_dedupe_identical_consecutive_state() -> Result<(), AppError> {
        let db = Database::memory()?;
        assert!(db.record_provider_activation("codex", Some("p1"), "direct", "switch", "fp")?);
        assert!(!db.record_provider_activation("codex", Some("p1"), "direct", "switch", "fp")?);
        assert!(db.record_provider_activation("codex", Some("p2"), "direct", "switch", "fp")?);
        Ok(())
    }

    #[test]
    fn codex_custom_route_uses_activation_timeline() -> Result<(), AppError> {
        let db = Database::memory()?;
        let conn = lock_conn!(db.conn);

        let without_activation =
            codex_attribution_from_model_provider(&conn, Some("custom"), 1_000, 1_000)?;
        assert_eq!(without_activation.provider_id, "_codex_custom");
        assert_eq!(without_activation.confidence, "unknown");

        conn.execute(
            "INSERT INTO provider_activations
                (app_type, provider_id, mode, effective_at_ms, source, config_fingerprint, created_at)
             VALUES ('codex', 'volcengine', 'direct', 500000, 'switch', 'f1', 1)",
            [],
        )?;
        let attributed =
            codex_attribution_from_model_provider(&conn, Some("custom"), 1_000, 1_000)?;
        assert_eq!(attributed.provider_id, "volcengine");
        assert_eq!(attributed.confidence, "high");
        assert_eq!(attributed.source, "activation");
        Ok(())
    }
}
