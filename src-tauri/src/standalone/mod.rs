//! Headless standalone 运行入口：不依赖 Tauri，启动本地代理 + 管理 API。
//!
//! 作为 lib 内部模块，可访问 `pub(crate)` 项（`ProxyServer` / `Database` 等）。

pub mod admin;

use std::sync::Arc;

use crate::database::Database;
use crate::proxy::server::ProxyServer;
use crate::proxy::types::ProxyConfig;

/// CLI 参数。
struct CliArgs {
    db_path: std::path::PathBuf,
    address: String,
    port: u16,
}

/// 解析 CLI 参数（手写，避免引入 clap）。
///
/// 支持：`--db <path>`、`--address <ip>`、`--port <num>`、`-h/--help`。
/// 解析失败返回 `None`（调用方走退出码 2）。
fn parse_cli_args() -> Option<CliArgs> {
    let mut db_path = dirs::config_dir()
        .map(|d| d.join("cc-switch").join("cli-proxy.db"))
        .unwrap_or_else(|| std::path::PathBuf::from("cli-proxy.db"));
    let mut address = "127.0.0.1".to_string();
    let mut port: u16 = 15721;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => {
                let Some(v) = args.next() else {
                    eprintln!("--db 需要参数");
                    return None;
                };
                if v.is_empty() {
                    eprintln!("--db 不能为空字符串");
                    return None;
                }
                db_path = std::path::PathBuf::from(v);
            }
            "--address" => {
                let Some(v) = args.next() else {
                    eprintln!("--address 需要参数");
                    return None;
                };
                address = v;
            }
            "--port" => {
                let Some(v) = args.next() else {
                    eprintln!("--port 需要参数");
                    return None;
                };
                port = match v.parse() {
                    Ok(n) => n,
                    Err(_) => {
                        eprintln!("--port 不是合法数字: {v}");
                        return None;
                    }
                };
            }
            "-h" | "--help" => {
                eprintln!(
                    "用法: cc-switch-proxy [--db <path>] [--address <ip>] [--port <num>]\n\n\
                     启动本地代理（Codex 协议转换）+ 管理 API。\n\
                     默认监听 127.0.0.1:15721，DB: ~/.config/cc-switch/cli-proxy.db"
                );
                return None;
            }
            other => {
                eprintln!("未知参数: {other}");
                return None;
            }
        }
    }

    Some(CliArgs {
        db_path,
        address,
        port,
    })
}

/// 归一化监听地址，确保 `ProxyServer::start` 的 `format!("{addr}:{port}")` 能解析为合法 `SocketAddr`。
///
/// - `localhost` → `127.0.0.1`（`SocketAddr` 不解析主机名）
/// - IPv6（如 `::1`）→ `[::1]`（IPv6 字面量必须用方括号）
/// - IPv4 / 已带方括号 → 原样
fn normalize_listen_address(addr: &str) -> String {
    if addr == "localhost" {
        "127.0.0.1".to_string()
    } else if addr.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("[{addr}]")
    } else {
        addr.to_string()
    }
}

/// 启动 standalone 代理。返回进程退出码（见 spec §9）。
pub async fn run() -> i32 {
    let Some(args) = parse_cli_args() else {
        return 2;
    };

    // 安全：admin API 无鉴权，强制监听回环地址（设计文档 §12）。
    let is_loopback = args.address == "localhost"
        || args
            .address
            .parse::<std::net::IpAddr>()
            .map(|a| a.is_loopback())
            .unwrap_or(false);
    if !is_loopback {
        eprintln!(
            "[cc-switch-proxy] 拒绝启动：--address {} 非回环地址。admin API 无鉴权，仅允许 127.0.0.1/localhost。",
            args.address
        );
        return 2;
    }

    let db = match Database::open_at(&args.db_path) {
        Ok(db) => Arc::new(db),
        Err(e) => {
            eprintln!(
                "[cc-switch-proxy] 数据库初始化失败 ({}): {e}",
                args.db_path.display()
            );
            return 3;
        }
    };

    let config = ProxyConfig {
        listen_address: normalize_listen_address(&args.address),
        listen_port: args.port,
        ..ProxyConfig::default()
    };

    let admin_router = admin::build_admin_router();
    let server = ProxyServer::new(config, db, None).with_extra_routes(admin_router);

    let info = match server.start().await {
        Ok(info) => info,
        Err(e) => {
            eprintln!("[cc-switch-proxy] 代理启动失败: {e}");
            return 4;
        }
    };

    log::info!(
        "[cc-switch-proxy] 已启动：http://{}:{}  （DB: {}）",
        info.address,
        info.port,
        args.db_path.display()
    );
    eprintln!(
        "[cc-switch-proxy] 监听 http://{}:{}  管理 API: POST http://127.0.0.1:{}/admin/providers",
        info.address, info.port, info.port
    );

    // 等待停止信号（Ctrl-C / SIGTERM）
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(e) => {
                log::warn!("[cc-switch-proxy] SIGTERM handler 安装失败，仅依赖 Ctrl-C: {e}");
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }

    log::info!("[cc-switch-proxy] 收到停止信号，正在关闭…");
    if let Err(e) = server.stop().await {
        eprintln!("[cc-switch-proxy] 停止异常: {e}");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_listen_address_handles_variants() {
        assert_eq!(normalize_listen_address("localhost"), "127.0.0.1");
        assert_eq!(normalize_listen_address("127.0.0.1"), "127.0.0.1");
        assert_eq!(normalize_listen_address("::1"), "[::1]");
        assert_eq!(normalize_listen_address("fe80::1"), "[fe80::1]");

        // 归一化后拼端口必须能 parse 成合法 SocketAddr（回归 guard）
        for addr in ["127.0.0.1", "::1"] {
            let norm = normalize_listen_address(addr);
            let joined = format!("{norm}:15721");
            assert!(
                joined.parse::<std::net::SocketAddr>().is_ok(),
                "{joined} 应为合法 SocketAddr"
            );
        }
    }
}
