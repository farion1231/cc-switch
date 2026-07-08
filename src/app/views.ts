import type { AppId } from "@/lib/api";

/**
 * 应用外壳的视图定义与持久化。
 * 从 App.tsx 抽离，作为唯一事实来源（Sidebar 等处共用）。
 */

export type View =
  | "providers"
  | "settings"
  | "prompts"
  | "skills"
  | "skillsDiscovery"
  | "mcp"
  | "agents"
  | "universal"
  | "sessions"
  | "workspace"
  | "openclawEnv"
  | "openclawTools"
  | "openclawAgents"
  | "hermesMemory";

export const APP_STORAGE_KEY = "cc-switch-last-app";
export const VIEW_STORAGE_KEY = "cc-switch-last-view";

export const VALID_APPS: readonly AppId[] = [
  "claude",
  "claude-desktop",
  "codex",
  "gemini",
  "opencode",
  "openclaw",
  "hermes",
];

export const VALID_VIEWS: readonly View[] = [
  "providers",
  "settings",
  "prompts",
  "skills",
  "skillsDiscovery",
  "mcp",
  "agents",
  "universal",
  "sessions",
  "workspace",
  "openclawEnv",
  "openclawTools",
  "openclawAgents",
  "hermesMemory",
];

/** 支持会话管理的应用 */
export const SESSION_APPS: readonly AppId[] = [
  "claude",
  "codex",
  "opencode",
  "openclaw",
  "gemini",
  "hermes",
];

export const getInitialApp = (): AppId => {
  const saved = localStorage.getItem(APP_STORAGE_KEY) as AppId | null;
  if (saved && VALID_APPS.includes(saved)) {
    return saved;
  }
  return "claude";
};

export const getInitialView = (): View => {
  const saved = localStorage.getItem(VIEW_STORAGE_KEY) as View | null;
  if (saved && VALID_VIEWS.includes(saved)) {
    return saved;
  }
  return "providers";
};
