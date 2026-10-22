/** `@tauri-apps/api/window` 的 web shim —— mock 窗口方法（no-op）。 */
/**
 * 用 Proxy 兜底：window API 有几十个方法（setDecorations / setMinimize /
 * onScaleChanged …），逐个枚举易漏。未知方法/属性 → 返回 noop（异步），
 * `on*` 事件 → 返回 unlisten 桩，已知布尔/字符串返回假值。
 */

export interface MockWindow {
  [k: string]: (...args: unknown[]) => Promise<unknown>;
}

const noop = async () => {};
const noopUnlisten = async () => () => {};

const windowShim = new Proxy({} as MockWindow, {
  get(_t, prop) {
    // 已知返回布尔
    if (
      prop === "isMaximized" ||
      prop === "isDecorated" ||
      prop === "isFullscreen" ||
      prop === "isVisible"
    ) {
      return async () => false;
    }
    // 已知返回字符串
    if (prop === "currentTheme" || prop === "label" || prop === "title") {
      return async () => null;
    }
    // 事件监听 on* → 返回 unlisten 桩
    if (typeof prop === "string" && prop.startsWith("on")) {
      return noopUnlisten;
    }
    // 其余方法 → noop
    return noop;
  },
});

export function getCurrentWindow(): MockWindow {
  return windowShim;
}
