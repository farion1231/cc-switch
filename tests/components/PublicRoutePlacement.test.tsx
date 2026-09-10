import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { toast } from "sonner";
import { ProxyTabContent } from "@/components/settings/ProxyTabContent";
import { ProxyPanel } from "@/components/proxy/ProxyPanel";
import { ProxyToggle } from "@/components/proxy/ProxyToggle";
import { ProviderCard } from "@/components/providers/ProviderCard";
import { getAppLabel } from "@/config/appConfig";
import type { SettingsFormState } from "@/hooks/useSettings";
import type { Provider } from "@/types";

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
}

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
  }),
}));

vi.mock("@/components/proxy/PublicRoutePanel", () => ({
  PublicRoutePanel: () => <div data-testid="public-route-panel" />,
}));

vi.mock("@/components/providers/ProviderHealthBadge", () => ({
  ProviderHealthBadge: () => null,
}));

// ProxyTabContent 只从 barrel 引入 ProxyPanel，stub 掉避免其内部 hooks
vi.mock("@/components/proxy", () => ({
  ProxyPanel: () => <div data-testid="proxy-panel-stub" />,
}));

vi.mock("@/components/proxy/AutoFailoverConfigPanel", () => ({
  AutoFailoverConfigPanel: () => <div />,
}));

vi.mock("@/components/proxy/FailoverQueueManager", () => ({
  FailoverQueueManager: () => <div />,
}));

vi.mock("@/components/settings/RectifierConfigPanel", () => ({
  RectifierConfigPanel: () => <div />,
}));

vi.mock("@/components/settings/GlobalProxySettings", () => ({
  GlobalProxySettings: () => <div />,
}));

// 可变的测试状态，各用例自行调整
const state = {
  cursorTakeover: false,
  publicRouteEnabled: false,
};

vi.mock("@/lib/query/proxy", () => ({
  usePublicRouteStatus: () => ({ data: { enabled: state.publicRouteEnabled } }),
  useProxyStatusQuery: () => ({
    data: {
      running: true,
      address: "127.0.0.1",
      port: 15721,
      success_rate: 100,
      total_requests: 0,
      active_connections: 0,
      active_targets: [],
      current_provider: null,
      uptime_seconds: 0,
    },
  }),
  useProxyTakeoverStatus: () => ({
    data: { cursor: state.cursorTakeover },
  }),
  useSetProxyTakeoverForApp: () => ({
    mutateAsync: vi.fn().mockResolvedValue(undefined),
    isPending: false,
  }),
  useGlobalProxyConfig: () => ({
    data: { listenAddress: "127.0.0.1", listenPort: 15721 },
  }),
  useUpdateGlobalProxyConfig: () => ({ mutateAsync: vi.fn() }),
}));

vi.mock("@/lib/query/failover", () => ({
  useFailoverQueue: () => ({ data: [] }),
  useProviderHealth: () => ({ data: {} }),
}));

vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: () => ({ data: null }),
}));

const setTakeoverForAppMock = vi.fn().mockResolvedValue(undefined);

vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({
    isRunning: true,
    takeoverStatus: { cursor: state.cursorTakeover },
    setTakeoverForApp: setTakeoverForAppMock,
    isPending: false,
    status: { address: "127.0.0.1", port: 15721 },
  }),
}));

const PUBLIC_ROUTE_HINT_KEY = "proxy.publicRoute.takeoverHint";

describe("ProxyTabContent 公网路由栏目", () => {
  const settings = {} as SettingsFormState;
  const onAutoSave = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    state.cursorTakeover = false;
    state.publicRouteEnabled = true;
  });

  it("在本地路由之后渲染公网路由栏目", () => {
    render(<ProxyTabContent settings={settings} onAutoSave={onAutoSave} />);

    const proxyTitle = screen.getByText("settings.advanced.proxy.title");
    const publicRouteTitle = screen.getByText("settings.advanced.publicRoute.title");

    expect(
      proxyTitle.compareDocumentPosition(publicRouteTitle) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("栏目上展示公网路由启用状态徽标", () => {
    render(<ProxyTabContent settings={settings} onAutoSave={onAutoSave} />);

    expect(
      screen.getByText("settings.advanced.publicRoute.enabled"),
    ).toBeInTheDocument();
  });

  it("展开公网路由栏目后展示公网路由面板", () => {
    render(<ProxyTabContent settings={settings} onAutoSave={onAutoSave} />);

    fireEvent.click(screen.getByText("settings.advanced.publicRoute.title"));

    expect(screen.getByTestId("public-route-panel")).toBeInTheDocument();
  });
});

describe("ProxyPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    state.cursorTakeover = false;
    state.publicRouteEnabled = false;
  });

  const renderPanel = () =>
    render(
      <ProxyPanel
        enableLocalProxy={true}
        onEnableLocalProxyChange={() => {}}
        onToggleProxy={async () => {}}
        isProxyPending={false}
      />,
    );

  it("不再内嵌公网路由面板（已迁移到独立的公网路由栏目）", () => {
    state.cursorTakeover = true;
    state.publicRouteEnabled = true;
    renderPanel();

    expect(screen.queryByTestId("public-route-panel")).not.toBeInTheDocument();
  });

  it("打开 cursor 路由开关时弹出公网路由设置提示", async () => {
    renderPanel();

    const cursorRow = screen.getByText(getAppLabel("cursor")).parentElement!;
    fireEvent.click(within(cursorRow).getByRole("switch"));

    await waitFor(() =>
      expect(vi.mocked(toast.info)).toHaveBeenCalledWith(
        PUBLIC_ROUTE_HINT_KEY,
        expect.anything(),
      ),
    );
  });

  it("打开非 cursor 应用的路由开关时不弹公网路由提示", async () => {
    renderPanel();

    const claudeRow = screen.getByText(getAppLabel("claude")).parentElement!;
    fireEvent.click(within(claudeRow).getByRole("switch"));

    await waitFor(() => expect(vi.mocked(toast.success)).toHaveBeenCalled());
    expect(vi.mocked(toast.info)).not.toHaveBeenCalled();
  });
});

describe("ProxyToggle（主页面路由开关）", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    state.cursorTakeover = false;
    state.publicRouteEnabled = false;
  });

  it("打开 cursor 路由开关时弹出公网路由设置提示", async () => {
    render(<ProxyToggle activeApp="cursor" />);

    fireEvent.click(screen.getByRole("switch"));

    await waitFor(() =>
      expect(setTakeoverForAppMock).toHaveBeenCalledWith({
        appType: "cursor",
        enabled: true,
      }),
    );
    await waitFor(() =>
      expect(vi.mocked(toast.info)).toHaveBeenCalledWith(
        PUBLIC_ROUTE_HINT_KEY,
        expect.anything(),
      ),
    );
  });

  it("关闭 cursor 路由开关时不弹公网路由提示", async () => {
    state.cursorTakeover = true;
    render(<ProxyToggle activeApp="cursor" />);

    fireEvent.click(screen.getByRole("switch"));

    await waitFor(() =>
      expect(setTakeoverForAppMock).toHaveBeenCalledWith({
        appType: "cursor",
        enabled: false,
      }),
    );
    expect(vi.mocked(toast.info)).not.toHaveBeenCalled();
  });
});

describe("ProviderCard Cursor 供应商标签", () => {
  const cursorProvider = {
    id: "cursor-third-party",
    name: "ThirdParty",
    category: "custom",
    settingsConfig: {},
    createdAt: 0,
  } as unknown as Provider;

  const renderCard = (isProxyTakeover: boolean) =>
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <ProviderCard
          provider={cursorProvider}
          isCurrent={false}
          appId="cursor"
          onSwitch={() => {}}
          onEdit={() => {}}
          onDelete={() => {}}
          onConfigureUsage={() => {}}
          onOpenWebsite={() => {}}
          onDuplicate={() => {}}
          isProxyRunning={true}
          isProxyTakeover={isProxyTakeover}
        />
      </QueryClientProvider>,
    );

  it("不再展示「需要路由」标签（路由开关开启时也不展示）", () => {
    renderCard(true);

    expect(screen.queryByText("cursor.needsRouting")).not.toBeInTheDocument();
  });

  it("路由开关关闭时不展示「需要公网路由」标签", () => {
    renderCard(false);

    expect(
      screen.queryByText("cursor.needsPublicRoute"),
    ).not.toBeInTheDocument();
  });

  it("路由开关开启时展示「需要公网路由」标签", () => {
    renderCard(true);

    expect(screen.getByText("cursor.needsPublicRoute")).toBeInTheDocument();
  });
});
