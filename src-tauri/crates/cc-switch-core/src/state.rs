use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;

use crate::error::CoreError;
use crate::schema::{
    configure_connection, initialize_new_database, read_schema_version, validate_existing_database,
};

/// Agent 可安全构造的无界面状态，只持有目标主机数据库与显式 HOME。
///
/// HOME 不从进程全局环境变量重复读取，防止测试、桌面适配和并发远程会话互相污染。
pub struct HeadlessState {
    connection: Mutex<Connection>,
    home: PathBuf,
}

impl HeadlessState {
    /// 打开目标 HOME 的真实数据库；新库可初始化，已有库只校验而绝不执行迁移 DDL。
    pub fn open(home: impl AsRef<Path>) -> Result<Self, CoreError> {
        let home = home.as_ref().to_path_buf();
        let data_dir = home.join(".cc-switch");
        std::fs::create_dir_all(&data_dir).map_err(|source| CoreError::Io {
            path: data_dir.clone(),
            source,
        })?;
        let database_path = data_dir.join("cc-switch.db");
        let is_new = !database_path.exists();
        let connection = Connection::open(&database_path)?;
        configure_connection(&connection)?;
        if is_new {
            initialize_new_database(&connection)?;
        } else {
            validate_existing_database(&connection)?;
        }
        Ok(Self {
            connection: Mutex::new(connection),
            home,
        })
    }

    /// 构造与磁盘新库相同 schema 的隔离状态，供事务和协议测试使用。
    pub fn memory(home: impl AsRef<Path>) -> Result<Self, CoreError> {
        let connection = Connection::open_in_memory()?;
        configure_connection(&connection)?;
        initialize_new_database(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
            home: home.as_ref().to_path_buf(),
        })
    }

    /// 公开只读连接闭包供桌面适配层复用 Core 查询，避免泄漏锁守卫和连接所有权。
    pub fn with_connection<T>(
        &self,
        operation: impl FnOnce(&Connection) -> Result<T, CoreError>,
    ) -> Result<T, CoreError> {
        let connection = self.connection()?;
        operation(&connection)
    }

    /// 读取当前 schema 版本，供握手兼容判断和诊断使用。
    pub fn schema_version(&self) -> Result<i32, CoreError> {
        self.with_connection(|connection| Ok(read_schema_version(connection)?))
    }

    /// 返回显式目标 HOME；live writer 只能基于此路径，不能再次读取进程环境变量。
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// Provider/Usage 写事务仍由各领域服务管理；仅在 crate 内暴露锁守卫。
    pub(crate) fn connection(&self) -> Result<MutexGuard<'_, Connection>, CoreError> {
        self.connection.lock().map_err(|_| CoreError::StatePoisoned)
    }
}
