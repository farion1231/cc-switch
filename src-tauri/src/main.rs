// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 在 Linux 上设置 WebKit 环境变量以解决 DMA-BUF 渲染问题
    // 某些 Linux 系统（如 Debian 13.2、Nvidia GPU）上 WebKitGTK 的 DMA-BUF 渲染器可能导致白屏/黑屏
    // 参考: https://github.com/tauri-apps/tauri/issues/9394
    #[cfg(target_os = "linux")]
    {
        // 该规避针对的是 NVIDIA 驱动这一类环境（tauri#9394）。在现代 WebKitGTK +
        // Mesa 上（如 2.52 + Mesa 26 + AMD）强制禁用 DMABUF 反而会让回退的 EGL 路径
        // 初始化失败，WebProcess 启动即 abort、主窗口空白（#6514），因此只在这台
        // 机器加载了 NVIDIA 内核模块时才设置；用户预设仍然优先。
        if std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").is_err()
            && proc_modules_indicate_nvidia(
                &std::fs::read_to_string("/proc/modules").unwrap_or_default(),
            )
        {
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

/// 判断 /proc/modules 的内容是否表明加载了 NVIDIA 内核模块（闭源 `nvidia`
/// 及其 `nvidia_*` 子模块，或开源 `nouveau`）。纯函数便于跨平台单测；读不到
/// /proc/modules（如某些容器环境）时按未加载处理。
#[cfg(any(target_os = "linux", test))]
fn proc_modules_indicate_nvidia(modules: &str) -> bool {
    modules.lines().any(|line| {
        let module_name = line.split_whitespace().next().unwrap_or("");
        module_name == "nvidia" || module_name == "nouveau" || module_name.starts_with("nvidia_")
    })
}

#[cfg(test)]
mod tests {
    use super::proc_modules_indicate_nvidia;

    #[test]
    fn proprietary_nvidia_module_counts() {
        let proc_modules = concat!(
            "nvidia 61480960 180 nvidia_modeset,nvidia_uvm,nvidia_drm, Live 0x0 (OE)\n",
            "amdgpu 6148096 12 - Live 0x0\n",
        );
        assert!(proc_modules_indicate_nvidia(proc_modules));
    }

    #[test]
    fn nvidia_submodule_alone_counts() {
        let proc_modules = "nvidia_drm 122880 8 - Live 0x0 (E)\n";
        assert!(proc_modules_indicate_nvidia(proc_modules));
    }

    #[test]
    fn nouveau_counts() {
        let proc_modules = "nouveau 2785280 16 - Live 0x0\n";
        assert!(proc_modules_indicate_nvidia(proc_modules));
    }

    #[test]
    fn amd_and_intel_only_do_not_count() {
        let proc_modules = concat!(
            "amdgpu 6148096 12 - Live 0x0\n",
            "i915 3227648 20 - Live 0x0\n",
            "snd_hda_intel 122880 5 - Live 0x0\n",
        );
        assert!(!proc_modules_indicate_nvidia(proc_modules));
    }

    #[test]
    fn dependency_column_mentions_do_not_count() {
        // 只看每行第一个字段（模块名）；依赖列表等其它列里出现的 nvidia 字样不算。
        let proc_modules = "amdgpu 6148096 12 nvidia_something, Live 0x0\n";
        assert!(!proc_modules_indicate_nvidia(proc_modules));
    }

    #[test]
    fn empty_modules_do_not_count() {
        assert!(!proc_modules_indicate_nvidia(""));
    }
}
