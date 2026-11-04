import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { ReactElement } from "react";
import { http, HttpResponse } from "msw";
import type { Provider } from "@/types";
import {
  ProviderList,
  type SwitchModeProps,
} from "@/components/providers/ProviderList";
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

const dshAccountStatusMock = vi.fn();

vi.mock("@/components/providers/DshAccount", () => ({
  useDshAccountStatus: (enabled: boolean) =>
    enabled ? dshAccountStatusMock() : { data: undefined },
  accountName: (status: any, fallback: string) =>
    status.profile?.name ?? fallback,
  DshAccountCard: ({ status }: any) => (
    <div data-testid="dsh-account-card">{status.profile?.name}</div>
  ),
}));

vi.mock("@/hooks/useDragSort", () => ({
  useDragSort: (...args: unknown[]) => useDragSortMock(...args),
}));

vi.mock("@/components/providers/ProviderCard", () => ({
  ProviderCard: (props: any) => {
    providerCardRenderSpy(props);
    const { provider, presentation, onEdit, onDelete, onDuplicate } = props;

    return (
      <div data-testid={`provider-card-${provider.id}`}>
        {presentation.status && (
          <span data-testid={`status-${provider.id}`}>
            {presentation.status.label}
          </span>
        )}
        {presentation.buttons.map((button: any) => (
          <button
            key={button.key}
            data-testid={`${button.key}-${provider.id}`}
            disabled={Boolean(button.disabledReason)}
            onClick={button.onClick}
          >
            {button.label}
          </button>
        ))}
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
          data-testid={`delete-${provider.id}`}
          onClick={() => onDelete(provider)}
        >
          delete
        </button>
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
  const baseProps = {
    currentProviderId: "",
    onSwitch: vi.fn(),
    onEdit: vi.fn(),
    onDelete: vi.fn(),
    onDuplicate: vi.fn(),
    onOpenWebsite: vi.fn(),
  };

  it.each([false, true])(
    "keeps account login inside the add-provider flow with existing providers: %s",
    (hasProvider) => {
      const providers: Record<string, Provider> = hasProvider
        ? {
            custom: {
              id: "custom",
              name: "Custom",
              settingsConfig: {},
              meta: { liveConfigManaged: true },
            },
          }
        : {};
      renderWithQueryClient(
        <ProviderList {...baseProps} appId="kimi" providers={providers} />,
      );
      expect(screen.queryByText("kimi.oauth.title")).not.toBeInTheDocument();
    },
  );

  function switchMode(
    overrides: Partial<SwitchModeProps> = {},
  ): SwitchModeProps {
    return {
      active: "direct",
      view: "direct",
      directId: null,
      routeId: null,
      failoverOn: false,
      queue: [],
      stackMembers: new Map(),
      routingReason: () => "",
      serviceRunning: false,
      actions: {
        switchDirect: vi.fn(),
        needsRouteDialog: vi.fn(),
        exitAndUse: vi.fn(),
        routeTo: vi.fn(),
        queueAdd: vi.fn(),
        queueRemove: vi.fn(),
        queueMove: vi.fn(),
        stackAdd: vi.fn(),
        stackRemove: vi.fn(),
        stackSetDefault: vi.fn(),
      },
      ...overrides,
    };
  }

  it("should render skeleton placeholders when loading", () => {
    const { container } = renderWithQueryClient(
      <ProviderList {...baseProps} providers={{}} appId="claude" isLoading />,
    );

    expect(container.querySelectorAll(".border-dashed")).toHaveLength(3);
  });

  it("should show empty state and trigger create callback when no providers exist", () => {
    const handleCreate = vi.fn();

    renderWithQueryClient(
      <ProviderList
        {...baseProps}
        providers={{}}
        appId="claude"
        onCreate={handleCreate}
      />,
    );

    expect(
      screen.getByRole("heading", { name: "provider.noProviders" }),
    ).toBeInTheDocument();
    // 页头已经有实心的「添加供应商」，空状态里的两个按钮都是描边
    const importButton = screen.getByRole("button", {
      name: "provider.importCurrent",
    });
    const addButton = screen.getByRole("button", {
      name: "provider.addProvider",
    });
    for (const button of [importButton, addButton]) {
      expect(button.className).toContain("border-border-strong");
      expect(button.className).not.toContain("bg-action");
    }

    fireEvent.click(addButton);
    expect(handleCreate).toHaveBeenCalledTimes(1);
  });

  it("renders in the order returned by useDragSort and wires the mode actions", () => {
    const providerA = createProvider({ id: "a", name: "A" });
    const providerB = createProvider({ id: "b", name: "B" });
    const handleEdit = vi.fn();
    const handleDelete = vi.fn();
    const handleDuplicate = vi.fn();
    const mode = switchMode({ directId: "b" });

    useDragSortMock.mockReturnValue({
      sortedProviders: [providerB, providerA],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    renderWithQueryClient(
      <ProviderList
        {...baseProps}
        providers={{ a: providerA, b: providerB }}
        currentProviderId="b"
        appId="claude"
        onEdit={handleEdit}
        onDelete={handleDelete}
        onDuplicate={handleDuplicate}
        switchMode={mode}
      />,
    );

    expect(providerCardRenderSpy).toHaveBeenCalledTimes(2);
    expect(providerCardRenderSpy.mock.calls[0][0].provider.id).toBe("b");
    expect(providerCardRenderSpy.mock.calls[1][0].provider.id).toBe("a");
    expect(lastProps("b")?.isCurrent).toBe(true);
    expect(screen.getByTestId("drag-attr-b")).toHaveTextContent("b");
    expect(screen.getByTestId("drag-attr-a")).toHaveTextContent("a");

    // 直连那家显示「使用中」，其余是「切换」
    expect(screen.getByTestId("status-b")).toHaveTextContent(
      "providerCard.status.inUse",
    );
    fireEvent.click(screen.getByTestId("switch-a"));
    expect(mode.actions.switchDirect).toHaveBeenCalledWith(providerA);

    fireEvent.click(screen.getByTestId("edit-b"));
    fireEvent.click(screen.getByTestId("duplicate-b"));
    fireEvent.click(screen.getByTestId("delete-a"));
    expect(handleEdit).toHaveBeenCalledWith(providerB);
    expect(handleDuplicate).toHaveBeenCalledWith(providerB);
    expect(handleDelete).toHaveBeenCalledWith(providerA);
    expect(useDragSortMock).toHaveBeenCalledWith(
      { a: providerA, b: providerB },
      "claude",
    );
  });

  it("shows queue sections while failover is on", () => {
    const providerA = createProvider({ id: "a", name: "A" });
    const providerB = createProvider({ id: "b", name: "B" });
    useDragSortMock.mockReturnValue({
      sortedProviders: [providerA, providerB],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    renderWithQueryClient(
      <ProviderList
        {...baseProps}
        providers={{ a: providerA, b: providerB }}
        appId="claude"
        switchMode={switchMode({
          active: "route",
          view: "route",
          routeId: "b",
          failoverOn: true,
          queue: ["b"],
        })}
      />,
    );

    expect(
      screen.getByRole("heading", { name: "providerCard.section.queue" }),
    ).toBeInTheDocument();
    expect(screen.getByTestId("queueRemove-b")).toBeInTheDocument();
    expect(screen.getByTestId("queueAdd-a")).toBeInTheDocument();
  });

  it("uses the additive layout for apps that keep several providers", async () => {
    const providerA = createProvider({ id: "a", name: "A" });
    const live = createProvider({ id: "live", name: "Live" });
    const onSwitch = vi.fn();
    const onRemoveFromConfig = vi.fn();
    useDragSortMock.mockReturnValue({
      sortedProviders: [providerA, live],
      sensors: [],
      handleDragEnd: vi.fn(),
    });
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_opencode_live_provider_ids`, () =>
        HttpResponse.json(["live"]),
      ),
    );

    renderWithQueryClient(
      <ProviderList
        {...baseProps}
        providers={{ a: providerA, live }}
        appId="opencode"
        onSwitch={onSwitch}
        onRemoveFromConfig={onRemoveFromConfig}
      />,
    );

    fireEvent.click(await screen.findByTestId("remove-live"));
    expect(onRemoveFromConfig).toHaveBeenCalledWith(live);
    fireEvent.click(screen.getByTestId("add-a"));
    expect(onSwitch).toHaveBeenCalledWith(providerA);
  });

  it.each(["mcode"] as const)(
    "uses native membership to add and remove %s providers",
    async (appId) => {
      const saved = createProvider({
        id: "saved",
        name: "Saved",
        meta: { liveConfigManaged: false },
      });
      const live = createProvider({
        id: "live",
        name: "Live",
        meta: { liveConfigManaged: true },
      });
      const onSwitch = vi.fn();
      const onRemoveFromConfig = vi.fn();
      useDragSortMock.mockReturnValue({
        sortedProviders: [saved, live],
        sensors: [],
        handleDragEnd: vi.fn(),
      });
      renderWithQueryClient(
        <ProviderList
          {...baseProps}
          providers={{ saved, live }}
          appId={appId}
          onSwitch={onSwitch}
          onRemoveFromConfig={onRemoveFromConfig}
        />,
      );
      fireEvent.click(await screen.findByTestId("remove-live"));
      expect(onRemoveFromConfig).toHaveBeenCalledWith(live);
      fireEvent.click(screen.getByTestId("add-saved"));
      expect(onSwitch).toHaveBeenCalledWith(saved);
    },
  );

  it("adds, switches and removes Kimi providers and marks only the native default-model owner as in use", () => {
    const current = createProvider({
      id: "current",
      name: "Current",
      meta: { liveConfigManaged: true },
    });
    const other = createProvider({
      id: "other",
      name: "Other",
      meta: { liveConfigManaged: true },
    });
    const saved = createProvider({
      id: "saved",
      name: "Saved",
      meta: { liveConfigManaged: false },
    });
    const onSwitch = vi.fn();
    const onSetAsDefault = vi.fn();
    const onRemoveFromConfig = vi.fn();
    useDragSortMock.mockReturnValue({
      sortedProviders: [current, other, saved],
      sensors: [],
      handleDragEnd: vi.fn(),
    });
    const props = {
      ...baseProps,
      appId: "kimi" as const,
      providers: { current, other, saved },
      onSwitch,
      onSetAsDefault,
      onRemoveFromConfig,
    };
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const { rerender } = render(
      <QueryClientProvider client={client}>
        <ProviderList {...props} currentProviderId="current" />
      </QueryClientProvider>,
    );
    expect(screen.getByTestId("status-current")).toHaveTextContent(
      "providerCard.status.default",
    );
    expect(lastProps("current")?.presentation.tone).toBe("direct");
    expect(lastProps("current")?.isCurrent).toBe(true);
    expect(screen.queryByTestId("switch-current")).not.toBeInTheDocument();
    expect(
      lastProps("current")?.presentation.deleteDisabledReason,
    ).toBeTruthy();
    expect(
      lastProps("current")?.presentation.buttons.find(
        (button: { key: string }) => button.key === "remove",
      )?.disabledReason,
    ).toBeTruthy();
    // 已添加的卡在「更多」菜单里「设为默认」；未添加的卡「添加」只写进配置
    expect(screen.queryByTestId("switch-other")).not.toBeInTheDocument();
    lastProps("other")
      ?.presentation.menuItems.find(
        (action: { key: string }) => action.key === "setDefault",
      )
      .onSelect();
    expect(onSetAsDefault).toHaveBeenLastCalledWith(other, undefined);
    expect(screen.queryByTestId("switch-saved")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("add-saved"));
    expect(onSwitch).toHaveBeenLastCalledWith(saved);
    fireEvent.click(screen.getByTestId("remove-other"));
    expect(onRemoveFromConfig).toHaveBeenCalledWith(other);
    rerender(
      <QueryClientProvider client={client}>
        <ProviderList {...props} currentProviderId="other" />
      </QueryClientProvider>,
    );
    expect(screen.queryByTestId("status-current")).not.toBeInTheDocument();
    expect(screen.getByTestId("status-other")).toHaveTextContent(
      "providerCard.status.default",
    );
    expect(
      lastProps("current")?.presentation.menuItems.map(
        (action: { key: string }) => action.key,
      ),
    ).toEqual(["setDefault"]);
  });

  it("does not mark any Kimi provider as current when the native selection is missing", () => {
    const provider = createProvider({
      id: "live",
      meta: { liveConfigManaged: true },
    });
    useDragSortMock.mockReturnValue({
      sortedProviders: [provider],
      sensors: [],
      handleDragEnd: vi.fn(),
    });
    renderWithQueryClient(
      <ProviderList
        {...baseProps}
        appId="kimi"
        providers={{ live: provider }}
      />,
    );
    expect(screen.queryByTestId("status-live")).not.toBeInTheDocument();
    expect(
      lastProps("live")?.presentation.menuItems.map(
        (action: { key: string }) => action.key,
      ),
    ).toEqual(["setDefault"]);
  });

  it("marks the native Kimi OAuth card as official without marking API-key presets", () => {
    const official = createProvider({
      id: "managed:kimi-code",
      name: "Kimi Official",
      icon: "kimi",
      settingsConfig: {
        provider: { type: "kimi", oauth: { key: "oauth/kimi-code" } },
        models: {},
      },
      meta: { liveConfigManaged: true },
    });
    const apiKey = createProvider({
      id: "kimi-coding",
      name: "Kimi For Coding",
      category: "cn_official",
      meta: { liveConfigManaged: true },
    });
    useDragSortMock.mockReturnValue({
      sortedProviders: [official, apiKey],
      sensors: [],
      handleDragEnd: vi.fn(),
    });
    renderWithQueryClient(
      <ProviderList
        {...baseProps}
        appId="kimi"
        currentProviderId={official.id}
        providers={{ [official.id]: official, [apiKey.id]: apiKey }}
      />,
    );
    expect(lastProps(official.id)?.presentation.chips).toContainEqual({
      key: "official",
      label: "providerCard.chip.official",
      tone: "outline",
    });
    expect(lastProps(official.id)?.presentation.status.label).toBe(
      "providerCard.status.default",
    );
    expect(lastProps(apiKey.id)?.presentation.chips).toEqual([]);
  });

  it("marks every card for global search and has no list search of its own", () => {
    const alpha = createProvider({ id: "alpha", name: "Alpha Labs" });
    const beta = createProvider({ id: "beta", name: "Beta Works" });
    useDragSortMock.mockReturnValue({
      sortedProviders: [alpha, beta],
      sensors: [],
      handleDragEnd: vi.fn(),
    });
    const { container } = renderWithQueryClient(
      <ProviderList
        {...baseProps}
        providers={{ alpha, beta }}
        appId="codex"
        switchMode={switchMode()}
      />,
    );

    for (const id of ["alpha", "beta"]) {
      const anchor = container.querySelector(
        `[data-search-anchor="provider:${id}"]`,
      );
      expect(anchor).toContainElement(
        screen.getByTestId(`provider-card-${id}`),
      );
    }
    // 找供应商走全局搜索：列表里不再有自己的搜索框，⌘F 也不再由列表接管
    const event = new KeyboardEvent("keydown", {
      key: "f",
      ctrlKey: true,
      cancelable: true,
    });
    fireEvent(window, event);
    expect(event.defaultPrevented).toBe(false);
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
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
      // Pi 只有启用 / 停用，没有路由、队列、健康状态
      for (const props of [currentCards.at(-1), inactiveCards.at(-1)]) {
        expect(props.isCurrent).toBe(false);
        expect(props.presentation.showHealth).toBeFalsy();
        expect(props.presentation.move).toBeUndefined();
        expect(
          props.presentation.buttons.map((button: any) => button.key),
        ).toEqual(["remove"]);
      }
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
        presentation: { buttons: [{ key: "remove" }] },
      });
      expect(latestCardProps.presentation.deleteDisabledReason).toBeUndefined();
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

    // 读到 Pi 当前配置之前不能改
    const enable = await screen.findByTestId("add-inactive-pi");
    await waitFor(() => expect(enable).toBeEnabled());
    fireEvent.click(enable);
    expect(onSwitch).toHaveBeenCalledWith(provider);
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

    await screen.findByText("无法读取 Pi 当前配置");
    await waitFor(() => {
      const latestCardProps = providerCardRenderSpy.mock.calls
        .map(([props]) => props)
        .filter((props) => props.provider.id === provider.id)
        .at(-1);
      expect(latestCardProps).toMatchObject({
        isCurrent: false,
        presentation: { buttons: [{ key: "add" }] },
      });
      expect(latestCardProps.presentation.deleteDisabledReason).toBe(
        "pi.current.stateUnavailableHint",
      );
    });
  });

  it("warns when an additive app's live config cannot be read", async () => {
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_hermes_live_provider_ids`, () =>
        HttpResponse.json("Failed to parse Hermes config as YAML", {
          status: 500,
        }),
      ),
    );

    renderWithQueryClient(
      <ProviderList
        providers={{}}
        currentProviderId=""
        appId="hermes"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    await screen.findByText("无法读取 apps.hermes 配置文件");
    expect(
      screen.getByText(/Failed to parse Hermes config as YAML/),
    ).toBeInTheDocument();
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

  it("shows the signed-in DeepSeek account as an official card under Added", async () => {
    dshAccountStatusMock.mockReturnValue({
      data: { signedIn: true, expired: false, profile: { name: "Jason" } },
    });
    renderWithQueryClient(
      <ProviderList
        providers={{}}
        currentProviderId=""
        appId="dsh"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
        onCreate={vi.fn()}
      />,
    );

    expect(await screen.findByTestId("dsh-account-card")).toHaveTextContent(
      "Jason",
    );
    expect(screen.getByText("providerCard.section.added")).toBeInTheDocument();
    expect(
      screen.queryByText("providerCard.section.addedEmpty"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "provider.addProvider" }),
    ).not.toBeInTheDocument();
  });

  it("keeps the empty state and no account card while signed out of DeepSeek", async () => {
    dshAccountStatusMock.mockReturnValue({
      data: { signedIn: false, expired: false, profile: null },
    });
    renderWithQueryClient(
      <ProviderList
        providers={{}}
        currentProviderId=""
        appId="dsh"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
        onCreate={vi.fn()}
      />,
    );

    expect(
      await screen.findByRole("button", { name: "provider.addProvider" }),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("dsh-account-card")).not.toBeInTheDocument();
  });
});
