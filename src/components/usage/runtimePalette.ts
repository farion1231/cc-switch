import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import type { AppId } from "@/lib/api";

const RUNTIME_COLORS = new Map<string, string>([
  ["claude", "var(--agent-claude)"],
  ["openclaw", "var(--usage-runtime-openclaw)"],
  ["hermes", "var(--usage-runtime-hermes)"],
  ["codex", "var(--agent-codex)"],
  ["gemini", "var(--agent-gemini)"],
  ["opencode", "var(--agent-opencode)"],
  ["pi", "var(--agent-pi)"],
  ["agy", "var(--heat-app-agy)"],
  ["codebuddy", "var(--heat-app-codebuddy)"],
  ["copilot", "var(--heat-app-copilot)"],
  ["dsh", "var(--heat-app-dsh)"],
  ["grokbuild", "var(--heat-app-grokbuild)"],
  ["kimi", "var(--heat-app-kimi)"],
  ["mcode", "var(--heat-app-mcode)"],
  ["omp", "var(--heat-app-omp)"],
  ["workbuddy", "var(--heat-app-workbuddy)"],
]);

export function runtimeName(app: string): string {
  return Object.prototype.hasOwnProperty.call(APP_DISPLAY_NAME, app)
    ? APP_DISPLAY_NAME[app as AppId]
    : app;
}

export function runtimeColor(app: string): string {
  return RUNTIME_COLORS.get(app) ?? "var(--chart-usage)";
}
