import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { useState, type ReactElement } from "react";
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

  it.each([{ ctrlKey: true }, { metaKey: true }])(
    "keeps search in the list flow and restores cards on close (%o)",
    async (shortcut) => {
      const alpha = createProvider({ id: "alpha", name: "Alpha Labs" });
      const beta = createProvider({ id: "beta", name: "Beta Works" });
      const onEdit = vi.fn();
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
          onEdit={onEdit}
        />,
      );

      fireEvent.keyDown(window, { key: "f", ...shortcut });
      const input = screen.getByRole("textbox", { name: "Search providers" });
      // A body portal with fixed positioning covers the first card's actions.
      // Keep the panel before the cards in the list's normal document flow.
      expect(container).toContainElement(input);
      const list = container.firstElementChild!;
      const panel = list.firstElementChild!;
      expect(panel).toContainElement(input);
      expect(panel).not.toHaveClass("fixed");
      expect(panel).not.toHaveClass("absolute");
      expect(panel.nextElementSibling).toContainElement(
        screen.getByTestId("provider-card-alpha"),
      );
      await waitFor(() => expect(input).toHaveFocus());

      fireEvent.change(input, { target: { value: "beta" } });
      expect(
        screen.queryByTestId("provider-card-alpha"),
      ).not.toBeInTheDocument();
      fireEvent.click(screen.getByTestId("edit-beta"));
      expect(onEdit).toHaveBeenCalledWith(beta);

      fireEvent.keyDown(window, { key: "Escape" });
      await waitFor(() => expect(input).not.toBeInTheDocument());
      expect(screen.getByTestId("provider-card-alpha")).toBeInTheDocument();

      fireEvent.keyDown(window, { key: "f", ...shortcut });
      const reopened = screen.getByRole("textbox", {
        name: "Search providers",
      });
      expect(reopened).toHaveValue("beta");
      fireEvent.click(screen.getByRole("button", { name: "Clear" }));
      expect(screen.getByTestId("provider-card-alpha")).toBeInTheDocument();
      fireEvent.click(
        screen.getByRole("button", { name: "Close provider search" }),
      );
      await waitFor(() => expect(reopened).not.toBeInTheDocument());
    },
  );

  it.each([
    { shortcut: { ctrlKey: true }, controlled: false },
    { shortcut: { metaKey: true }, controlled: false },
    { shortcut: { ctrlKey: true }, controlled: true },
    { shortcut: { metaKey: true }, controlled: true },
  ])(
    "refocuses an already open search panel (%o)",
    async ({ shortcut, controlled }) => {
      const providers = Array.from({ length: 30 }, (_, index) =>
        createProvider({ id: `provider-${index}`, name: `Provider ${index}` }),
      );
      useDragSortMock.mockReturnValue({
        sortedProviders: providers,
        sensors: [],
        handleDragEnd: vi.fn(),
      });
      function SearchList() {
        const [open, setOpen] = useState(false);
        return (
          <div
            data-testid="scroll-container"
            style={{ height: 200, overflowY: "auto" }}
          >
            <ProviderList
              {...baseProps}
              providers={Object.fromEntries(
                providers.map((provider) => [provider.id, provider]),
              )}
              appId="claude"
              {...(controlled
                ? { searchOpen: open, onSearchOpenChange: setOpen }
                : {})}
            />
          </div>
        );
      }
      renderWithQueryClient(<SearchList />);
      fireEvent.keyDown(window, { key: "f", ...shortcut });
      const input = screen.getByRole("textbox", {
        name: "Search providers",
      }) as HTMLInputElement;
      await waitFor(() => expect(input).toHaveFocus());
      fireEvent.change(input, { target: { value: "Provider" } });
      const scrollIntoView = vi.fn();
      input.scrollIntoView = scrollIntoView;

      // jsdom has no layout: model scrolling away and assert the browser scroll request.
      const scrollContainer = screen.getByTestId("scroll-container");
      for (let invocation = 1; invocation <= 2; invocation++) {
        scrollContainer.scrollTop = 1000;
        screen.getByTestId("edit-provider-29").focus();
        expect(input).not.toHaveFocus();
        const event = new KeyboardEvent("keydown", {
          key: "f",
          ...shortcut,
          cancelable: true,
        });
        fireEvent(window, event);
        expect(event.defaultPrevented).toBe(true);
        await waitFor(() => expect(input).toHaveFocus());
        expect(input.selectionStart).toBe(0);
        expect(input.selectionEnd).toBe("Provider".length);
        expect(scrollIntoView).toHaveBeenCalledTimes(invocation);
        expect(scrollIntoView).toHaveBeenLastCalledWith({ block: "nearest" });
      }
    },
  );

  it.each(["input", "textarea", "select", "contenteditable"])(
    "does not steal the search shortcut from another %s",
    async (targetType) => {
      const provider = createProvider();
      useDragSortMock.mockReturnValue({
        sortedProviders: [provider],
        sensors: [],
        handleDragEnd: vi.fn(),
      });
      renderWithQueryClient(
        <ProviderList
          {...baseProps}
          providers={{ [provider.id]: provider }}
          appId="claude"
        />,
      );
      fireEvent.keyDown(window, { key: "f", ctrlKey: true });
      const input = screen.getByRole("textbox", { name: "Search providers" });
      await waitFor(() => expect(input).toHaveFocus());
      const scrollIntoView = vi.fn();
      input.scrollIntoView = scrollIntoView;
      const target = document.createElement(
        targetType === "contenteditable" ? "div" : targetType,
      );
      if (targetType === "contenteditable") {
        target.tabIndex = 0;
        // jsdom does not implement isContentEditable.
        Object.defineProperty(target, "isContentEditable", { value: true });
      }
      document.body.appendChild(target);
      try {
        target.focus();
        const event = new KeyboardEvent("keydown", {
          key: "f",
          ctrlKey: true,
          cancelable: true,
        });
        fireEvent(window, event);
        expect(event.defaultPrevented).toBe(false);
        expect(target).toHaveFocus();
        expect(scrollIntoView).not.toHaveBeenCalled();
      } finally {
        target.remove();
      }
    },
  );

  it("renders the search field into the mode row slot instead of the list", async () => {
    const alpha = createProvider({ id: "alpha", name: "Alpha Labs" });
    useDragSortMock.mockReturnValue({
      sortedProviders: [alpha],
      sensors: [],
      handleDragEnd: vi.fn(),
    });
    const slot = document.createElement("div");
    document.body.appendChild(slot);
    try {
      const { container } = renderWithQueryClient(
        <ProviderList
          {...baseProps}
          providers={{ alpha }}
          appId="codex"
          switchMode={switchMode()}
          searchSlot={slot}
        />,
      );

      fireEvent.keyDown(window, { key: "f", ctrlKey: true });
      const input = screen.getByRole("textbox", { name: "Search providers" });
      expect(slot).toContainElement(input);
      expect(container).not.toContainElement(input);
      await waitFor(() => expect(input).toHaveFocus());
    } finally {
      slot.remove();
    }
  });

  it("renders nothing for a mode row slot that is not mounted yet", () => {
    const alpha = createProvider({ id: "alpha", name: "Alpha Labs" });
    useDragSortMock.mockReturnValue({
      sortedProviders: [alpha],
      sensors: [],
      handleDragEnd: vi.fn(),
    });
    renderWithQueryClient(
      <ProviderList
        {...baseProps}
        providers={{ alpha }}
        appId="codex"
        switchMode={switchMode()}
        searchSlot={null}
        searchOpen
      />,
    );

    expect(
      screen.queryByRole("textbox", { name: "Search providers" }),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("provider-card-alpha")).toBeInTheDocument();
  });

  it("puts the search field beside the first section title and keeps it while no provider matches", async () => {
    const live = createProvider({ id: "live", name: "Live Labs" });
    const other = createProvider({ id: "other", name: "Other Works" });
    useDragSortMock.mockReturnValue({
      sortedProviders: [live, other],
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
        providers={{ live, other }}
        appId="opencode"
      />,
    );

    const heading = await screen.findByRole("heading", {
      name: "providerCard.section.added",
    });
    fireEvent.keyDown(window, { key: "f", ctrlKey: true });
    const input = screen.getByRole("textbox", { name: "Search providers" });
    // 同一行：标题和搜索框共用列表的第一行，第一个分组里不再重复画标题
    const header = input.closest("div.min-h-8");
    expect(header).toContainElement(heading);
    expect(
      screen.getAllByRole("heading", { name: "providerCard.section.added" }),
    ).toHaveLength(1);
    await waitFor(() => expect(input).toHaveFocus());

    fireEvent.change(input, { target: { value: "nothing-matches" } });
    expect(screen.getByText("No providers match your search.")).toBeVisible();
    // 结果清空时输入框还是同一个节点，焦点不丢
    expect(screen.getByRole("textbox", { name: "Search providers" })).toBe(
      input,
    );
    expect(input).toHaveFocus();
  });

  it("adds the search row only while searching when the list has no section title", async () => {
    const desk = createProvider({ id: "desk", name: "Desk Relay" });
    useDragSortMock.mockReturnValue({
      sortedProviders: [desk],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    const { container } = renderWithQueryClient(
      <ProviderList
        {...baseProps}
        providers={{ desk }}
        appId="claude-desktop"
        switchMode={switchMode()}
      />,
    );

    const list = container.firstElementChild!;
    expect(list.firstElementChild).toContainElement(
      screen.getByTestId("provider-card-desk"),
    );

    fireEvent.keyDown(window, { key: "f", ctrlKey: true });
    const input = screen.getByRole("textbox", { name: "Search providers" });
    expect(list.firstElementChild).toContainElement(input);
    expect(list.firstElementChild?.nextElementSibling).toContainElement(
      screen.getByTestId("provider-card-desk"),
    );

    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(input).not.toBeInTheDocument());
    expect(list.firstElementChild).toContainElement(
      screen.getByTestId("provider-card-desk"),
    );
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

  it("matches the API address and stops filtering once the panel closes", () => {
    const relay = createProvider({
      id: "relay",
      name: "My Relay",
      settingsConfig: {
        env: { ANTHROPIC_BASE_URL: "https://api.relay-example.com" },
      },
    });
    const other = createProvider({ id: "other", name: "Other" });

    useDragSortMock.mockReturnValue({
      sortedProviders: [relay, other],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    const props = {
      providers: { relay, other },
      currentProviderId: "",
      appId: "claude" as const,
      onSwitch: vi.fn(),
      onEdit: vi.fn(),
      onDelete: vi.fn(),
      onDuplicate: vi.fn(),
      onOpenWebsite: vi.fn(),
    };
    const onSearchOpenChange = vi.fn();
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const ui = (searchOpen: boolean) => (
      <QueryClientProvider client={queryClient}>
        <ProviderList
          {...props}
          searchOpen={searchOpen}
          onSearchOpenChange={onSearchOpenChange}
        />
      </QueryClientProvider>
    );
    const { rerender } = render(ui(true));

    fireEvent.change(
      screen.getByPlaceholderText("Search name, notes, or URL..."),
      { target: { value: "relay-example" } },
    );
    expect(screen.getByTestId("provider-card-relay")).toBeInTheDocument();
    expect(screen.queryByTestId("provider-card-other")).not.toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onSearchOpenChange).toHaveBeenCalledWith(false);

    rerender(ui(false));
    expect(screen.getByTestId("provider-card-other")).toBeInTheDocument();
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
        expect(props.isInConfig).toBe(true);
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
        isInConfig: true,
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
        isInConfig: false,
      });
      expect(latestCardProps.presentation.deleteDisabledReason).toBe(
        "pi.current.stateUnavailableHint",
      );
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
