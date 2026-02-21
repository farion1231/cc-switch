import React from "react";
import type { AppId } from "@/lib/api/types";
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
  "codex",
  "gemini",
  "opencode",
  "openclaw",
];

/** App IDs shown in MCP & Skills panels (excludes OpenClaw) */
export const MCP_SKILLS_APP_IDS: AppId[] = [
  "claude",
  "codex",
  "gemini",
  "opencode",
];

export const APP_ICON_MAP: Record<AppId, AppConfig> = {
  claude: {
    label: "Claude",
    icon: <ClaudeIcon size={14} />,
    activeClass:
      "bg-orange-500/10 ring-1 ring-orange-500/20 hover:bg-orange-500/20 text-orange-600 dark:text-orange-400",
    badgeClass:
      "bg-orange-500/10 text-orange-700 dark:text-orange-300 hover:bg-orange-500/20 border-0 gap-1.5",
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
};
