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

// Linux 上禁用所有 drag region，规避 Wayland 下 gtk_window_begin_move_drag
// 相关的窗口事件异常（Tauri #13440）。macOS 上保留原有拖动行为；Windows
// 项目原本就不依赖这个。
//
// 这些常量设计为通过 JSX 属性 spread 消费（`{...DRAG_REGION_ATTR}`），
// 因为 `data-tauri-drag-region` 是 wry 侧的 attribute 存在性检测，必须
// 完全不渲染属性才算禁用；空字符串或 "false" 仍会触发。
export const DRAG_REGION_ENABLED = !isLinux();

export const DRAG_REGION_ATTR: Record<string, unknown> = DRAG_REGION_ENABLED
  ? { "data-tauri-drag-region": true }
  : {};

export const DRAG_REGION_STYLE: Record<string, unknown> = DRAG_REGION_ENABLED
  ? { WebkitAppRegion: "drag" }
  : {};
