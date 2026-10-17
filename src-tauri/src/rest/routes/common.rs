//! Phase 2 路由共享辅助。

use std::str::FromStr;

use serde::Deserialize;

use crate::app_config::AppType;
use crate::rest::error::RestError;

#[derive(Deserialize)]
pub struct AppQuery {
    pub app: String,
}

pub fn parse_app(app: &str) -> Result<AppType, RestError> {
    AppType::from_str(app).map_err(|e| RestError::Bad(format!("无效应用类型: {e}")))
}
