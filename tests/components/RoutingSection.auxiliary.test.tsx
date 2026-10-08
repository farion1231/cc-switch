import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { RoutingSection } from "@/components/settings/sections/RoutingSection";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

// claude（第 0 项）在路由模式，其余应用 direct。
// useQueries 的返回顺序与 PROXY_APP_IDS 一致。
vi.mock("@tanstack/react-query", () => ({
  useQueries: () => [
    { data: { mode: "route", routeProviderId: "p1" } },
    { data: { mode: "direct" } },
    { data: { mode: "direct" } },
    { data: { mode: "direct" } },
  ],
}));

vi.mock("@/lib/query", () => ({
  useProvidersQuery: () => ({
    data: {
      providers: { p1: { id: "p1", name: "P1" } },
      currentProviderId: "p1",
    },
  }),
}));

vi.mock("@/lib/query/proxy", () => ({
  useGlobalProxyConfig: () => ({
    data: { listenAddress: "127.0.0.1", listenPort: 15721 },
  }),
  useProxyStatusQuery: () => ({
    data: { running: true, port: 15721 },
  }),
  useUpdateGlobalProxyConfig: () => ({ mutateAsync: vi.fn() }),
}));

vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({
    isRunning: true,
    startProxyServer: vi.fn(),
    stopWithRestore: vi.fn(),
    isPending: false,
  }),
}));

// 队列内容不是本用例关心的，替身只为让面板可渲染
vi.mock("@/components/proxy/AuxiliaryQueueManager", () => ({
  AuxiliaryQueueManager: ({
    appType,
    disabled,
  }: {
    appType: string;
    disabled?: boolean;
  }) => (
    <div data-testid="auxiliary-queue-manager" data-disabled={String(disabled)}>
      {appType}
    </div>
  ),
}));
vi.mock("@/components/proxy/AutoFailoverConfigPanel", () => ({
  AutoFailoverConfigPanel: () => <div />,
}));
vi.mock("@/components/settings/RectifierConfigPanel", () => ({
  RectifierConfigPanel: () => <div />,
}));

function renderSection() {
  render(<RoutingSection onOpenApp={vi.fn()} />);
}

describe("RoutingSection auxiliary panel", () => {
  it("is Claude-only: no per-app segmented control inside the panel", () => {
    renderSection();

    const manager = screen.getByTestId("auxiliary-queue-manager");
    expect(manager).toHaveTextContent("claude");

    // 分类器面板不该出现故障转移那套 4 应用分段控件
    const panel = manager.closest("div");
    expect(panel?.querySelectorAll('[role="radiogroup"]').length ?? 0).toBe(0);
  });

  it("hands the auxiliary queue exactly the claude app type", () => {
    renderSection();

    expect(screen.getByTestId("auxiliary-queue-manager")).toHaveTextContent(
      /^claude$/,
    );
  });

  it("enables the queue while claude is attached to routing", () => {
    renderSection();

    expect(screen.getByTestId("auxiliary-queue-manager")).toHaveAttribute(
      "data-disabled",
      "false",
    );
  });
});
