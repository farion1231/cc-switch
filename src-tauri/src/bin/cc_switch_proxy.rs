//! cc-switch-proxy：headless 本地代理（Codex 协议转换）+ 管理 API，不依赖 Tauri。
//!
//! 薄壳：初始化日志后转调 `cc_switch_lib::standalone::run()`。

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .init();

    let exit_code = cc_switch_lib::standalone::run().await;
    std::process::exit(exit_code);
}
