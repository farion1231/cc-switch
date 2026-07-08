import { lazy } from "react";

/**
 * 视图级代码分割。
 * 除默认的供应商列表外，所有页面按需加载，
 * 大幅缩小首屏 bundle、加快冷启动。
 */

export const LazySettingsPage = lazy(() =>
  import("@/components/settings/SettingsPage").then((m) => ({
    default: m.SettingsPage,
  })),
);

export const LazyPromptPanel = lazy(
  () => import("@/components/prompts/PromptPanel"),
);

export const LazyUnifiedMcpPanel = lazy(
  () => import("@/components/mcp/UnifiedMcpPanel"),
);

export const LazyUnifiedSkillsPanel = lazy(
  () => import("@/components/skills/UnifiedSkillsPanel"),
);

export const LazySkillsPage = lazy(() =>
  import("@/components/skills/SkillsPage").then((m) => ({
    default: m.SkillsPage,
  })),
);

export const LazyAgentsPanel = lazy(() =>
  import("@/components/agents/AgentsPanel").then((m) => ({
    default: m.AgentsPanel,
  })),
);

export const LazyUniversalProviderPanel = lazy(() =>
  import("@/components/universal").then((m) => ({
    default: m.UniversalProviderPanel,
  })),
);

export const LazySessionManagerPage = lazy(() =>
  import("@/components/sessions/SessionManagerPage").then((m) => ({
    default: m.SessionManagerPage,
  })),
);

export const LazyWorkspaceFilesPanel = lazy(
  () => import("@/components/workspace/WorkspaceFilesPanel"),
);

export const LazyEnvPanel = lazy(
  () => import("@/components/openclaw/EnvPanel"),
);

export const LazyToolsPanel = lazy(
  () => import("@/components/openclaw/ToolsPanel"),
);

export const LazyAgentsDefaultsPanel = lazy(
  () => import("@/components/openclaw/AgentsDefaultsPanel"),
);

export const LazyHermesMemoryPanel = lazy(
  () => import("@/components/hermes/HermesMemoryPanel"),
);

/* 重量级对话框（表单 + CodeMirror）也按需加载 */

export const LazyAddProviderDialog = lazy(() =>
  import("@/components/providers/AddProviderDialog").then((m) => ({
    default: m.AddProviderDialog,
  })),
);

export const LazyEditProviderDialog = lazy(() =>
  import("@/components/providers/EditProviderDialog").then((m) => ({
    default: m.EditProviderDialog,
  })),
);

export const LazyUsageScriptModal = lazy(
  () => import("@/components/UsageScriptModal"),
);
