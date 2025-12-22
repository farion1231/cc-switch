import type { AppId } from "@/lib/api";

export type TabKey = "Codex" | "Claude" | "Gemini";

export const APP_ORDER: AppId[] = ["claude", "codex", "gemini"];

export const TAB_ITEMS: TabKey[] = ["Codex", "Claude", "Gemini"];

export const TAB_TO_APP: Record<TabKey, AppId> = {
  Codex: "codex",
  Claude: "claude",
  Gemini: "gemini",
};

export const APP_TO_TAB: Record<AppId, TabKey> = {
  codex: "Codex",
  claude: "Claude",
  gemini: "Gemini",
};
