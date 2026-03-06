//! Proxy server module
//!
//! This module provides the core proxy server functionality including:
//! - HTTP request forwarding
//! - Health checking
//! - Circuit breaker
//! - Failover management

pub mod circuit_breaker;
pub mod health;

pub use circuit_breaker::CircuitBreaker;
pub use health::HealthChecker;
