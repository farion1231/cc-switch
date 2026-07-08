import { useCallback, useEffect, useMemo, useState } from "react";
import type { VisibleApps } from "@/types";
import type { AppId } from "@/lib/api";
import {
  APP_STORAGE_KEY,
  VIEW_STORAGE_KEY,
  VALID_APPS,
  SESSION_APPS,
  getInitialApp,
  getInitialView,
  type View,
} from "./views";

/**
 * 外壳核心状态：当前应用 + 当前视图。
 * 负责持久化、隐藏应用回退、无会话支持时的视图回退。
 */
export function useAppShellState(visibleApps: VisibleApps) {
  const [activeApp, setActiveAppState] = useState<AppId>(getInitialApp);
  const [currentView, setCurrentView] = useState<View>(getInitialView);

  const sharedFeatureApp: AppId =
    activeApp === "claude-desktop" ? "claude" : activeApp;

  const setActiveApp = useCallback((app: AppId) => {
    localStorage.setItem(APP_STORAGE_KEY, app);
    setActiveAppState(app);
  }, []);

  useEffect(() => {
    localStorage.setItem(VIEW_STORAGE_KEY, currentView);
  }, [currentView]);

  // 当前应用被隐藏时回退到第一个可见应用
  useEffect(() => {
    if (!visibleApps[activeApp]) {
      const firstVisible =
        VALID_APPS.find((app) => visibleApps[app]) ?? "claude";
      setActiveApp(firstVisible);
    }
  }, [visibleApps, activeApp, setActiveApp]);

  const hasSkillsSupport = sharedFeatureApp !== "openclaw";
  const hasSessionSupport = SESSION_APPS.includes(sharedFeatureApp);

  // 切到不支持会话的应用时离开 sessions 视图
  useEffect(() => {
    const isUnsupportedSessionView =
      currentView === "sessions" && !SESSION_APPS.includes(sharedFeatureApp);
    const isUnsupportedSkillsView =
      !hasSkillsSupport &&
      (currentView === "skills" || currentView === "skillsDiscovery");
    const isUnsupportedOpenClawView =
      activeApp !== "openclaw" &&
      (currentView === "workspace" ||
        currentView === "openclawEnv" ||
        currentView === "openclawTools" ||
        currentView === "openclawAgents");
    const isUnsupportedHermesView =
      activeApp !== "hermes" && currentView === "hermesMemory";

    if (
      isUnsupportedSkillsView ||
      isUnsupportedSessionView ||
      isUnsupportedOpenClawView ||
      isUnsupportedHermesView
    ) {
      setCurrentView("providers");
    }
  }, [activeApp, sharedFeatureApp, currentView, hasSkillsSupport]);

  return useMemo(
    () => ({
      activeApp,
      setActiveApp,
      sharedFeatureApp,
      currentView,
      setCurrentView,
      hasSkillsSupport,
      hasSessionSupport,
    }),
    [
      activeApp,
      setActiveApp,
      sharedFeatureApp,
      currentView,
      hasSkillsSupport,
      hasSessionSupport,
    ],
  );
}
