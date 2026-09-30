import {
  render,
  screen,
  fireEvent,
  waitFor,
  act,
} from "@testing-library/react";
import {
  QueryClient,
  QueryClientProvider,
  focusManager,
} from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { ReactElement } from "react";
import { http, HttpResponse } from "msw";
import { toast } from "sonner";
import type { Provider } from "@/types";
import { ProviderList } from "@/components/providers/ProviderList";
import { server } from "../msw/server";

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    info: vi.fn(),
    warning: vi.fn(),
    error: vi.fn(),
  },
}));

const TAURI_ENDPOINT = "http://tauri.local";

const useDragSortMock = vi.fn();
const useSortableMock = vi.fn();
const providerCardRenderSpy = vi.fn();
/** 某张卡片最近一次渲染拿到的 props。 */
const lastProps = (id: string) =>
  providerCardRenderSpy.mock.calls
    .map((call) => call[0])
    .filter((props) => props.provider.id === id)
    .at(-1);

vi.mock("@/hooks/useDragSort", () => ({
  useDragSort: (...args: unknown[]) => useDragSortMock(...args),
}));

vi.mock("@/components/providers/ProviderCard", () => ({
  ProviderCard: (props: any) => {
    providerCardRenderSpy(props);
    const {
      provider,
      onSwitch,
      onEdit,
      onDelete,
      onDuplicate,
      onConfigureUsage,
    } = props;

    return (
      <div data-testid={`provider-card-${provider.id}`}>
        <button
          data-testid={`switch-${provider.id}`}
          onClick={() => onSwitch(provider)}
        >
          switch
        </button>
        <button
          data-testid={`edit-${provider.id}`}
          onClick={() => onEdit(provider)}
        >
          edit
        </button>
        <button
          data-testid={`duplicate-${provider.id}`}
          onClick={() => onDuplicate(provider)}
        >
          duplicate
        </button>
        <button
          data-testid={`usage-${provider.id}`}
          onClick={() => onConfigureUsage(provider)}
        >
          usage
        </button>
        <button
          data-testid={`delete-${provider.id}`}
          onClick={() => onDelete(provider)}
        >
          delete
        </button>
        <span data-testid={`is-current-${provider.id}`}>
          {props.isCurrent ? "current" : "inactive"}
        </span>
        <span data-testid={`drag-attr-${provider.id}`}>
          {props.dragHandleProps?.attributes?.["data-dnd-id"] ?? "none"}
        </span>
      </div>
    );
  },
}));

vi.mock("@/components/UsageFooter", () => ({
  default: () => <div data-testid="usage-footer" />,
}));

vi.mock("@dnd-kit/sortable", async () => {
  const actual = await vi.importActual<any>("@dnd-kit/sortable");

  return {
    ...actual,
    useSortable: (...args: unknown[]) => useSortableMock(...args),
  };
});

// Mock hooks that use QueryClient
vi.mock("@/hooks/useStreamCheck", () => ({
  useStreamCheck: () => ({
    checkProvider: vi.fn(),
    isChecking: () => false,
  }),
}));

vi.mock("@/lib/query/failover", () => ({
  useAutoFailoverEnabled: () => ({ data: false }),
  useFailoverQueue: () => ({ data: [] }),
  useAddToFailoverQueue: () => ({ mutate: vi.fn() }),
  useRemoveFromFailoverQueue: () => ({ mutate: vi.fn() }),
  useReorderFailoverQueue: () => ({ mutate: vi.fn() }),
}));

function createProvider(overrides: Partial<Provider> = {}): Provider {
  return {
    id: overrides.id ?? "provider-1",
    name: overrides.name ?? "Test Provider",
    settingsConfig: overrides.settingsConfig ?? {},
    category: overrides.category,
    createdAt: overrides.createdAt,
    sortIndex: overrides.sortIndex,
    meta: overrides.meta,
    websiteUrl: overrides.websiteUrl,
  };
}

function renderWithQueryClient(ui: ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });

  return render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
}

beforeEach(() => {
  useDragSortMock.mockReset();
  useSortableMock.mockReset();
  providerCardRenderSpy.mockClear();

  useSortableMock.mockImplementation(({ id }: { id: string }) => ({
    setNodeRef: vi.fn(),
    attributes: { "data-dnd-id": id },
    listeners: { onPointerDown: vi.fn() },
    transform: null,
    transition: null,
    isDragging: false,
  }));

  useDragSortMock.mockReturnValue({
    sortedProviders: [],
    sensors: [],
    handleDragEnd: vi.fn(),
  });
});

describe("ProviderList Component", () => {
  it("should render skeleton placeholders when loading", () => {
    const { container } = renderWithQueryClient(
      <ProviderList
        providers={{}}
        currentProviderId=""
        appId="claude"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
        isLoading
      />,
    );

    const placeholders = container.querySelectorAll(
      ".border-dashed.border-muted-foreground\\/40",
    );
    expect(placeholders).toHaveLength(3);
  });

  it("should show empty state and trigger create callback when no providers exist", () => {
    const handleCreate = vi.fn();
    useDragSortMock.mockReturnValueOnce({
      sortedProviders: [],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    renderWithQueryClient(
      <ProviderList
        providers={{}}
        currentProviderId=""
        appId="claude"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
        onCreate={handleCreate}
      />,
    );

    const addButton = screen.getByRole("button", {
      name: "provider.addProvider",
    });
    fireEvent.click(addButton);

    expect(handleCreate).toHaveBeenCalledTimes(1);
  });

  it("should render in order returned by useDragSort and pass through action callbacks", () => {
    const providerA = createProvider({ id: "a", name: "A" });
    const providerB = createProvider({ id: "b", name: "B" });

    const handleSwitch = vi.fn();
    const handleEdit = vi.fn();
    const handleDelete = vi.fn();
    const handleDuplicate = vi.fn();
    const handleUsage = vi.fn();
    const handleOpenWebsite = vi.fn();

    useDragSortMock.mockReturnValue({
      sortedProviders: [providerB, providerA],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    renderWithQueryClient(
      <ProviderList
        providers={{ a: providerA, b: providerB }}
        currentProviderId="b"
        appId="claude"
        onSwitch={handleSwitch}
        onEdit={handleEdit}
        onDelete={handleDelete}
        onDuplicate={handleDuplicate}
        onConfigureUsage={handleUsage}
        onOpenWebsite={handleOpenWebsite}
      />,
    );

    // Verify sort order
    expect(providerCardRenderSpy).toHaveBeenCalledTimes(2);
    expect(providerCardRenderSpy.mock.calls[0][0].provider.id).toBe("b");
    expect(providerCardRenderSpy.mock.calls[1][0].provider.id).toBe("a");

    // Verify current provider marker
    expect(providerCardRenderSpy.mock.calls[0][0].isCurrent).toBe(true);

    // Drag attributes from useSortable
    expect(
      providerCardRenderSpy.mock.calls[0][0].dragHandleProps?.attributes[
        "data-dnd-id"
      ],
    ).toBe("b");
    expect(
      providerCardRenderSpy.mock.calls[1][0].dragHandleProps?.attributes[
        "data-dnd-id"
      ],
    ).toBe("a");

    // Trigger action buttons
    fireEvent.click(screen.getByTestId("switch-b"));
    fireEvent.click(screen.getByTestId("edit-b"));
    fireEvent.click(screen.getByTestId("duplicate-b"));
    fireEvent.click(screen.getByTestId("usage-b"));
    fireEvent.click(screen.getByTestId("delete-a"));

    expect(handleSwitch).toHaveBeenCalledWith(providerB);
    expect(handleEdit).toHaveBeenCalledWith(providerB);
    expect(handleDuplicate).toHaveBeenCalledWith(providerB);
    expect(handleUsage).toHaveBeenCalledWith(providerB);
    expect(handleDelete).toHaveBeenCalledWith(providerA);

    // Verify useDragSort call parameters
    expect(useDragSortMock).toHaveBeenCalledWith(
      { a: providerA, b: providerB },
      "claude",
    );
  });

  it("marks the direct provider while the app is in routing mode", async () => {
    const providerA = createProvider({ id: "a", name: "A" });
    const providerB = createProvider({ id: "b", name: "B" });
    useDragSortMock.mockReturnValue({
      sortedProviders: [providerA, providerB],
      sensors: [],
      handleDragEnd: vi.fn(),
    });
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_direct_provider`, () =>
        HttpResponse.json("a"),
      ),
    );

    renderWithQueryClient(
      <ProviderList
        providers={{ a: providerA, b: providerB }}
        currentProviderId="b"
        appId="claude"
        isProxyTakeover
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onConfigureUsage={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    await waitFor(() => expect(lastProps("a")?.isDirectProvider).toBe(true));
    expect(lastProps("b")?.isCurrent).toBe(true);
    expect(lastProps("b")?.isDirectProvider).toBe(false);
  });

  it("turns cards into add / remove / set-as-default in Claude Stack mode", async () => {
    const route = createProvider({ id: "route", name: "Route" });
    const kimi = createProvider({ id: "kimi", name: "Kimi" });
    const other = createProvider({ id: "other", name: "Other" });
    const official = createProvider({
      id: "official",
      name: "Official",
      category: "official",
    });
    useDragSortMock.mockReturnValue({
      sortedProviders: [route, kimi, other, official],
      sensors: [],
      handleDragEnd: vi.fn(),
    });
    const setCalls: unknown[] = [];
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({
          active: true,
          members: [
            {
              providerId: "route",
              modelIds: ["ccs-claude-route--route-1"],
              route: true,
            },
            {
              providerId: "kimi",
              modelIds: ["ccs-claude-kimi--kimi-k3"],
              route: false,
            },
          ],
        }),
      ),
      http.post(
        `${TAURI_ENDPOINT}/set_proxy_stack_member`,
        async ({ request }) => {
          setCalls.push(await request.json());
          return HttpResponse.json(null);
        },
      ),
    );

    renderWithQueryClient(
      <ProviderList
        providers={{ route, kimi, other, official }}
        currentProviderId="route"
        appId="claude"
        isProxyTakeover
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onConfigureUsage={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    await waitFor(() => expect(lastProps("kimi")?.isStackMode).toBe(true));
    expect(lastProps("kimi")?.stackMember?.modelIds).toEqual([
      "ccs-claude-kimi--kimi-k3",
    ]);
    expect(lastProps("other")?.stackMember).toBeUndefined();
    expect(lastProps("other")?.onToggleStack).toBeTypeOf("function");
    // 默认那家也在名单里（移除按钮由卡片禁用）；官方账号不能添加。
    expect(lastProps("route")?.stackMember?.route).toBe(true);
    expect(lastProps("route")?.onToggleStack).toBeTypeOf("function");
    expect(lastProps("official")?.onToggleStack).toBeUndefined();
    // Stack 模式不做故障转移。
    expect(lastProps("kimi")?.onToggleFailover).toBeUndefined();
    expect(lastProps("kimi")?.isAutoFailoverEnabled).toBe(false);

    lastProps("kimi")?.onToggleStack(false);
    await waitFor(() => expect(setCalls).toHaveLength(1));
    expect(setCalls[0]).toEqual({
      appType: "claude",
      providerId: "kimi",
      enabled: false,
    });
  });

  it("reminds to restart Claude Code when Stack models change outside add / remove", async () => {
    const route = createProvider({ id: "route", name: "Route" });
    const kimi = createProvider({ id: "kimi", name: "Kimi" });
    useDragSortMock.mockReturnValue({
      sortedProviders: [route, kimi],
      sensors: [],
      handleDragEnd: vi.fn(),
    });
    const routeMember = (modelIds: string[]) => ({
      providerId: "route",
      modelIds,
      route: true,
    });
    let members = [
      routeMember(["ccs-claude-route--route-1"]),
      {
        providerId: "kimi",
        modelIds: ["ccs-claude-kimi--kimi-k3"],
        route: false,
      },
    ];
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({ active: true, members }),
      ),
      http.post(`${TAURI_ENDPOINT}/set_proxy_stack_member`, () =>
        HttpResponse.json(null),
      ),
    );
    vi.mocked(toast.info).mockClear();

    renderWithQueryClient(
      <ProviderList
        providers={{ route, kimi }}
        currentProviderId="route"
        appId="claude"
        isProxyTakeover
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );
    await waitFor(() => expect(lastProps("kimi")?.stackMember).toBeDefined());

    // 移出名单：保存成功的提示已经说了要重启，不再提示。
    members = [routeMember(["ccs-claude-route--route-1"])];
    lastProps("kimi")?.onToggleStack(false);
    await waitFor(() => expect(lastProps("kimi")?.stackMember).toBeUndefined());
    expect(toast.info).not.toHaveBeenCalled();

    // 别处改了默认那家的模型（编辑供应商、同步）：回到窗口时重查，提示重启。
    members = [
      routeMember(["ccs-claude-route--route-1", "ccs-claude-route--route-2"]),
    ];
    try {
      act(() => {
        focusManager.setFocused(false);
        focusManager.setFocused(true);
      });
      await waitFor(() =>
        expect(toast.info).toHaveBeenCalledWith(
          "provider.stackModelsChanged",
          expect.anything(),
        ),
      );
    } finally {
      focusManager.setFocused(undefined);
    }
  });

  it("keeps routing-mode cards when Stack mode is off", async () => {
    const route = createProvider({ id: "route", name: "Route" });
    const kimi = createProvider({ id: "kimi", name: "Kimi" });
    useDragSortMock.mockReturnValue({
      sortedProviders: [route, kimi],
      sensors: [],
      handleDragEnd: vi.fn(),
    });
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({
          active: false,
          members: [
            {
              providerId: "kimi",
              modelIds: ["ccs-claude-kimi--kimi-k3"],
              route: false,
            },
          ],
        }),
      ),
    );

    renderWithQueryClient(
      <ProviderList
        providers={{ route, kimi }}
        currentProviderId="route"
        appId="claude"
        isProxyTakeover
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onConfigureUsage={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    // 名单留着，但路由模式下不显示：没有添加 / 移除，照常有故障转移。
    await waitFor(() =>
      expect(lastProps("kimi")?.onToggleFailover).toBeTypeOf("function"),
    );
    expect(lastProps("kimi")?.isStackMode).toBe(false);
    expect(lastProps("kimi")?.stackMember).toBeUndefined();
    expect(lastProps("kimi")?.onToggleStack).toBeUndefined();
  });

  it("never lets ChatGPT accounts be added in Codex Stack mode", async () => {
    const thirdParty = (id: string) =>
      createProvider({
        id,
        name: id,
        settingsConfig: {
          auth: {},
          config: `model_provider = "custom"\n[model_providers.custom]\nbase_url = "https://${id}.example/v1"\n`,
        },
      });
    const route = thirdParty("route");
    const deepseek = thirdParty("deepseek");
    // 早期绑定托管账号的官方卡没有 category，按身份认。
    const managed = createProvider({
      id: "managed",
      name: "ChatGPT",
      settingsConfig: { auth: {}, config: "" },
      meta: {
        authBinding: {
          source: "managed_account",
          authProvider: "codex_oauth",
          accountId: "acct",
        },
      },
    } as Partial<Provider>);
    useDragSortMock.mockReturnValue({
      sortedProviders: [route, deepseek, managed],
      sensors: [],
      handleDragEnd: vi.fn(),
    });
    const setCalls: unknown[] = [];
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({
          active: true,
          members: [
            {
              providerId: "route",
              modelIds: ["ccs-route/gpt-route"],
              route: true,
            },
            {
              providerId: "deepseek",
              modelIds: ["ccs-deepseek/deepseek-v4-pro"],
              route: false,
            },
          ],
          notice: "officialModelsBundled",
        }),
      ),
      http.post(
        `${TAURI_ENDPOINT}/set_proxy_stack_member`,
        async ({ request }) => {
          setCalls.push(await request.json());
          return HttpResponse.json(null);
        },
      ),
    );

    renderWithQueryClient(
      <ProviderList
        providers={{ route, deepseek, managed }}
        currentProviderId="route"
        appId="codex"
        isProxyTakeover
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    await waitFor(() =>
      expect(lastProps("deepseek")?.stackMember).toBeDefined(),
    );
    expect(lastProps("deepseek")?.onToggleStack).toBeTypeOf("function");
    // 官方模型列表暂未取到：成员卡片带上提示。
    expect(lastProps("deepseek")?.stackNotice).toBe("officialModelsBundled");
    // 官方账号在 Stack 模式下只能设为默认。
    expect(lastProps("managed")?.isStackMode).toBe(true);
    expect(lastProps("managed")?.onToggleStack).toBeUndefined();

    lastProps("deepseek")?.onToggleStack(false);
    await waitFor(() => expect(setCalls).toHaveLength(1));
    expect(setCalls[0]).toEqual({
      appType: "codex",
      providerId: "deepseek",
      enabled: false,
    });
  });

  it("warns when Codex clients still use an old model list in Stack mode", async () => {
    const route = createProvider({ id: "route", name: "Route" });
    useDragSortMock.mockReturnValue({
      sortedProviders: [route],
      sensors: [],
      handleDragEnd: vi.fn(),
    });
    let active = true;
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({
          active,
          members: [
            { providerId: "route", modelIds: [], route: true },
          ],
          staleClients: { daemon: true, others: false },
        }),
      ),
    );

    const view = renderWithQueryClient(
      <ProviderList
        providers={{ route }}
        currentProviderId="route"
        appId="codex"
        isProxyTakeover
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );
    expect(
      await screen.findByText("proxy.stackMode.codexStale.title"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "proxy.stackMode.codexStale.restart" }),
    ).toBeInTheDocument();
    view.unmount();

    // 不在 Stack 模式：不提示。
    active = false;
    renderWithQueryClient(
      <ProviderList
        providers={{ route }}
        currentProviderId="route"
        appId="codex"
        isProxyTakeover
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );
    await waitFor(() =>
      expect(lastProps("route")?.isStackMode).toBe(false),
    );
    expect(
      screen.queryByText("proxy.stackMode.codexStale.title"),
    ).not.toBeInTheDocument();
  });

  it("hides Stack mode outside proxy mode and for apps without it", async () => {
    const provider = createProvider({ id: "a", name: "A" });
    useDragSortMock.mockReturnValue({
      sortedProviders: [provider],
      sensors: [],
      handleDragEnd: vi.fn(),
    });
    let stackReads = 0;
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () => {
        stackReads += 1;
        return HttpResponse.json({ active: true, members: [] });
      }),
    );

    for (const [appId, isProxyTakeover] of [
      ["claude", false],
      ["codex", false],
      ["gemini", true],
    ] as const) {
      providerCardRenderSpy.mockClear();
      const { unmount } = renderWithQueryClient(
        <ProviderList
          providers={{ a: provider }}
          currentProviderId="b"
          appId={appId}
          isProxyTakeover={isProxyTakeover}
          onSwitch={vi.fn()}
          onEdit={vi.fn()}
          onDelete={vi.fn()}
          onDuplicate={vi.fn()}
          onOpenWebsite={vi.fn()}
        />,
      );
      const props = providerCardRenderSpy.mock.calls.at(-1)?.[0];
      expect(props?.isStackMode, appId).toBe(false);
      expect(props?.onToggleStack, appId).toBeUndefined();
      expect(props?.stackMember, appId).toBeUndefined();
      unmount();
    }
    expect(stackReads).toBe(0);
  });

  it("filters providers with the search input", () => {
    const providerAlpha = createProvider({ id: "alpha", name: "Alpha Labs" });
    const providerBeta = createProvider({ id: "beta", name: "Beta Works" });

    useDragSortMock.mockReturnValue({
      sortedProviders: [providerAlpha, providerBeta],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    renderWithQueryClient(
      <ProviderList
        providers={{ alpha: providerAlpha, beta: providerBeta }}
        currentProviderId=""
        appId="claude"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    fireEvent.keyDown(window, { key: "f", metaKey: true });
    const searchInput = screen.getByPlaceholderText(
      "Search name, notes, or URL...",
    );
    // Initially both providers are rendered
    expect(screen.getByTestId("provider-card-alpha")).toBeInTheDocument();
    expect(screen.getByTestId("provider-card-beta")).toBeInTheDocument();

    fireEvent.change(searchInput, { target: { value: "beta" } });
    expect(screen.queryByTestId("provider-card-alpha")).not.toBeInTheDocument();
    expect(screen.getByTestId("provider-card-beta")).toBeInTheDocument();

    fireEvent.change(searchInput, { target: { value: "gamma" } });
    expect(screen.queryByTestId("provider-card-alpha")).not.toBeInTheDocument();
    expect(screen.queryByTestId("provider-card-beta")).not.toBeInTheDocument();
    expect(
      screen.getByText("No providers match your search."),
    ).toBeInTheDocument();
  });

  it("does not manufacture a Pi selection summary card", async () => {
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_pi_current_state`, () =>
        HttpResponse.json({
          enabledProviderIds: [],
        }),
      ),
    );

    renderWithQueryClient(
      <ProviderList
        providers={{}}
        currentProviderId=""
        appId="pi"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
        onCreate={vi.fn()}
      />,
    );

    expect(await screen.findByText("pi.empty.title")).toBeInTheDocument();
    expect(providerCardRenderSpy).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("button", { name: "provider.addProvider" }),
    ).not.toBeInTheDocument();
  });

  it("does not expose proxy or failover actions on Pi provider cards", async () => {
    const currentProvider = createProvider({
      id: "current-pi",
      name: "Current Pi",
    });
    const inactiveProvider = createProvider({
      id: "inactive-pi",
      name: "Inactive Pi",
    });
    useDragSortMock.mockReturnValue({
      sortedProviders: [currentProvider, inactiveProvider],
      sensors: [],
      handleDragEnd: vi.fn(),
    });
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_pi_current_state`, () =>
        HttpResponse.json({
          enabledProviderIds: ["current-pi", "inactive-pi"],
        }),
      ),
    );

    renderWithQueryClient(
      <ProviderList
        providers={{
          [currentProvider.id]: currentProvider,
          [inactiveProvider.id]: inactiveProvider,
        }}
        currentProviderId="current-pi"
        appId="pi"
        isProxyRunning
        isProxyTakeover
        activeProviderId="current-pi"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    await waitFor(() => {
      const currentCards = providerCardRenderSpy.mock.calls
        .map(([props]) => props)
        .filter((props) => props.provider.id === "current-pi");
      const inactiveCards = providerCardRenderSpy.mock.calls
        .map(([props]) => props)
        .filter((props) => props.provider.id === "inactive-pi");
      expect(currentCards).not.toHaveLength(0);
      expect(inactiveCards).not.toHaveLength(0);
      expect(currentCards.at(-1)).toMatchObject({
        isCurrent: false,
        isRemovalProtected: false,
        isProxyRunning: false,
        isProxyTakeover: false,
        isAutoFailoverEnabled: false,
        activeProviderId: undefined,
        onToggleFailover: undefined,
      });
      expect(inactiveCards.at(-1)).toMatchObject({
        isCurrent: false,
        isProxyRunning: false,
        isProxyTakeover: false,
      });
      expect(currentCards.at(-1)).not.toHaveProperty("piCurrentRoute");
    });
  });

  it("derives Pi membership only from the native provider ID list", async () => {
    const provider = createProvider({
      id: "drifted-pi",
      name: "Saved Pi",
      settingsConfig: { models: [{ id: "saved-model" }] },
    });
    useDragSortMock.mockReturnValue({
      sortedProviders: [provider],
      sensors: [],
      handleDragEnd: vi.fn(),
    });
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_pi_current_state`, () =>
        HttpResponse.json({
          enabledProviderIds: ["drifted-pi"],
        }),
      ),
    );

    renderWithQueryClient(
      <ProviderList
        providers={{ [provider.id]: provider }}
        currentProviderId=""
        appId="pi"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    await waitFor(() => {
      const latestCardProps = providerCardRenderSpy.mock.calls
        .map(([props]) => props)
        .filter((props) => props.provider.id === provider.id)
        .at(-1);
      expect(latestCardProps).toMatchObject({
        isCurrent: false,
        isInConfig: true,
        isRemovalProtected: false,
        isStateChangeProtected: false,
      });
    });
  });

  it("sets an inactive Pi provider through the ordinary provider action", async () => {
    const provider = createProvider({
      id: "inactive-pi",
      name: "Inactive Pi",
      settingsConfig: {
        models: [
          { id: "model-a", name: "Model A" },
          { id: "model-b", name: "Model B" },
        ],
      },
    });
    const onSwitch = vi.fn();
    useDragSortMock.mockReturnValue({
      sortedProviders: [provider],
      sensors: [],
      handleDragEnd: vi.fn(),
    });
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_pi_current_state`, () =>
        HttpResponse.json({
          enabledProviderIds: ["other-pi"],
        }),
      ),
    );

    renderWithQueryClient(
      <ProviderList
        providers={{ [provider.id]: provider }}
        currentProviderId=""
        appId="pi"
        onSwitch={onSwitch}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    fireEvent.click(await screen.findByTestId("switch-inactive-pi"));
    expect(onSwitch).toHaveBeenCalledWith(provider);
    const latestCardProps = providerCardRenderSpy.mock.calls
      .map(([props]) => props)
      .filter((props) => props.provider.id === "inactive-pi")
      .at(-1);
    expect(latestCardProps).not.toHaveProperty("onSwitchPiModel");
  });

  it("does not use legacy metadata when Pi's authoritative state is unavailable", async () => {
    const provider = createProvider({
      id: "legacy-pi",
      name: "Legacy Pi",
      meta: { liveConfigManaged: true },
    });
    useDragSortMock.mockReturnValue({
      sortedProviders: [provider],
      sensors: [],
      handleDragEnd: vi.fn(),
    });
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_pi_current_state`, () =>
        HttpResponse.json("current state unavailable", { status: 500 }),
      ),
    );

    renderWithQueryClient(
      <ProviderList
        providers={{ [provider.id]: provider }}
        currentProviderId=""
        appId="pi"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    await screen.findByRole("alert");
    await waitFor(() => {
      const latestCardProps = providerCardRenderSpy.mock.calls
        .map(([props]) => props)
        .filter((props) => props.provider.id === provider.id)
        .at(-1);
      expect(latestCardProps).toMatchObject({
        isCurrent: false,
        isInConfig: false,
        isStateChangeProtected: true,
      });
    });
  });

  it("keeps Pi provider creation on the page-level add action", async () => {
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_pi_current_state`, () =>
        HttpResponse.json({
          enabledProviderIds: [],
        }),
      ),
    );

    renderWithQueryClient(
      <ProviderList
        providers={{}}
        currentProviderId=""
        appId="pi"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
        onCreate={vi.fn()}
      />,
    );

    await screen.findByText("pi.empty.title");
    expect(
      screen.queryByRole("button", { name: "provider.importCurrent" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "provider.addProvider" }),
    ).not.toBeInTheDocument();
  });

  it("does not tell MiniMax Code users to click a missing import button", async () => {
    renderWithQueryClient(
      <ProviderList
        providers={{}}
        currentProviderId=""
        appId="mcode"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
        onCreate={vi.fn()}
      />,
    );

    await screen.findByText("mcode.empty.title");
    expect(screen.getByText("mcode.empty.description")).toBeInTheDocument();
    expect(
      screen.queryByText("provider.noProvidersDescription"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "provider.importCurrent" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "provider.addProvider" }),
    ).toBeInTheDocument();
  });
});
