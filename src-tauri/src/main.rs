// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
fn appimage_im_module_cache_contains(cache: &str, module: &str) -> bool {
    cache.lines().any(|line| {
        line.split_whitespace()
            .any(|token| token.trim_matches('"') == module)
    })
}

#[cfg(target_os = "linux")]
const HOST_IM_MODULE_CACHES: [&str; 4] = [
    "/usr/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules.cache",
    "/usr/lib/aarch64-linux-gnu/gtk-3.0/3.0.0/immodules.cache",
    "/usr/lib64/gtk-3.0/3.0.0/immodules.cache",
    "/usr/lib/gtk-3.0/3.0.0/immodules.cache",
];

#[cfg(target_os = "linux")]
fn first_existing_im_module_cache<'a>(
    paths: &'a [&'a std::path::Path],
) -> Option<&'a std::path::Path> {
    paths.iter().copied().find(|path| path.is_file())
}

#[cfg(target_os = "linux")]
fn fix_appimage_im_module_cache() {
    // linuxdeploy-plugin-gtk points AppImage GTK at a bundled cache that may
    // not contain the input method selected by the user's desktop session.
    if std::env::var_os("APPIMAGE").is_none() {
        return;
    }

    let Some(module) = std::env::var_os("GTK_IM_MODULE") else {
        return;
    };
    let Some(cache_path) = std::env::var_os("GTK_IM_MODULE_FILE") else {
        return;
    };
    let module = module.to_string_lossy();

    let cache = std::fs::read_to_string(&cache_path).ok();
    if cache
        .as_deref()
        .is_some_and(|cache| appimage_im_module_cache_contains(cache, &module))
    {
        return;
    }

    let host_caches: Vec<_> = HOST_IM_MODULE_CACHES
        .iter()
        .map(std::path::Path::new)
        .collect();
    if let Some(host_cache) = first_existing_im_module_cache(&host_caches) {
        // GTK_EXE_PREFIX makes an unset GTK_IM_MODULE_FILE resolve back to the
        // bundled cache, so point GTK at a host cache explicitly.
        std::env::set_var("GTK_IM_MODULE_FILE", host_cache);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::{appimage_im_module_cache_contains, first_existing_im_module_cache};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn detects_input_method_module_in_cache() {
        let cache = r#""/usr/lib/gtk/immodules/im-fcitx5.so" "fcitx5" "Fcitx5""#;
        assert!(appimage_im_module_cache_contains(cache, "fcitx5"));
        assert!(!appimage_im_module_cache_contains(cache, "ibus"));
    }

    #[test]
    fn selects_host_cache_when_bundled_cache_omits_input_method() {
        let dir = tempdir().unwrap();
        let bundled_cache = r#""/usr/lib/gtk/immodules/im-xim.so" "xim" "X Input Method""#;
        assert!(!appimage_im_module_cache_contains(bundled_cache, "fcitx5"));

        let host_cache = dir.path().join("immodules.cache");
        fs::write(&host_cache, "fcitx5").unwrap();
        let missing_cache = dir.path().join("missing.cache");
        let candidates = [missing_cache.as_path(), host_cache.as_path()];

        assert_eq!(
            first_existing_im_module_cache(&candidates),
            Some(host_cache.as_path())
        );
    }
}

fn main() {
    // 在 Linux 上设置 WebKit 环境变量以解决 DMA-BUF 渲染问题
    // 某些 Linux 系统（如 Debian 13.2、Nvidia GPU）上 WebKitGTK 的 DMA-BUF 渲染器可能导致白屏/黑屏
    // 参考: https://github.com/tauri-apps/tauri/issues/9394
    #[cfg(target_os = "linux")]
    {
        fix_appimage_im_module_cache();

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
