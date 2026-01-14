// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    // Parse CLI arguments first
    let cli = cc_switch_lib::cli::Cli::parse();

    // Check if CLI mode is requested
    if let Some(cc_switch_lib::cli::CliCommand::Cmd { action }) = cli.command {
        // CLI mode: skip GUI, run headless
        return cc_switch_lib::cli::run_cli(action);
    }

    // 在 Linux 上设置 WebKit 环境变量以解决 DMA-BUF 渲染问题
    // 某些 Linux 系统（如 Debian 13.2、Nvidia GPU）上 WebKitGTK 的 DMA-BUF 渲染器可能导致白屏/黑屏
    // 参考: https://github.com/tauri-apps/tauri/issues/9394
    #[cfg(target_os = "linux")]
    {
        if std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").is_err() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    // Normal GUI mode
    cc_switch_lib::run();
    ExitCode::SUCCESS
}
