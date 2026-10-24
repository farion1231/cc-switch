// 轻量平台检测，避免在 SSR 或无 navigator 的环境报错
export const isMac = (): boolean => {
  try {
    const ua = navigator.userAgent || "";
    const plat = (navigator.platform || "").toLowerCase();
    return /mac/i.test(ua) || plat.includes("mac");
  } catch {
    return false;
  }
};

export const isWindows = (): boolean => {
  try {
    const ua = navigator.userAgent || "";
    return /windows|win32|win64/i.test(ua);
  } catch {
    return false;
  }
};

export const isLinux = (): boolean => {
  try {
    const ua = navigator.userAgent || "";
    // WebKitGTK/Chromium 在 Linux/Wayland/X11 下 UA 通常包含 Linux 或 X11
    return (
      /linux|x11/i.test(ua) && !/android/i.test(ua) && !isMac() && !isWindows()
    );
  } catch {
    return false;
  }
};

// 三端都启用 Tauri 的事件拖拽。
//
// Linux 之前因 Tauri #13440 整组禁用 drag region，但那个 issue 实际是 Tao
// 的 Wayland CSD EventBox 吞掉了原生标题栏按钮的点击，与拖动无关；禁用的
// 结果是「关闭系统装饰 + 使用应用内按钮」时窗口完全无法移动。点击问题已在
// linux_fix 里单独修补，这里恢复拖动。
//
// Windows 仍只使用 Tauri 的事件拖拽，不启用原生拖拽 CSS，避免 WebView2
// 更新拖拽区域时持续消耗主线程 CPU 或挂起。macOS 保留原生拖拽 CSS。
//
// 这些常量设计为通过 JSX 属性 spread 消费（`{...DRAG_REGION_ATTR}`）。
export const DRAG_REGION_ENABLED = true;

export const DRAG_REGION_ATTR: Record<string, unknown> = DRAG_REGION_ENABLED
  ? { "data-tauri-drag-region": true }
  : {};

// 与 index.css 中的 .is-mac 规则保持一致：原生拖拽 CSS 只用于 macOS。
export const DRAG_REGION_STYLE: Record<string, unknown> = isMac()
  ? { WebkitAppRegion: "drag" }
  : {};
