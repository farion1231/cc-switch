//! 数据访问对象 (DAO) 模块
//!
//! 提供各类数据的 CRUD 操作。

mod mcp;
mod prompts;
mod providers;
mod settings;
mod skills;
mod universal_providers;

// 所有 DAO 方法都通过 Database impl 提供，无需单独导出
