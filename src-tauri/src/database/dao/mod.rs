//! Data Access Object layer
//!
//! Database access operations for each domain

pub mod custom_icons;
pub mod failover;
pub mod mcp;
pub mod profiles;
pub mod prompts;
pub mod providers;
pub mod providers_seed;
pub mod proxy;
pub mod settings;
pub mod skills;
pub mod stream_check;
pub mod universal_providers;
pub mod usage_rollup;

// 所有 DAO 方法都通过 Database impl 提供，无需单独导出
// 导出 FailoverQueueItem / Profile 供外部使用
pub use failover::FailoverQueueItem;
pub use profiles::Profile;

use crate::app_config::AppType;
use crate::error::AppError;
use indexmap::IndexMap;
use rusqlite::types::{ToSql, ToSqlOutput};

/// 解析一行里存的 JSON 列。解析失败就报错并点名是哪一行哪一列：不能换成默认值，
/// 否则下一次保存会把默认值写回去，盖掉库里原来的数据（比如新版本写入、旧版本认不出的字段）。
pub(crate) fn json_column<T: serde::de::DeserializeOwned>(
    row: &rusqlite::Row<'_>,
    index: usize,
    entry: &str,
) -> rusqlite::Result<T> {
    let raw: String = row.get(index)?;
    serde_json::from_str(&raw).map_err(|error| {
        let column = row.as_ref().column_name(index).unwrap_or("?").to_string();
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            format!("{entry} 的 {column} 无法解析，已保持原样未改动: {error}").into(),
        )
    })
}

/// 库里有、但存的 JSON 读不出来的一行（比如新版本写入、旧版本认不出的字段）。
/// 只给界面展示和删除：它没有能编辑、切换或写回的内容，库里的原始字节保持不动。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnreadableEntry {
    pub id: String,
    pub name: String,
    pub error: String,
}

/// 一次列表读取的结果：能读的行，和读不出来的行。
pub struct Rows<T> {
    pub readable: IndexMap<String, T>,
    pub unreadable: Vec<UnreadableEntry>,
}

impl<T> Rows<T> {
    /// 有读不出来的行就整体报错，点名第一行。会据此写库或写 live 的调用方用这个：
    /// 把读不出来的行当成「不存在」，就可能按同一个 id 新建一行盖掉它。
    pub fn into_strict(self) -> Result<IndexMap<String, T>, AppError> {
        match self.unreadable.into_iter().next() {
            Some(entry) => Err(AppError::Database(entry.error)),
            None => Ok(self.readable),
        }
    }

    /// 只读、不写的地方：读不出来的行记一条警告后跳过，不拖累其余各行。
    pub fn into_readable(self, purpose: &str) -> IndexMap<String, T> {
        for entry in &self.unreadable {
            log::warn!("{purpose}: 跳过读不出来的 {}: {}", entry.id, entry.error);
        }
        self.readable
    }

    /// 导入用：能读的行，加上读不出来的那些 id。导入要跳过这些 id 并报告，
    /// 不能当成「没有」再按同一个 id 新建一行盖掉它。
    pub fn into_readable_and_unreadable_ids(self) -> (IndexMap<String, T>, Vec<String>) {
        let ids = self.unreadable.into_iter().map(|entry| entry.id).collect();
        (self.readable, ids)
    }

    /// 只要其中一行：它本身读不出来就报错并点名，别的行坏了不影响。
    pub fn get(&self, id: &str) -> Result<Option<&T>, AppError> {
        if let Some(entry) = self.unreadable.iter().find(|entry| entry.id == id) {
            return Err(AppError::Database(entry.error.clone()));
        }
        Ok(self.readable.get(id))
    }
}

/// 读一行；[`json_column`] 报的解析失败变成 [`UnreadableEntry`]（`id`、`name` 在第 0、1 列），
/// 其他 SQL 错误照常上抛。
pub(crate) fn read_row<T>(
    row: &rusqlite::Row<'_>,
    decode: impl FnOnce(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> rusqlite::Result<Result<T, UnreadableEntry>> {
    match decode(row) {
        Ok(value) => Ok(Ok(value)),
        Err(rusqlite::Error::FromSqlConversionFailure(_, _, error)) => Ok(Err(UnreadableEntry {
            id: row.get(0)?,
            name: row.get(1)?,
            error: error.to_string(),
        })),
        Err(error) => Err(error),
    }
}

/// 库里 `app_type` 列存的就是 [`AppType::as_str`]，DAO 直接绑定 `&AppType`。
impl ToSql for AppType {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        self.as_str().to_sql()
    }
}
