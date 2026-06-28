//! Headless web server entry point.
//!
//! Shared by the full desktop binary when invoked with `--headless` and by the
//! Docker `api-only` build. This path skips Tauri/GTK initialization entirely,
//! so it works without a display server.

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::web::{create_router, handlers::ws::WsState, models::app_state::AppState};

fn print_auth_token_once() {
    let already_printed = std::env::var("CC_SWITCH_AUTH_TOKEN_PRINTED")
        .map(|v| v == "1")
        .unwrap_or(false);

    if already_printed {
        return;
    }

    let token = crate::web::middleware::auth::get_auth_token();
    println!();
    println!("============================================================");
    println!("[cc-switch] AUTH_TOKEN (paste this on the web login page):");
    println!("{}", token);
    println!("============================================================");
    println!();
    println!("To rotate: cc-switch rotate-token");
    println!("To suppress this message: set CC_SWITCH_AUTH_TOKEN_PRINTED=1");

    std::env::set_var("CC_SWITCH_AUTH_TOKEN_PRINTED", "1");
}

pub fn run_headless() -> ! {
    let port: u16 = std::env::var("CC_SWITCH_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3001);

    let bind_all = std::env::var("CC_SWITCH_WEB_BIND_ALL")
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let bind_addr: [u8; 4] = if bind_all {
        [0, 0, 0, 0]
    } else {
        [127, 0, 0, 1]
    };
    let addr = SocketAddr::from((bind_addr, port));

    // Resolve database path: env override or default app config dir
    let db_path = std::env::var("CC_SWITCH_DB_PATH").unwrap_or_else(|_| {
        let dir = crate::config::get_app_config_dir();
        std::fs::create_dir_all(&dir).expect("Failed to create app config directory");
        dir.join("cc-switch.db")
            .to_str()
            .expect("db path is not valid UTF-8")
            .to_owned()
    });

    log::info!("=== cc-switch headless mode ===");
    log::info!("Listening on http://{}", addr);
    log::info!("Database: {}", db_path);

    print_auth_token_once();

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(async move {
        let web_state = Arc::new(AppState::new(&db_path).expect("Failed to initialize database"));

        let (tx, _rx) = broadcast::channel(100);
        let ws_state = Arc::new(WsState::new(tx));

        let router = create_router(web_state, ws_state);

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .unwrap_or_else(|e| panic!("Failed to bind to {addr}: {e}"));

        log::info!("Web server ready");

        axum::serve(listener, router)
            .await
            .expect("Web server exited with error");
    });

    std::process::exit(0);
}
