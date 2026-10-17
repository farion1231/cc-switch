//! REST 控制面模块。
//!
//! - `config`（`RestConfig`）**始终编译**：它是 `AppSettings.rest_api` 字段类型，
//!   feature off 时也需可序列化。
//! - `service`/`routes`/`auth`/`error` 仅在 `rest_api` feature 开启时编译。

pub mod config;

#[cfg(feature = "rest_api")]
pub mod auth;
#[cfg(feature = "rest_api")]
pub mod commands;
#[cfg(feature = "rest_api")]
pub mod error;
#[cfg(feature = "rest_api")]
pub mod openapi;
#[cfg(feature = "rest_api")]
pub mod routes;
#[cfg(feature = "rest_api")]
pub mod service;
#[cfg(feature = "rest_api")]
pub mod web;

pub use config::RestConfig;
