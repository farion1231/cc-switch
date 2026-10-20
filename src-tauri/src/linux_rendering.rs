//! Linux 的 WebKitGTK 渲染策略，必须在 GTK / WebKit 初始化前配置。
//!
//! WebKitGTK 2.44+ 使用合成 + SHM 保留透明窗口，旧版默认沿用软件绘制。
//! 用户仍可用 WebKit 原有环境变量覆盖；禁用渲染器或合成时面板退回不透明。

use std::ffi::OsStr;

const DISABLE_DMABUF: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
const DISABLE_COMPOSITING: &str = "WEBKIT_DISABLE_COMPOSITING_MODE";

/// 只补齐未设置的值；这组参数作用于整个进程，而非单个 WebView。
pub fn configure_environment() {
    // SAFETY: 版本查询只返回所加载库的版本常量，不需要初始化 GTK / WebKit。
    let (major, minor) = unsafe {
        (
            webkit2gtk::ffi::webkit_get_major_version(),
            webkit2gtk::ffi::webkit_get_minor_version(),
        )
    };
    for (name, value) in environment_defaults(major, minor) {
        if std::env::var_os(name).is_none() {
            std::env::set_var(name, value);
        }
    }
}

fn environment_defaults(major: u32, minor: u32) -> [(&'static str, &'static str); 3] {
    // TODO: 上游透明软件绘制和 DMA-BUF 驱动兼容性修复后，重新评估默认路径。
    // 当前不能简单禁用整个 renderer：WebKitGTK 2.54.1 的透明软件绘制会丢失局部内容。
    // 关联 #8556：禁用软件绘制的透明路径，使用 SHM 合成保留透明窗口。
    // 2.42 不识别 FORCE_SHM，不能用它替代原有的禁用参数；从稳定版 2.44 起才默认启用。
    let disable = if (major, minor) >= (2, 44) { "0" } else { "1" };
    [
        (DISABLE_DMABUF, disable),
        (DISABLE_COMPOSITING, disable),
        ("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1"),
    ]
}

fn compositing_requested(
    disable_dmabuf: Option<&OsStr>,
    disable_compositing: Option<&OsStr>,
) -> bool {
    // 与 WebKit 一致：变量存在且不等于 "0" 即禁用，空字符串也算禁用。
    [disable_dmabuf, disable_compositing]
        .into_iter()
        .all(|value| value.is_none_or(|value| value == "0"))
}

/// 主窗口与面板必须遵循相同的合成开关；WebView 级别的 Never 会绕开 SHM 合成。
pub(crate) fn compositing_enabled() -> bool {
    compositing_requested(
        std::env::var_os(DISABLE_DMABUF).as_deref(),
        std::env::var_os(DISABLE_COMPOSITING).as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_webkit_defaults_to_opaque_software_rendering() {
        for (major, minor) in [(2, 40), (2, 42), (2, 43)] {
            let defaults = environment_defaults(major, minor);
            assert_eq!(defaults[0], (DISABLE_DMABUF, "1"));
            assert_eq!(defaults[1], (DISABLE_COMPOSITING, "1"));
            assert!(!compositing_requested(
                Some(OsStr::new(defaults[0].1)),
                Some(OsStr::new(defaults[1].1)),
            ));
        }
    }

    #[test]
    fn supported_webkit_defaults_to_transparent_shm_compositing() {
        for (major, minor) in [(2, 44), (2, 54), (3, 0)] {
            let defaults = environment_defaults(major, minor);
            assert_eq!(defaults[0], (DISABLE_DMABUF, "0"));
            assert_eq!(defaults[1], (DISABLE_COMPOSITING, "0"));
            assert_eq!(defaults[2], ("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1"));
            assert!(compositing_requested(
                Some(OsStr::new(defaults[0].1)),
                Some(OsStr::new(defaults[1].1)),
            ));
        }
    }

    #[test]
    fn compositing_follows_effective_webkit_overrides() {
        for (dmabuf, compositing, transparent) in [
            (None, None, true),
            (Some("0"), Some("0"), true),
            (Some("0"), None, true),
            (None, Some("0"), true),
            (Some("1"), Some("0"), false),
            (Some("0"), Some("1"), false),
            (Some("1"), Some("1"), false),
            (Some(""), Some("0"), false),
            (Some("0"), Some("false"), false),
        ] {
            assert_eq!(
                compositing_requested(dmabuf.map(OsStr::new), compositing.map(OsStr::new)),
                transparent,
                "DMA-BUF override: {dmabuf:?}, compositing override: {compositing:?}"
            );
        }
    }
}
