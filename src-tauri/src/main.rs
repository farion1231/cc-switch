// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
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

        // AppImage 的 GTK 启动钩子 (linuxdeploy-plugin-gtk.sh) 还会无条件
        // 导出 GTK_IM_MODULE_FILE（指向 bundle 的 immodules.cache）和
        // GTK_EXE_PREFIX（指向 AppDir），而该 bundle 缓存只含 GTK 自带输入法
        // 模块、没有任何 fcitx/ibus 条目。用户配了 GTK_IM_MODULE=fcitx 时
        // GTK 查无此模块会回退 XIM，WebKitGTK 文本框一获得焦点就卡死整个
        // 应用（见 issue #7746）。注意：钩子是同一套 export，所以只摘掉
        // GTK_IM_MODULE_FILE 没用——GTK 会顺着 GTK_EXE_PREFIX 回到同一份
        // bundle 缓存（或被引到一个不存在的 multiarch 路径）。这里在 GTK
        // 初始化前判定：仅当变量确实指向 AppDir 内的 bundle 缓存、且该缓存
        // 服务不了用户配置的输入法模块时，才把变量改指向主机上真正列出该
        // 模块的缓存；一台都找不到则保持原状（其余情况一律不动，零回归）。
        // 逃生开关与上面 CC_SWITCH_GDK_BACKEND 同风格，钩子不会触碰它：
        //   CC_SWITCH_GTK_IM_MODULE_FILE=keep     强制保持钩子写入的路径
        //   CC_SWITCH_GTK_IM_MODULE_FILE=<path>   强制改写为指定缓存路径
        cc_switch_lib::apply_gtk_im_module_file_fix();
    }

    cc_switch_lib::run();
}
