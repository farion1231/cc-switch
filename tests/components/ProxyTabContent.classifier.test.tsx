import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ProxyTabContent } from "@/components/settings/ProxyTabContent";
import type { SettingsFormState } from "@/hooks/useSettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({
    isRunning: true,
    takeoverStatus: { claude: true, codex: true, gemini: true, grokbuild: true },
    startProxyServer: vi.fn(),
    stopWithRestore: vi.fn(),
    isPending: false,
  }),
}));

// 队列内容不是本用例关心的，替身只为让面板可渲染
vi.mock("@/components/proxy/ClassifierQueueManager", () => ({
  ClassifierQueueManager: ({ appType }: { appType: string }) => (
    <div data-testid="classifier-queue-manager">{appType}</div>
  ),
}));
vi.mock("@/components/proxy/FailoverQueueManager", () => ({
  FailoverQueueManager: () => <div />,
}));
vi.mock("@/components/proxy/AutoFailoverConfigPanel", () => ({
  AutoFailoverConfigPanel: () => <div />,
}));
vi.mock("@/components/settings/RectifierConfigPanel", () => ({
  RectifierConfigPanel: () => <div />,
}));
vi.mock("@/components/settings/GlobalProxySettings", () => ({
  GlobalProxySettings: () => <div />,
}));
vi.mock("@/components/proxy", () => ({
  ProxyPanel: () => <div />,
}));

const settings = {} as SettingsFormState;

function openClassifierPanel() {
  render(<ProxyTabContent settings={settings} onAutoSave={vi.fn()} />);
  fireEvent.click(screen.getByText("settings.advanced.classifier.title"));
}

describe("ProxyTabContent classifier panel", () => {
  it("is Claude-only: no per-app tab strip inside the panel", () => {
    openClassifierPanel();

    const manager = screen.getByTestId("classifier-queue-manager");
    expect(manager).toHaveTextContent("claude");

    // 分类器面板不该出现故障转移那套 4 应用标签页
    const panel = manager.closest("[data-state]");
    expect(panel?.querySelectorAll('[role="tab"]').length ?? 0).toBe(0);
  });

  it("hands the classifier queue exactly the claude app type", () => {
    openClassifierPanel();

    expect(screen.getByTestId("classifier-queue-manager")).toHaveTextContent(
      /^claude$/,
    );
  });
});
