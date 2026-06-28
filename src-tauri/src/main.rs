// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // CLI subcommand: rotate-token. Runs before any Tauri/GTK init so it works
    // in both desktop and api-only builds without a display server.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "rotate-token") {
        let new_token = cc_switch_lib::rotate_auth_token();
        println!("New AUTH_TOKEN: {}", new_token);
        println!();
        println!("To use this token:");
        println!("  1. Restart the cc-switch server for the new token to take effect.");
        println!("  2. Paste the token on the web login page.");
        return;
    }

    // API-only mode: activated at Docker build time via --features api-only.
    // Skips Tauri/GTK entirely — no display server required.
    #[cfg(feature = "api-only")]
    cc_switch_lib::api_only::run(); // never returns (-> !)

    // Full desktop mode (default build).
    #[cfg(not(feature = "api-only"))]
    {
        // 在 Linux 上设置 WebKit 环境变量以解决 DMA-BUF 渲染问题
        // 某些 Linux 系统（如 Debian 13.2、Nvidia GPU）上 WebKitGTK 的 DMA-BUF 渲染器可能导致白屏/黑屏
        // 参考: https://github.com/tauri-apps/tauri/issues/9394
        #[cfg(target_os = "linux")]
        {
            if std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").is_err() {
                std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
            }
            // 禁用 WebKitGTK 合成模式，规避 resize 时 webview 崩溃以及部分 Wayland
            // 合成器下的 surface 协商问题（整窗 UI 点击无响应、必须最大化-还原才能恢复）。
            // 参考: https://github.com/tauri-apps/tauri/issues/9394
            if std::env::var("WEBKIT_DISABLE_COMPOSITING_MODE").is_err() {
                std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
            }
        }

        cc_switch_lib::run();
    }
}
