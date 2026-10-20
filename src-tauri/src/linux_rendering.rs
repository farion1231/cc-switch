//! Linux 的 WebKitGTK 渲染策略，必须在 GTK / WebKit 初始化前配置。
//!
//! 使用合成 + SHM 保留透明窗口，同时避开部分驱动的 DMA-BUF 协商错误。
//! 用户仍可用 WebKit 原有环境变量覆盖；禁用渲染器或合成时面板退回不透明。

use std::ffi::OsStr;

const DISABLE_DMABUF: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
const DISABLE_COMPOSITING: &str = "WEBKIT_DISABLE_COMPOSITING_MODE";

/// 只补齐未设置的值；这组参数作用于整个进程，而非单个 WebView。
pub fn configure_environment() {
    // TODO: 上游透明软件绘制和 DMA-BUF 驱动兼容性修复后，重新评估默认路径。
    // 当前不能简单禁用整个 renderer：WebKitGTK 2.54.1 的透明软件绘制会丢失局部内容。
    // 关联 #8556：禁用软件绘制的透明路径，使用 SHM 合成保留透明窗口。
    for (name, value) in [
        (DISABLE_DMABUF, "0"),
        (DISABLE_COMPOSITING, "0"),
        ("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1"),
    ] {
        if std::env::var_os(name).is_none() {
            std::env::set_var(name, value);
        }
    }
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
