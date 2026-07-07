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
                db_path = std::path::PathBuf::from(v);
            }
            "--address" => {
                address = args.next().unwrap_or(address);
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

/// 启动 standalone 代理。返回进程退出码（见 spec §9）。
pub async fn run() -> i32 {
    let Some(args) = parse_cli_args() else {
        return 2;
    };

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
        listen_address: args.address.clone(),
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
        let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
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
