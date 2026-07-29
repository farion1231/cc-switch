//! 可被桌面端与临时 Agent 共同使用的无界面业务核心。
//!
//! 该边界禁止依赖 Tauri、托盘、窗口和代理服务器生命周期。Provider 状态将在下一阶段
//! 迁入此处；先建立独立包可以让依赖审计在业务迁移前持续生效。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use indexmap::IndexMap;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use cc_switch_protocol::capabilities::{CommandCapabilityRegistry, RemoteCapabilityError};

/// 最小数据库 schema 版本仅描述 Agent 当前可读写的 Provider 边界，不冒充桌面数据库的
/// 全量迁移版本。后续扩表时必须随兼容性测试递增。
pub const SCHEMA_VERSION: i32 = 1;

/// Agent 可安全构造的最小状态，只持有业务数据库与显式 HOME。
///
/// HOME 不从进程全局环境变量重复读取，避免并发测试和长连接之间互相污染；桌面端接入时
/// 也可以把已解析的用户目录明确传入。
pub struct HeadlessState {
    connection: Mutex<Connection>,
    home: PathBuf,
}

impl HeadlessState {
    pub fn open(home: impl AsRef<Path>) -> Result<Self, CoreError> {
        let home = home.as_ref().to_path_buf();
        let data_dir = home.join(".cc-switch");
        std::fs::create_dir_all(&data_dir).map_err(|source| CoreError::Io {
            path: data_dir.clone(),
            source,
        })?;
        let database_path = data_dir.join("cc-switch.db");
        let connection = Connection::open(&database_path)?;
        // Agent 会话不注册桌面端自动同步 hook；远程命令完成后由协议响应明确确认落盘结果。
        initialize_schema(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
            home,
        })
    }

    pub fn memory(home: impl AsRef<Path>) -> Result<Self, CoreError> {
        let connection = Connection::open_in_memory()?;
        initialize_schema(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
            home: home.as_ref().to_path_buf(),
        })
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, CoreError> {
        self.connection.lock().map_err(|_| CoreError::StatePoisoned)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRecord {
    pub id: String,
    pub name: String,
    pub settings_config: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_index: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSortUpdate {
    pub id: String,
    pub sort_index: i64,
}

pub struct ProviderService;

impl ProviderService {
    pub fn list(
        state: &HeadlessState,
        app: &str,
    ) -> Result<IndexMap<String, ProviderRecord>, CoreError> {
        validate_app(app)?;
        let connection = state.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, name, settings_config, website_url, category, created_at, sort_index, notes, meta
             FROM providers WHERE app = ?1
             ORDER BY sort_index ASC, created_at ASC, id ASC",
        )?;
        let rows = statement.query_map([app], provider_from_row)?;
        let mut providers = IndexMap::new();
        for provider in rows {
            let provider = provider?;
            providers.insert(provider.id.clone(), provider);
        }
        Ok(providers)
    }

    pub fn current(state: &HeadlessState, app: &str) -> Result<String, CoreError> {
        validate_app(app)?;
        let connection = state.connection()?;
        Ok(connection
            .query_row(
                "SELECT provider_id FROM current_providers WHERE app = ?1",
                [app],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or_default())
    }

    pub fn add(
        state: &HeadlessState,
        app: &str,
        mut provider: ProviderRecord,
        _add_to_live: bool,
    ) -> Result<bool, CoreError> {
        validate_provider(app, &provider)?;
        let mut connection = state.connection()?;
        let transaction = connection.transaction()?;
        let current = current_in_transaction(&transaction, app)?;
        provider.created_at.get_or_insert_with(unix_timestamp);
        provider
            .sort_index
            .get_or_insert(next_sort_index(&transaction, app)?);
        insert_provider(&transaction, app, &provider)?;
        if current.is_none() {
            set_current(&transaction, app, &provider.id)?;
        }
        transaction.commit()?;
        drop(connection);

        // 独占模式的首个供应商必须立即形成 live 配置，否则数据库显示“当前”但 CLI 仍无配置。
        if current.is_none() {
            write_live(state, app, &provider.settings_config)?;
        }
        Ok(true)
    }

    pub fn update(
        state: &HeadlessState,
        app: &str,
        original_id: &str,
        mut provider: ProviderRecord,
    ) -> Result<bool, CoreError> {
        validate_provider(app, &provider)?;
        if original_id != provider.id {
            return Err(CoreError::ProviderIdChangeUnsupported);
        }

        let connection = state.connection()?;
        let existing = get_provider(&connection, app, original_id)?
            .ok_or_else(|| CoreError::ProviderNotFound(original_id.to_string()))?;
        provider.created_at = provider.created_at.or(existing.created_at);
        provider.sort_index = provider.sort_index.or(existing.sort_index);
        let changed = update_provider(&connection, app, &provider)?;
        let is_current = current_in_connection(&connection, app)?.as_deref() == Some(original_id);
        drop(connection);

        if is_current {
            write_live(state, app, &provider.settings_config)?;
        }
        Ok(changed == 1)
    }

    pub fn delete(state: &HeadlessState, app: &str, id: &str) -> Result<(), CoreError> {
        validate_app(app)?;
        let connection = state.connection()?;
        if current_in_connection(&connection, app)?.as_deref() == Some(id) {
            return Err(CoreError::CurrentProviderDeletion);
        }
        connection.execute(
            "DELETE FROM providers WHERE app = ?1 AND id = ?2",
            params![app, id],
        )?;
        Ok(())
    }

    pub fn switch(state: &HeadlessState, app: &str, id: &str) -> Result<(), CoreError> {
        validate_app(app)?;
        let mut connection = state.connection()?;
        let transaction = connection.transaction()?;
        let provider = get_provider(&transaction, app, id)?
            .ok_or_else(|| CoreError::ProviderNotFound(id.to_string()))?;
        set_current(&transaction, app, id)?;
        transaction.commit()?;
        drop(connection);
        write_live(state, app, &provider.settings_config)
    }

    pub fn update_sort_order(
        state: &HeadlessState,
        app: &str,
        updates: &[ProviderSortUpdate],
    ) -> Result<(), CoreError> {
        validate_app(app)?;
        let mut connection = state.connection()?;
        let transaction = connection.transaction()?;
        for update in updates {
            let changed = transaction.execute(
                "UPDATE providers SET sort_index = ?3 WHERE app = ?1 AND id = ?2",
                params![app, update.id, update.sort_index],
            )?;
            if changed != 1 {
                return Err(CoreError::ProviderNotFound(update.id.clone()));
            }
        }
        transaction.commit()?;
        Ok(())
    }
}

/// 将稳定 RPC 命令名映射到无界面 Provider 服务。
///
/// 参数解析放在 core 而非 Agent 入口，保证进程内测试、桌面兼容层和 SSH 进程使用同一套
/// camelCase DTO 与错误码，不在各传输层复制业务分发规则。
pub fn dispatch_provider_command(
    state: &HeadlessState,
    command: &str,
    args: Value,
) -> Result<Value, ProviderCommandError> {
    CommandCapabilityRegistry::provider_phase().require(command)?;
    match command {
        "provider.list" => {
            let args: AppArgs = parse_args(args)?;
            to_value(ProviderService::list(state, &args.app)?)
        }
        "provider.current" => {
            let args: AppArgs = parse_args(args)?;
            to_value(ProviderService::current(state, &args.app)?)
        }
        "provider.add" => {
            let args: AddArgs = parse_args(args)?;
            to_value(ProviderService::add(
                state,
                &args.app,
                args.provider,
                args.add_to_live.unwrap_or(true),
            )?)
        }
        "provider.update" => {
            let args: UpdateArgs = parse_args(args)?;
            let original_id = args.original_id.unwrap_or_else(|| args.provider.id.clone());
            to_value(ProviderService::update(
                state,
                &args.app,
                &original_id,
                args.provider,
            )?)
        }
        "provider.delete" => {
            let args: IdArgs = parse_args(args)?;
            ProviderService::delete(state, &args.app, &args.id)?;
            Ok(Value::Bool(true))
        }
        "provider.switch" => {
            let args: IdArgs = parse_args(args)?;
            ProviderService::switch(state, &args.app, &args.id)?;
            Ok(serde_json::json!({ "warnings": [] }))
        }
        "provider.update_sort_order" => {
            let args: SortArgs = parse_args(args)?;
            ProviderService::update_sort_order(state, &args.app, &args.updates)?;
            Ok(Value::Bool(true))
        }
        _ => Err(ProviderCommandError::CommandNotExposed(command.to_string())),
    }
}

#[derive(Debug, Deserialize)]
struct AppArgs {
    app: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddArgs {
    app: String,
    provider: ProviderRecord,
    add_to_live: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateArgs {
    app: String,
    provider: ProviderRecord,
    original_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IdArgs {
    app: String,
    id: String,
}

#[derive(Debug, Deserialize)]
struct SortArgs {
    app: String,
    updates: Vec<ProviderSortUpdate>,
}

fn parse_args<T: for<'de> Deserialize<'de>>(args: Value) -> Result<T, ProviderCommandError> {
    serde_json::from_value(args)
        .map_err(|error| ProviderCommandError::InvalidArgument(error.to_string()))
}

fn to_value<T: Serialize>(value: T) -> Result<Value, ProviderCommandError> {
    serde_json::to_value(value).map_err(ProviderCommandError::Serialization)
}

fn initialize_schema(connection: &Connection) -> Result<(), CoreError> {
    connection.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE IF NOT EXISTS providers (
             app TEXT NOT NULL,
             id TEXT NOT NULL,
             name TEXT NOT NULL,
             settings_config TEXT NOT NULL,
             website_url TEXT,
             category TEXT,
             created_at INTEGER NOT NULL,
             sort_index INTEGER NOT NULL,
             notes TEXT,
             meta TEXT,
             PRIMARY KEY (app, id)
         );
         CREATE TABLE IF NOT EXISTS current_providers (
             app TEXT PRIMARY KEY,
             provider_id TEXT NOT NULL
         );",
    )?;
    Ok(())
}

fn validate_app(app: &str) -> Result<(), CoreError> {
    match app {
        "claude" | "codex" | "gemini" | "grokbuild" => Ok(()),
        _ => Err(CoreError::UnsupportedApp(app.to_string())),
    }
}

fn validate_provider(app: &str, provider: &ProviderRecord) -> Result<(), CoreError> {
    validate_app(app)?;
    if provider.id.trim().is_empty() || provider.name.trim().is_empty() {
        return Err(CoreError::InvalidProvider(
            "供应商 id 和 name 不能为空".to_string(),
        ));
    }
    if !provider.settings_config.is_object() {
        return Err(CoreError::InvalidProvider(
            "settingsConfig 必须是 JSON 对象".to_string(),
        ));
    }
    Ok(())
}

fn insert_provider(
    transaction: &Transaction<'_>,
    app: &str,
    provider: &ProviderRecord,
) -> Result<(), CoreError> {
    transaction.execute(
        "INSERT INTO providers
         (app, id, name, settings_config, website_url, category, created_at, sort_index, notes, meta)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        provider_params(app, provider)?,
    )?;
    Ok(())
}

fn update_provider(
    connection: &Connection,
    app: &str,
    provider: &ProviderRecord,
) -> Result<usize, CoreError> {
    Ok(connection.execute(
        "UPDATE providers SET name = ?3, settings_config = ?4, website_url = ?5,
         category = ?6, created_at = ?7, sort_index = ?8, notes = ?9, meta = ?10
         WHERE app = ?1 AND id = ?2",
        provider_params(app, provider)?,
    )?)
}

fn provider_params<'a>(
    app: &'a str,
    provider: &'a ProviderRecord,
) -> Result<[rusqlite::types::Value; 10], CoreError> {
    Ok([
        app.to_string().into(),
        provider.id.clone().into(),
        provider.name.clone().into(),
        serde_json::to_string(&provider.settings_config)?.into(),
        provider.website_url.clone().into(),
        provider.category.clone().into(),
        provider.created_at.unwrap_or_else(unix_timestamp).into(),
        provider.sort_index.unwrap_or_default().into(),
        provider.notes.clone().into(),
        provider
            .meta
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?
            .into(),
    ])
}

fn provider_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProviderRecord> {
    let settings: String = row.get(2)?;
    let meta: Option<String> = row.get(8)?;
    Ok(ProviderRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        settings_config: serde_json::from_str(&settings).map_err(json_from_sql_error)?,
        website_url: row.get(3)?,
        category: row.get(4)?,
        created_at: row.get(5)?,
        sort_index: row.get(6)?,
        notes: row.get(7)?,
        meta: meta
            .map(|value| serde_json::from_str(&value).map_err(json_from_sql_error))
            .transpose()?,
    })
}

fn json_from_sql_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn get_provider(
    connection: &Connection,
    app: &str,
    id: &str,
) -> Result<Option<ProviderRecord>, CoreError> {
    connection
        .query_row(
            "SELECT id, name, settings_config, website_url, category, created_at, sort_index, notes, meta
             FROM providers WHERE app = ?1 AND id = ?2",
            params![app, id],
            provider_from_row,
        )
        .optional()
        .map_err(CoreError::Database)
}

fn current_in_connection(connection: &Connection, app: &str) -> Result<Option<String>, CoreError> {
    Ok(connection
        .query_row(
            "SELECT provider_id FROM current_providers WHERE app = ?1",
            [app],
            |row| row.get(0),
        )
        .optional()?)
}

fn current_in_transaction(
    transaction: &Transaction<'_>,
    app: &str,
) -> Result<Option<String>, CoreError> {
    current_in_connection(transaction, app)
}

fn set_current(transaction: &Transaction<'_>, app: &str, id: &str) -> Result<(), CoreError> {
    transaction.execute(
        "INSERT INTO current_providers (app, provider_id) VALUES (?1, ?2)
         ON CONFLICT(app) DO UPDATE SET provider_id = excluded.provider_id",
        params![app, id],
    )?;
    Ok(())
}

fn next_sort_index(transaction: &Transaction<'_>, app: &str) -> Result<i64, CoreError> {
    Ok(transaction.query_row(
        "SELECT COALESCE(MAX(sort_index), -1) + 1 FROM providers WHERE app = ?1",
        [app],
        |row| row.get(0),
    )?)
}

fn write_live(state: &HeadlessState, app: &str, settings: &Value) -> Result<(), CoreError> {
    let path = match app {
        "claude" => state.home.join(".claude").join("settings.json"),
        "gemini" => state.home.join(".gemini").join("settings.json"),
        // Codex 的 auth/config 双文件和 GrokBuild 格式将在对应纵向测试中单独固定，不能把
        // 未验证的 JSON 直接写入真实配置位置。
        _ => return Err(CoreError::LiveWriteUnsupported(app.to_string())),
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| CoreError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let bytes = serde_json::to_vec_pretty(settings)?;
    std::fs::write(&path, bytes).map_err(|source| CoreError::Io { path, source })
}

fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("无界面状态锁已损坏")]
    StatePoisoned,
    #[error("不支持的应用类型: {0}")]
    UnsupportedApp(String),
    #[error("无界面模式尚未支持该应用的 live 写入: {0}")]
    LiveWriteUnsupported(String),
    #[error("供应商不存在: {0}")]
    ProviderNotFound(String),
    #[error("不能删除当前供应商")]
    CurrentProviderDeletion,
    #[error("当前阶段不支持修改供应商 ID")]
    ProviderIdChangeUnsupported,
    #[error("供应商配置无效: {0}")]
    InvalidProvider(String),
    #[error("数据库操作失败: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("JSON 操作失败: {0}")]
    Json(#[from] serde_json::Error),
    #[error("文件操作失败 {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderCommandError {
    #[error("远程命令未开放: {0}")]
    CommandNotExposed(String),
    #[error("远程命令参数无效: {0}")]
    InvalidArgument(String),
    #[error("远程业务执行失败: {0}")]
    Business(#[from] CoreError),
    #[error("远程结果序列化失败: {0}")]
    Serialization(serde_json::Error),
}

impl ProviderCommandError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::CommandNotExposed(_) => "COMMAND_NOT_EXPOSED",
            Self::InvalidArgument(_) => "INVALID_ARGUMENT",
            Self::Business(_) => "REMOTE_BUSINESS_ERROR",
            Self::Serialization(_) => "REMOTE_SERIALIZATION_ERROR",
        }
    }
}

impl From<RemoteCapabilityError> for ProviderCommandError {
    fn from(value: RemoteCapabilityError) -> Self {
        match value {
            RemoteCapabilityError::CommandNotExposed(command) => Self::CommandNotExposed(command),
        }
    }
}
