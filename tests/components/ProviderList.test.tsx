import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { ReactElement } from "react";
import type { Provider } from "@/types";
import { ProviderList } from "@/components/providers/ProviderList";

const {
  modelTestProviderMock,
  dndContextPropsSpy,
  updateProviderMock,
  updateSortOrderMock,
  updateTrayMenuMock,
  addToFailoverQueueMock,
  removeFromFailoverQueueMock,
  failoverHookState,
} = vi.hoisted(() => ({
  modelTestProviderMock: vi.fn(),
  dndContextPropsSpy: vi.fn(),
  updateProviderMock: vi.fn(),
  updateSortOrderMock: vi.fn(),
  updateTrayMenuMock: vi.fn(),
  addToFailoverQueueMock: vi.fn(),
  removeFromFailoverQueueMock: vi.fn(),
  failoverHookState: {
    isAutoFailoverEnabled: false,
    failoverQueue: [] as Array<{ providerId: string; providerName: string }>,
  },
}));
const useDragSortMock = vi.fn();
const useSortableMock = vi.fn();
const providerCardRenderSpy = vi.fn();

vi.mock("@/hooks/useDragSort", () => ({
  useDragSort: (...args: unknown[]) => useDragSortMock(...args),
}));

vi.mock("@dnd-kit/core", async () => {
  const actual = await vi.importActual<any>("@dnd-kit/core");

  return {
    ...actual,
    DndContext: ({ children, ...props }: any) => {
      dndContextPropsSpy(props);
      return <div data-testid="provider-dnd-context">{children}</div>;
    },
    useDroppable: () => ({ setNodeRef: vi.fn() }),
  };
});

vi.mock("@/lib/api/providers", async () => {
  const actual = await vi.importActual<any>("@/lib/api/providers");

  return {
    ...actual,
    providersApi: {
      ...actual.providersApi,
      update: updateProviderMock,
      updateSortOrder: updateSortOrderMock,
      updateTrayMenu: updateTrayMenuMock,
    },
  };
});

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

vi.mock("@/lib/api/connectivity-check", () => ({
  modelTestProvider: modelTestProviderMock,
}));

vi.mock("@/lib/query/failover", () => ({
  useAutoFailoverEnabled: () => ({
    data: failoverHookState.isAutoFailoverEnabled,
  }),
  useFailoverQueue: () => ({ data: failoverHookState.failoverQueue }),
  useAddToFailoverQueue: () => ({
    mutate: vi.fn(),
    mutateAsync: addToFailoverQueueMock,
  }),
  useRemoveFromFailoverQueue: () => ({
    mutate: vi.fn(),
    mutateAsync: removeFromFailoverQueueMock,
  }),
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
  modelTestProviderMock.mockReset();
  providerCardRenderSpy.mockClear();
  dndContextPropsSpy.mockClear();
  updateProviderMock.mockReset();
  updateProviderMock.mockResolvedValue(undefined);
  updateSortOrderMock.mockReset();
  updateSortOrderMock.mockResolvedValue(undefined);
  updateTrayMenuMock.mockReset();
  updateTrayMenuMock.mockResolvedValue(undefined);
  addToFailoverQueueMock.mockReset();
  addToFailoverQueueMock.mockResolvedValue(undefined);
  removeFromFailoverQueueMock.mockReset();
  removeFromFailoverQueueMock.mockResolvedValue(undefined);
  failoverHookState.isAutoFailoverEnabled = false;
  failoverHookState.failoverQueue = [];

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

  it("passes provider group metadata and a group action menu to every card", () => {
    const groupedProvider = createProvider({
      id: "grouped",
      name: "Grouped Provider",
      meta: {
        providerGroupId: "group-team",
        providerGroupName: "Team",
        providerGroupSortIndex: 0,
      },
    });
    const ungroupedProvider = createProvider({
      id: "ungrouped",
      name: "Ungrouped Provider",
    });

    useDragSortMock.mockReturnValue({
      sortedProviders: [groupedProvider, ungroupedProvider],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    renderWithQueryClient(
      <ProviderList
        providers={{ grouped: groupedProvider, ungrouped: ungroupedProvider }}
        currentProviderId=""
        appId="claude"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByText("Team"));

    const renderedCardProps = providerCardRenderSpy.mock.calls.map(
      (call) => call[0] as { provider: Provider; groupMenu?: unknown },
    );
    const groupedCardProps = renderedCardProps.find(
      (props) => props.provider.id === "grouped",
    );
    const ungroupedCardProps = renderedCardProps.find(
      (props) => props.provider.id === "ungrouped",
    );

    expect(groupedCardProps?.provider.meta?.providerGroupName).toBe("Team");
    expect(groupedCardProps?.groupMenu).toBeTruthy();
    expect(ungroupedCardProps?.groupMenu).toBeTruthy();
  });

  it("renders group controls and headers in sticky-safe containers", () => {
    const groupedProvider = createProvider({
      id: "grouped",
      meta: {
        providerGroupId: "group-team",
        providerGroupName: "Team",
        providerGroupSortIndex: 0,
      },
    });

    useDragSortMock.mockReturnValue({
      sortedProviders: [groupedProvider],
      sensors: [],
      handleDragEnd: vi.fn(),
    });
    useSortableMock.mockImplementation(({ id }: { id: string }) => ({
      setNodeRef: vi.fn(),
      attributes: { "data-dnd-id": id },
      listeners: { onPointerDown: vi.fn() },
      transform: { x: 0, y: 0, scaleX: 1, scaleY: 1 },
      transition: "transform 150ms ease",
      isDragging: false,
    }));

    renderWithQueryClient(
      <ProviderList
        providers={{ grouped: groupedProvider }}
        currentProviderId=""
        appId="claude"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    const controls = screen.getByTestId("provider-group-controls");
    const group = screen.getByTestId("provider-group-group-team");
    const header = screen.getByTestId("provider-group-header-group-team");

    expect(controls).toHaveClass("sticky", "top-0");
    expect(group).toHaveClass("overflow-visible", "p-0");
    expect(group).not.toHaveAttribute("style");
    expect(header).toHaveClass("sticky");
    expect(header).toHaveStyle({ top: "8px" });
  });

  it("shows the active provider URL in a collapsed group", () => {
    const groupedProvider = createProvider({
      id: "active-provider",
      name: "Active Provider",
      websiteUrl: "https://active.example.test/v1",
      meta: {
        providerGroupId: "group-team",
        providerGroupName: "Team",
        providerGroupSortIndex: 0,
      },
    });

    useDragSortMock.mockReturnValue({
      sortedProviders: [groupedProvider],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    renderWithQueryClient(
      <ProviderList
        providers={{ "active-provider": groupedProvider }}
        currentProviderId="active-provider"
        appId="claude"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    const activeProvider = screen.getByTestId(
      "provider-group-active-provider-group-team",
    );
    expect(activeProvider).toHaveClass("w-full");
    expect(activeProvider).not.toHaveClass("max-w-[42%]");
    expect(
      screen.getByTestId("provider-group-active-url-group-team"),
    ).toHaveTextContent("https://active.example.test/v1");
  });

  it("places a provider group between ungrouped provider cards by sort index", () => {
    const before = createProvider({ id: "before", sortIndex: 0 });
    const groupedFirst = createProvider({
      id: "grouped-first",
      sortIndex: 1,
      meta: {
        providerGroupId: "group-team",
        providerGroupName: "Team",
        providerGroupSortIndex: 0,
      },
    });
    const groupedSecond = createProvider({
      id: "grouped-second",
      sortIndex: 2,
      meta: {
        providerGroupId: "group-team",
        providerGroupName: "Team",
        providerGroupSortIndex: 0,
      },
    });
    const after = createProvider({ id: "after", sortIndex: 3 });

    useDragSortMock.mockReturnValue({
      sortedProviders: [before, groupedFirst, groupedSecond, after],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    renderWithQueryClient(
      <ProviderList
        providers={{
          before,
          "grouped-first": groupedFirst,
          "grouped-second": groupedSecond,
          after,
        }}
        currentProviderId=""
        appId="claude"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    const beforeCard = screen.getByTestId("provider-card-before");
    const group = screen.getByTestId("provider-group-group-team");
    const afterCard = screen.getByTestId("provider-card-after");

    expect(
      beforeCard.compareDocumentPosition(group) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(
      group.compareDocumentPosition(afterCard) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
  });

  it("selects providers while sweeping across cards in group management mode", () => {
    const groupedProvider = createProvider({
      id: "grouped",
      meta: {
        providerGroupId: "group-team",
        providerGroupName: "Team",
        providerGroupSortIndex: 0,
      },
    });
    const first = createProvider({ id: "first" });
    const second = createProvider({ id: "second" });

    useDragSortMock.mockReturnValue({
      sortedProviders: [groupedProvider, first, second],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    renderWithQueryClient(
      <ProviderList
        providers={{ grouped: groupedProvider, first, second }}
        currentProviderId=""
        appId="claude"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByTestId("provider-group-manage-toggle"));

    const firstControl = screen.getByTestId(
      "provider-selection-control-first",
    );
    const secondControl = screen.getByTestId(
      "provider-selection-control-second",
    );
    fireEvent.pointerDown(firstControl, { button: 0, pointerId: 7 });
    fireEvent.pointerEnter(screen.getByTestId("sortable-provider-card-second"), {
      pointerId: 7,
    });
    fireEvent.pointerUp(window, { pointerId: 7 });

    expect(firstControl).toHaveAttribute("data-selected", "true");
    expect(secondControl).toHaveAttribute("data-selected", "true");
  });

  it("selects a provider with one click in group management mode", () => {
    const groupedProvider = createProvider({
      id: "grouped",
      meta: {
        providerGroupId: "group-team",
        providerGroupName: "Team",
        providerGroupSortIndex: 0,
      },
    });
    const first = createProvider({ id: "first" });

    useDragSortMock.mockReturnValue({
      sortedProviders: [groupedProvider, first],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    renderWithQueryClient(
      <ProviderList
        providers={{ grouped: groupedProvider, first }}
        currentProviderId=""
        appId="claude"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByTestId("provider-group-manage-toggle"));

    const firstControl = screen.getByTestId(
      "provider-selection-control-first",
    );
    fireEvent.pointerDown(firstControl, { button: 0, pointerId: 17 });
    fireEvent.pointerUp(window, { pointerId: 17 });

    expect(firstControl).toHaveAttribute("data-selected", "true");
  });

  it("adds every missing provider in a group to the failover queue", async () => {
    failoverHookState.isAutoFailoverEnabled = true;
    const first = createProvider({
      id: "group-first",
      meta: {
        providerGroupId: "group-team",
        providerGroupName: "Team",
        providerGroupSortIndex: 0,
      },
    });
    const second = createProvider({
      id: "group-second",
      meta: {
        providerGroupId: "group-team",
        providerGroupName: "Team",
        providerGroupSortIndex: 0,
      },
    });

    useDragSortMock.mockReturnValue({
      sortedProviders: [first, second],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    renderWithQueryClient(
      <ProviderList
        providers={{ "group-first": first, "group-second": second }}
        currentProviderId=""
        appId="claude"
        isProxyTakeover
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByTestId("provider-group-menu-group-team"));
    fireEvent.click(
      screen.getByTestId("provider-group-add-failover-group-team"),
    );

    await waitFor(() =>
      expect(addToFailoverQueueMock).toHaveBeenCalledTimes(2),
    );
    expect(addToFailoverQueueMock).toHaveBeenNthCalledWith(1, {
      appType: "claude",
      providerId: "group-first",
    });
    expect(addToFailoverQueueMock).toHaveBeenNthCalledWith(2, {
      appType: "claude",
      providerId: "group-second",
    });
  });

  it("removes queued providers in a group from the failover queue", async () => {
    failoverHookState.isAutoFailoverEnabled = true;
    failoverHookState.failoverQueue = [
      { providerId: "group-first", providerName: "First" },
    ];
    const first = createProvider({
      id: "group-first",
      meta: {
        providerGroupId: "group-team",
        providerGroupName: "Team",
        providerGroupSortIndex: 0,
      },
    });
    const second = createProvider({
      id: "group-second",
      meta: {
        providerGroupId: "group-team",
        providerGroupName: "Team",
        providerGroupSortIndex: 0,
      },
    });

    useDragSortMock.mockReturnValue({
      sortedProviders: [first, second],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    renderWithQueryClient(
      <ProviderList
        providers={{ "group-first": first, "group-second": second }}
        currentProviderId=""
        appId="claude"
        isProxyTakeover
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByTestId("provider-group-menu-group-team"));
    fireEvent.click(
      screen.getByTestId("provider-group-remove-failover-group-team"),
    );

    await waitFor(() =>
      expect(removeFromFailoverQueueMock).toHaveBeenCalledTimes(1),
    );
    expect(removeFromFailoverQueueMock).toHaveBeenCalledWith({
      appType: "claude",
      providerId: "group-first",
    });
  });

  it("sorts an ungrouped provider before a group dropped on the group outer target", async () => {
    const before = createProvider({ id: "before", sortIndex: 0 });
    const groupedFirst = createProvider({
      id: "grouped-first",
      sortIndex: 1,
      meta: {
        providerGroupId: "group-team",
        providerGroupName: "Team",
        providerGroupSortIndex: 0,
      },
    });
    const groupedSecond = createProvider({
      id: "grouped-second",
      sortIndex: 2,
      meta: {
        providerGroupId: "group-team",
        providerGroupName: "Team",
        providerGroupSortIndex: 0,
      },
    });
    const after = createProvider({ id: "after", sortIndex: 3 });

    useDragSortMock.mockReturnValue({
      sortedProviders: [before, groupedFirst, groupedSecond, after],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    renderWithQueryClient(
      <ProviderList
        providers={{
          before,
          "grouped-first": groupedFirst,
          "grouped-second": groupedSecond,
          after,
        }}
        currentProviderId=""
        appId="claude"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    const dndContextProps = dndContextPropsSpy.mock.calls[
      dndContextPropsSpy.mock.calls.length - 1
    ][0] as { onDragEnd: (event: unknown) => Promise<void> };
    await dndContextProps.onDragEnd({
      active: {
        id: "after",
        rect: {
          current: {
            translated: { top: 80, height: 20 },
          },
        },
      },
      over: {
        id: "provider-group:group-team",
        rect: { top: 140, height: 80 },
      },
    });

    expect(updateSortOrderMock).toHaveBeenCalledWith(
      [
        { id: "before", sortIndex: 0 },
        { id: "after", sortIndex: 1 },
        { id: "grouped-first", sortIndex: 2 },
        { id: "grouped-second", sortIndex: 3 },
      ],
      "claude",
    );
  });

  it("adds an ungrouped provider dropped in the middle of a group member card", async () => {
    const groupedProvider = createProvider({
      id: "grouped",
      meta: {
        providerGroupId: "group-team",
        providerGroupName: "Team",
        providerGroupSortIndex: 0,
      },
    });
    const ungroupedProvider = createProvider({ id: "ungrouped" });

    useDragSortMock.mockReturnValue({
      sortedProviders: [groupedProvider, ungroupedProvider],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    renderWithQueryClient(
      <ProviderList
        providers={{ grouped: groupedProvider, ungrouped: ungroupedProvider }}
        currentProviderId=""
        appId="claude"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    const dndContextProps = dndContextPropsSpy.mock.calls[
      dndContextPropsSpy.mock.calls.length - 1
    ][0] as { onDragEnd: (event: unknown) => Promise<void> };
    await dndContextProps.onDragEnd({
      active: {
        id: "ungrouped",
        rect: {
          current: {
            translated: { top: 130, height: 20 },
          },
        },
      },
      over: {
        id: "grouped",
        rect: { top: 100, height: 80 },
      },
    });

    expect(updateProviderMock).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "ungrouped",
        meta: {
          providerGroupId: "group-team",
          providerGroupName: "Team",
          providerGroupSortIndex: 0,
        },
      }),
      "claude",
    );
  });

  it("tests every provider in a group and renders the aggregate result", async () => {
    const first = createProvider({
      id: "group-first",
      meta: {
        providerGroupId: "group-team",
        providerGroupName: "Team",
        providerGroupSortIndex: 0,
      },
    });
    const second = createProvider({
      id: "group-second",
      meta: {
        providerGroupId: "group-team",
        providerGroupName: "Team",
        providerGroupSortIndex: 0,
      },
    });
    modelTestProviderMock
      .mockResolvedValueOnce({
        status: "operational",
        success: true,
        message: "ok",
        testedAt: 1,
        retryCount: 0,
      })
      .mockResolvedValueOnce({
        status: "degraded",
        success: true,
        message: "slow",
        testedAt: 2,
        retryCount: 0,
      });

    useDragSortMock.mockReturnValue({
      sortedProviders: [first, second],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    renderWithQueryClient(
      <ProviderList
        providers={{ "group-first": first, "group-second": second }}
        currentProviderId=""
        appId="claude"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByTestId("provider-group-model-test-group-team"));

    await waitFor(() => {
      expect(modelTestProviderMock).toHaveBeenCalledTimes(2);
    });

    expect(modelTestProviderMock).toHaveBeenNthCalledWith(
      1,
      "claude",
      "group-first",
    );
    expect(modelTestProviderMock).toHaveBeenNthCalledWith(
      2,
      "claude",
      "group-second",
    );
    expect(
      screen.getByTestId("provider-group-model-test-summary-group-team"),
    ).toHaveTextContent("1");
  });
});
