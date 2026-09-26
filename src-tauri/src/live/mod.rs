//! 所有权层：CC Switch 只拥有客户端配置里的关键字段，其余字节不碰。
//!
//! - `floor`：每个应用的关键字段和供应商独有字段（纯函数谓词和常量清单）；
//! - `patch`：按格式保序改写（JSON、TOML、`.env`），解析不了就停；
//! - `engine`：写锁、计划、临时文件、首写备份、重读比对。

pub mod engine;
pub mod floor;
pub mod patch;
