import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { OverviewTab } from "@/components/codex-workbench/OverviewTab";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: { defaultValue?: string }) =>
      opts?.defaultValue ?? key,
    i18n: { language: "zh" },
  }),
}));

vi.mock("@/lib/query/codexWorkbench", () => ({
  useCodexWorkbenchStatusQuery: () => ({
    data: {
      runtimeState: "idle",
      cdpPort: 9222,
      bridgeState: "ready",
      currentProviderId: "provider-a",
      proxyRunning: true,
      lastError: null,
    },
    isLoading: false,
  }),
  useCodexWorkbenchSettingsQuery: () => ({
    data: {
      enhancements: {
        pluginUnlock: true,
        autoExpand: true,
        sessionDelete: false,
        wideConversation: false,
        nativeMenu: false,
        userScriptRuntime: false,
        markdownExport: false,
        modelSwitcher: false,
        systemPrompt: true,
        reasoningResume: true,
        reasoningToken: false,
      },
    },
    isLoading: false,
  }),
}));

describe("OverviewTab", () => {
  it("shows runtime / provider flags without prompt body text", () => {
    render(<OverviewTab />);

    const root = screen.getByTestId("codex-overview-tab");
    expect(root).toBeInTheDocument();
    expect(root.textContent).toMatch(/CDP/);
    expect(root.textContent).toMatch(/Bridge/);
    expect(root.textContent).toContain("provider-a");
    expect(root.textContent).toContain("提示词替换");
    expect(root.textContent).toContain("推理续接");
    expect(root.textContent).toContain("reasoningResume");
    expect(root.textContent).toContain("systemPrompt");

    // no-body contract: never render a long system prompt payload
    expect(root.textContent).not.toMatch(
      /You are ChatGPT|system prompt body|BEGIN PROMPT/i,
    );
    expect(root.textContent).toContain("不读取、不展示任何提示词正文");
  });
});
