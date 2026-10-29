import React from "react";
import type { AppId } from "@/lib/api/types";
import type { PiFamilyAppId } from "@/lib/api/pi";
import type { VisibleApps } from "@/types";
import {
  ClaudeIcon,
  CodexIcon,
  GeminiIcon,
  OpenClawIcon,
} from "@/components/BrandIcons";
import { ProviderIcon } from "@/components/ProviderIcon";

export interface AppConfig {
  label: string;
  icon: React.ReactNode;
  activeClass: string;
  badgeClass: string;
}

export const APP_IDS: AppId[] = [
  "claude",
  "claude-desktop",
  "codex",
  "gemini",
  "grokbuild",
  "opencode",
  "openclaw",
  "hermes",
  "pi",
  "mcode",
  "kimi",
  "dsh",
  "agy",
  "omp",
];

export const DEFAULT_VISIBLE_APPS: VisibleApps = {
  claude: true,
  "claude-desktop": true,
  codex: true,
  gemini: true,
  grokbuild: true,
  opencode: true,
  openclaw: true,
  hermes: true,
  pi: true,
  mcode: true,
  kimi: true,
  dsh: true,
  agy: true,
  omp: true,
};

/**
 * 应用页和侧栏的应用顺序：先按用户拖出来的顺序（`settings.appOrder`，不认识的 ID 忽略），
 * 没在里面的应用（比如新版本加的）插回默认顺序里它前一个应用的后面。
 */
export function orderAppIds(saved: readonly string[] | undefined): AppId[] {
  if (!saved?.length) return [...APP_IDS];
  const known = new Set<string>(APP_IDS);
  const ordered: AppId[] = [];
  for (const id of saved) {
    if (known.has(id) && !ordered.includes(id as AppId)) {
      ordered.push(id as AppId);
    }
  }
  APP_IDS.forEach((app, index) => {
    if (ordered.includes(app)) return;
    const previous = APP_IDS.slice(0, index)
      .reverse()
      .find((id) => ordered.includes(id));
    ordered.splice(previous ? ordered.indexOf(previous) + 1 : 0, 0, app);
  });
  return ordered;
}

/** App IDs shown in Skills panels. */
export const SKILLS_APP_IDS: AppId[] = [
  "claude",
  "codex",
  "gemini",
  "grokbuild",
  "opencode",
  "hermes",
  "pi",
  "mcode",
  "kimi",
  "dsh",
  "agy",
  "omp",
];

export type ProxyAppId = Extract<
  AppId,
  "claude" | "codex" | "gemini" | "grokbuild"
>;

/** Apps with a complete local gateway + failover data plane. */
export const PROXY_APP_IDS: ProxyAppId[] = [
  "claude",
  "codex",
  "gemini",
  "grokbuild",
];

export function isProxyAppId(appId: string): appId is ProxyAppId {
  return (PROXY_APP_IDS as string[]).includes(appId);
}

/**
 * 有直连 / 路由模式的应用（后端 `AppType::has_route_mode`）：上面四个，加上 Claude Desktop。
 * Desktop 经同一个路由服务转发，但没有故障转移和路由设置（不在 `PROXY_APP_IDS` 里）。
 */
export type ModeAppId = ProxyAppId | "claude-desktop";

export const MODE_APP_IDS: ModeAppId[] = [...PROXY_APP_IDS, "claude-desktop"];

export function isModeAppId(appId: string): appId is ModeAppId {
  return (MODE_APP_IDS as string[]).includes(appId);
}

/** 支持 Stack 模式的应用（后端 `mode::stack::supports_stack` 的镜像）。 */
export type StackAppId = Extract<
  ModeAppId,
  "claude" | "codex" | "claude-desktop"
>;

export const STACK_APP_IDS: StackAppId[] = [
  "claude",
  "codex",
  "claude-desktop",
];

export function isStackAppId(appId: string): appId is StackAppId {
  return (STACK_APP_IDS as string[]).includes(appId);
}

export type AdditiveAppId = Extract<
  AppId,
  "opencode" | "openclaw" | "hermes" | "pi" | "mcode" | "kimi" | "dsh" | "omp"
>;

export const ADDITIVE_APP_IDS: AdditiveAppId[] = [
  "mcode",
  "kimi",
  "dsh",
  "opencode",
  "openclaw",
  "hermes",
  "pi",
  "omp",
];

export function isAdditiveAppId(appId: string): appId is AdditiveAppId {
  return (ADDITIVE_APP_IDS as string[]).includes(appId);
}

/** Oh My Pi 是 Pi 的分支：供应商表单、启用状态和用量脚本命令与 Pi 共用。 */
export const PI_FAMILY_APP_IDS: PiFamilyAppId[] = ["pi", "omp"];

export function isPiFamilyApp(appId: string): appId is PiFamilyAppId {
  return (PI_FAMILY_APP_IDS as string[]).includes(appId);
}

/**
 * 切换只替换关键字段的应用：供应商编辑器显示「切到这个供应商之后配置文件的样子」，由后端
 * `ProviderService::editor_view` 投影。
 */
export const EDITOR_VIEW_APP_IDS: AppId[] = [
  "claude",
  "codex",
  "gemini",
  "grokbuild",
];

export function usesEditorView(appId: AppId): boolean {
  return EDITOR_VIEW_APP_IDS.includes(appId);
}

/** Claude Desktop、OpenClaw 不由 CC Switch 管理 MCP；Pi 1.0 起内置 MCP（`~/.pi/agent/mcp.json`） */
export type McpAppId = Exclude<AppId, "claude-desktop" | "openclaw">;
export const MCP_APP_IDS: McpAppId[] = [
  "claude",
  "codex",
  "gemini",
  "grokbuild",
  "opencode",
  "hermes",
  "pi",
  "mcode",
  "kimi",
  "dsh",
  "agy",
  "omp",
];

export function isMcpAppId(appId: string): appId is McpAppId {
  return (MCP_APP_IDS as string[]).includes(appId);
}

export function usesGeminiStyleForm(appId: string): boolean {
  return appId === "gemini" || appId === "agy";
}

export const APP_ICON_MAP: Record<AppId, AppConfig> = {
  claude: {
    label: "Claude",
    icon: <ClaudeIcon size={14} />,
    activeClass:
      "bg-orange-500/10 ring-1 ring-orange-500/20 hover:bg-orange-500/20 text-orange-600 dark:text-orange-400",
    badgeClass:
      "bg-orange-500/10 text-orange-700 dark:text-orange-300 hover:bg-orange-500/20 border-0 gap-1.5",
  },
  "claude-desktop": {
    label: "Claude Desktop",
    icon: <ClaudeIcon size={14} />,
    activeClass:
      "bg-amber-500/10 ring-1 ring-amber-500/20 hover:bg-amber-500/20 text-amber-700 dark:text-amber-300",
    badgeClass:
      "bg-amber-500/10 text-amber-700 dark:text-amber-300 hover:bg-amber-500/20 border-0 gap-1.5",
  },
  codex: {
    label: "Codex",
    icon: <CodexIcon size={14} />,
    activeClass:
      "bg-green-500/10 ring-1 ring-green-500/20 hover:bg-green-500/20 text-green-600 dark:text-green-400",
    badgeClass:
      "bg-green-500/10 text-green-700 dark:text-green-300 hover:bg-green-500/20 border-0 gap-1.5",
  },
  gemini: {
    label: "Gemini",
    icon: <GeminiIcon size={14} />,
    activeClass:
      "bg-blue-500/10 ring-1 ring-blue-500/20 hover:bg-blue-500/20 text-blue-600 dark:text-blue-400",
    badgeClass:
      "bg-blue-500/10 text-blue-700 dark:text-blue-300 hover:bg-blue-500/20 border-0 gap-1.5",
  },
  grokbuild: {
    label: "Grok Build",
    icon: (
      <ProviderIcon
        icon="grok"
        name="Grok Build"
        size={14}
        showFallback={false}
      />
    ),
    activeClass:
      "bg-cyan-500/10 ring-1 ring-cyan-500/20 hover:bg-cyan-500/20 text-cyan-700 dark:text-cyan-300",
    badgeClass:
      "bg-cyan-500/10 text-cyan-700 dark:text-cyan-300 hover:bg-cyan-500/20 border-0 gap-1.5",
  },
  opencode: {
    label: "OpenCode",
    icon: (
      <ProviderIcon
        icon="opencode"
        name="OpenCode"
        size={14}
        showFallback={false}
      />
    ),
    activeClass:
      "bg-indigo-500/10 ring-1 ring-indigo-500/20 hover:bg-indigo-500/20 text-indigo-600 dark:text-indigo-400",
    badgeClass:
      "bg-indigo-500/10 text-indigo-700 dark:text-indigo-300 hover:bg-indigo-500/20 border-0 gap-1.5",
  },
  openclaw: {
    label: "OpenClaw",
    icon: <OpenClawIcon size={14} />,
    activeClass:
      "bg-rose-500/10 ring-1 ring-rose-500/20 hover:bg-rose-500/20 text-rose-600 dark:text-rose-400",
    badgeClass:
      "bg-rose-500/10 text-rose-700 dark:text-rose-300 hover:bg-rose-500/20 border-0 gap-1.5",
  },
  hermes: {
    label: "Hermes",
    icon: (
      <ProviderIcon
        icon="hermes"
        name="Hermes"
        size={14}
        showFallback={false}
      />
    ),
    activeClass:
      "bg-violet-500/10 ring-1 ring-violet-500/20 hover:bg-violet-500/20 text-violet-600 dark:text-violet-400",
    badgeClass:
      "bg-violet-500/10 text-violet-700 dark:text-violet-300 hover:bg-violet-500/20 border-0 gap-1.5",
  },
  dsh: {
    label: "DeepSeek Harness",
    icon: <ProviderIcon icon="deepseek" name="DeepSeek Harness" size={14} />,
    activeClass: "bg-blue-500/10 text-blue-600 dark:text-blue-400",
    badgeClass:
      "bg-blue-500/10 text-blue-700 dark:text-blue-300 border-0 gap-1.5",
  },
  kimi: {
    label: "Kimi Code",
    icon: <ProviderIcon icon="kimi" name="Kimi Code" size={14} />,
    activeClass: "bg-blue-500/10 text-blue-600 dark:text-blue-400",
    badgeClass:
      "bg-blue-500/10 text-blue-700 dark:text-blue-300 border-0 gap-1.5",
  },
  mcode: {
    label: "MiniMax Code",
    icon: <ProviderIcon icon="minimax" name="MiniMax Code" size={14} />,
    activeClass: "bg-orange-500/10 text-orange-600 dark:text-orange-400",
    badgeClass:
      "bg-orange-500/10 text-orange-700 dark:text-orange-300 border-0 gap-1.5",
  },
  pi: {
    label: "Pi",
    icon: <ProviderIcon icon="pi" name="Pi" size={14} showFallback={false} />,
    activeClass:
      "bg-fuchsia-500/10 ring-1 ring-fuchsia-500/20 hover:bg-fuchsia-500/20 text-fuchsia-600 dark:text-fuchsia-400",
    badgeClass:
      "bg-fuchsia-500/10 text-fuchsia-700 dark:text-fuchsia-300 hover:bg-fuchsia-500/20 border-0 gap-1.5",
  },
  agy: {
    label: "Antigravity",
    icon: (
      <ProviderIcon
        icon="antigravity"
        name="Antigravity"
        size={14}
        showFallback={false}
      />
    ),
    activeClass:
      "bg-sky-500/10 ring-1 ring-sky-500/20 hover:bg-sky-500/20 text-sky-700 dark:text-sky-300",
    badgeClass:
      "bg-sky-500/10 text-sky-700 dark:text-sky-300 hover:bg-sky-500/20 border-0 gap-1.5",
  },
  omp: {
    label: "Oh My Pi",
    icon: (
      <ProviderIcon icon="omp" name="Oh My Pi" size={14} showFallback={false} />
    ),
    activeClass:
      "bg-cyan-500/10 ring-1 ring-cyan-500/20 hover:bg-cyan-500/20 text-cyan-600 dark:text-cyan-400",
    badgeClass:
      "bg-cyan-500/10 text-cyan-700 dark:text-cyan-300 hover:bg-cyan-500/20 border-0 gap-1.5",
  },
};

export function getAppLabel(appId: string): string {
  return APP_ICON_MAP[appId as AppId]?.label ?? appId;
}
