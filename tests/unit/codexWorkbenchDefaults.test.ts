import { describe, expect, it } from "vitest";
import {
  DEFAULT_CODEX_ENHANCEMENTS,
  DEFAULT_CODEX_WORKBENCH_SETTINGS,
} from "@/types/codexWorkbench";

describe("Codex workbench defaults", () => {
  it("enables the first six enhancements and disables the last five", () => {
    const e = DEFAULT_CODEX_ENHANCEMENTS;
    expect(e.pluginUnlock).toBe(true);
    expect(e.autoExpand).toBe(true);
    expect(e.sessionDelete).toBe(true);
    expect(e.wideConversation).toBe(true);
    expect(e.nativeMenu).toBe(true);
    expect(e.userScriptRuntime).toBe(true);
    expect(e.markdownExport).toBe(false);
    expect(e.modelSwitcher).toBe(false);
    expect(e.systemPrompt).toBe(false);
    expect(e.reasoningResume).toBe(false);
    expect(e.reasoningToken).toBe(false);
  });

  it("uses 30-minute radar TTL and script market URL", () => {
    expect(DEFAULT_CODEX_WORKBENCH_SETTINGS.radarTtlMinutes).toBe(30);
    expect(DEFAULT_CODEX_WORKBENCH_SETTINGS.scriptMarketUrl).toContain(
      "CodexPlusPlusScriptMarket",
    );
    expect(DEFAULT_CODEX_WORKBENCH_SETTINGS.autoLaunch).toBe(true);
    expect(DEFAULT_CODEX_WORKBENCH_SETTINGS.autoStartProxy).toBe(true);
  });
});
