import { useCallback, useEffect, useState } from "react";

/** 窗口窄于这个宽度时侧栏自动收成图标轨（用户手动选过就按手动的来）。 */
export const SIDEBAR_AUTO_COLLAPSE_WIDTH = 960;

const STORAGE_KEY = "cc-switch-sidebar-collapsed";

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

  return { collapsed, toggle };
}
