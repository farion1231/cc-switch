// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 在 Linux 上设置 WebKit 环境变量以解决 DMA-BUF 渲染问题
    // 某些 Linux 系统（如 Debian 13.2、Nvidia GPU）上 WebKitGTK 的 DMA-BUF 渲染器可能导致白屏/黑屏
    // 参考: https://github.com/tauri-apps/tauri/issues/9394
    #[cfg(target_os = "linux")]
    {
        // 规避 AppImage 打包的旧版 libwayland-client 与宿主 Mesa/Wayland 的 EGL 冲突 (#8723, #7335)
        // linuxdeploy 打包的旧版 libwayland-client.so.0 排在 LD_LIBRARY_PATH 最前，
        // 导致 WebKitWebProcess 在较新发行版（Arch / openSUSE / Fedora / Ubuntu 24.04+）上启动即报错：
        // Could not create default EGL display: EGL_BAD_PARAMETER. Aborting...
        // 在检测到 Wayland 会话与 AppImage 环境时，预加载宿主系统的原生 libwayland-client.so.0 即可彻底解决此崩溃。
        if std::env::var("_CC_SWITCH_WAYLAND_PRELOADED").is_err() && std::env::var("APPIMAGE").is_ok() {
            let is_wayland = std::env::var("WAYLAND_DISPLAY").is_ok()
                || std::env::var("XDG_SESSION_TYPE")
                    .map(|s| s.eq_ignore_ascii_case("wayland"))
                    .unwrap_or(false);

            if is_wayland {
                let candidates = [
                    "/usr/lib64/libwayland-client.so.0",
                    "/usr/lib/x86_64-linux-gnu/libwayland-client.so.0",
                    "/usr/lib/libwayland-client.so.0",
                    "/usr/lib/aarch64-linux-gnu/libwayland-client.so.0",
                ];
                if let Some(host_wayland) = candidates.iter().find(|p| std::path::Path::new(p).exists()) {
                    use std::os::unix::process::CommandExt;
                    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap_or_default());
                    cmd.args(std::env::args_os().skip(1));
                    cmd.env("_CC_SWITCH_WAYLAND_PRELOADED", "1");
                    let new_preload = match std::env::var("LD_PRELOAD") {
                        Ok(existing) if !existing.is_empty() => format!("{}:{}", host_wayland, existing),
                        _ => host_wayland.to_string(),
                    };
                    cmd.env("LD_PRELOAD", new_preload);
                    let _ = cmd.exec();
                }
            }
        }

        if std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").is_err() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
        // 禁用 WebKitGTK 合成模式，规避 resize 时 webview 崩溃以及部分 Wayland
        // 合成器下的 surface 协商问题（整窗 UI 点击无响应、必须最大化-还原才能恢复）。
        // 参考: https://github.com/tauri-apps/tauri/issues/9394
        if std::env::var("WEBKIT_DISABLE_COMPOSITING_MODE").is_err() {
            std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        }

        // AppImage 的 GTK 启动钩子 (linuxdeploy-plugin-gtk.sh) 会无条件
        // `export GDK_BACKEND=x11` 强制走 XWayland，以规避历史上的 Wayland 崩溃
        // (tauri-apps/tauri#8541)。但在较新的 Wayland + NVIDIA 环境下，强制 XWayland
        // 反而使 WebKitGTK 的 webview 收不到指针事件（标题栏可点、网页内容点不动），
        // resize 后黑屏；改回原生 Wayland 即可解决，且该崩溃在 WebKitGTK 2.52 上已不复现。
        // 由于该钩子会覆盖用户预设的 GDK_BACKEND，这里提供一个钩子不会触碰的逃生开关：
        // 设置 CC_SWITCH_GDK_BACKEND=wayland 即可强制覆盖，默认行为保持不变（零回归）。
        if let Ok(backend) = std::env::var("CC_SWITCH_GDK_BACKEND") {
            if !backend.is_empty() {
                std::env::set_var("GDK_BACKEND", backend);
            }
        }
    }

    cc_switch_lib::run();
}
