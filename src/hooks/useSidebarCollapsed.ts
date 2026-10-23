import { useCallback, useEffect, useState } from "react";
import { isMac } from "@/lib/platform";

/** 窗口窄于这个宽度时侧栏自动收成图标轨（用户手动选过就按手动的来）。 */
export const SIDEBAR_AUTO_COLLAPSE_WIDTH = 960;

const STORAGE_KEY = "cc-switch-sidebar-collapsed";

export const SIDEBAR_EXPANDED_WIDTH = 200;
/** 收起时的图标轨宽度：macOS 的红绿灯（新版更大）要约 74px，Mac 上放宽到 84 */
export const sidebarRailWidth = () => (isMac() ? 84 : 72);

export const sidebarWidth = (collapsed: boolean) =>
  collapsed ? sidebarRailWidth() : SIDEBAR_EXPANDED_WIDTH;

function readManualPreference(): boolean | null {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === "true") return true;
    if (saved === "false") return false;
  } catch {
    // 读不到就按窗口宽度自动决定
  }
  return null;
}

function isNarrowWindow(): boolean {
  return (
    typeof window !== "undefined" &&
    window.innerWidth < SIDEBAR_AUTO_COLLAPSE_WIDTH
  );
}

/**
 * 侧栏展开 / 收起：默认跟随窗口宽度（< 960 收起），⌘\ 或按钮手动切换后记住手动的选择。
 * 只在 Sidebar 里用：状态放在 App 的话，每次切换都会把整页重渲染一遍，动画第一帧就掉帧。
 */
export function useSidebarCollapsed() {
  const [manual, setManual] = useState<boolean | null>(readManualPreference);
  const [narrow, setNarrow] = useState(isNarrowWindow);

  useEffect(() => {
    const onResize = () => setNarrow(isNarrowWindow());
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  const collapsed = manual ?? narrow;
  const toggle = useCallback(() => {
    const next = !collapsed;
    setManual(next);
    try {
      localStorage.setItem(STORAGE_KEY, String(next));
    } catch {
      // 记不住也不影响这次切换
    }
  }, [collapsed]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key === "\\") {
        event.preventDefault();
        toggle();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [toggle]);

  return { collapsed, toggle };
}
