//! 代理模块日志错误码定义
//!
//! 格式: [模块-编号] 消息
//! - CB: Circuit Breaker (熔断器)
//! - SRV: Server (服务器)
//! - FWD: Forwarder (转发器)

/// 熔断器日志码
pub mod cb {
    pub const OPEN_TO_HALF_OPEN: &str = "CB-001";
    pub const HALF_OPEN_TO_CLOSED: &str = "CB-002";
    pub const HALF_OPEN_PROBE_FAILED: &str = "CB-003";
    pub const TRIGGERED_FAILURES: &str = "CB-004";
    pub const TRIGGERED_ERROR_RATE: &str = "CB-005";
    #[cfg(test)]
    pub const MANUAL_RESET: &str = "CB-006";
}

/// 服务器日志码
pub mod srv {
    pub const STARTED: &str = "SRV-001";
    pub const STOPPED: &str = "SRV-002";
    pub const STOP_TIMEOUT: &str = "SRV-003";
    pub const TASK_ERROR: &str = "SRV-004";
    pub const ACCEPT_ERR: &str = "SRV-005";
    pub const CONN_ERR: &str = "SRV-006";
}

/// 转发器日志码
pub mod fwd {
    pub const PROVIDER_FAILED_RETRY: &str = "FWD-001";
    pub const ALL_PROVIDERS_FAILED: &str = "FWD-002";
    pub const SINGLE_PROVIDER_FAILED: &str = "FWD-003";
    pub const CIRCUIT_SKIPPED: &str = "FWD-004";
}
