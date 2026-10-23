//! REST 控制面配置（`RestConfig`）。
//!
//! 此模块**始终编译**（不受 `rest_api` feature 影响），因为 `RestConfig`
//! 是 `AppSettings.rest_api` 字段的类型，需在 feature off 时也可序列化。
//! 实际的 REST 服务器（`service`/`routes`/`auth`/`error`）在 `rest_api`
//! feature 开启时才编译。

use serde::{Deserialize, Serialize};

/// REST 控制面配置。存于 `settings.json` 的 `restApi` 段（`AppSettings.rest_api`）。
///
/// - `enabled`：总开关。`rest_api` feature on 且 `enabled=true` 时，`setup` 自动起 listener。
/// - `listen_address`：默认 `127.0.0.1`；可设 `0.0.0.0`（此时 token 强制非空）。
/// - `listen_port`：默认 `8787`。
/// - `token`：Bearer token；空则首次启用时由 `set_rest_config` 生成。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_listen_address")]
    pub listen_address: String,
    #[serde(default = "default_listen_port")]
    pub listen_port: u16,
    #[serde(default)]
    pub token: String,
}

impl Default for RestConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen_address: default_listen_address(),
            listen_port: default_listen_port(),
            token: String::new(),
        }
    }
}

fn default_listen_address() -> String {
    "127.0.0.1".to_string()
}

fn default_listen_port() -> u16 {
    8787
}

impl RestConfig {
    /// 绑定 `0.0.0.0` 时 token 必须非空（R-010/A-012）。
    pub fn is_bind_all_without_token(&self) -> bool {
        self.listen_address == "0.0.0.0" && self.token.trim().is_empty()
    }

    /// 绑定 socket 地址。
    pub fn bind_addr(&self) -> Result<std::net::SocketAddr, String> {
        let addr: std::net::IpAddr = self
            .listen_address
            .parse()
            .map_err(|e| format!("无效监听地址 {}: {e}", self.listen_address))?;
        Ok(std::net::SocketAddr::new(addr, self.listen_port))
    }
}
