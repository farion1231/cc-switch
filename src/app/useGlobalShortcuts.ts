import { useEffect, useRef } from "react";
import { isTextEditableTarget } from "@/utils/domUtils";
import type { View } from "./views";

/**
 * 全局快捷键：
 *  - Cmd/Ctrl+, 打开设置
 *  - Escape 返回上一级视图
 */
export function useGlobalShortcuts(
  currentView: View,
  setCurrentView: (view: View) => void,
) {
  const currentViewRef = useRef(currentView);
  const setViewRef = useRef(setCurrentView);

  useEffect(() => {
    currentViewRef.current = currentView;
    setViewRef.current = setCurrentView;
  }, [currentView, setCurrentView]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "," && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        setViewRef.current("settings");
        return;
      }

      if (event.key !== "Escape" || event.defaultPrevented) return;

      if (document.body.style.overflow === "hidden") return;

      const view = currentViewRef.current;
      if (view === "providers") return;

      if (isTextEditableTarget(event.target)) return;

      event.preventDefault();
      setViewRef.current(view === "skillsDiscovery" ? "skills" : "providers");
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, []);
}
