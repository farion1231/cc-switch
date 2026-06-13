import {
  render,
  screen,
  fireEvent,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { ReactElement } from "react";
import type { Provider } from "@/types";
import { ProviderList } from "@/components/providers/ProviderList";

const useDragSortMock = vi.fn();
const useSortableMock = vi.fn();
const providerCardRenderSpy = vi.fn();
const providerApiMocks = vi.hoisted(() => ({
  update: vi.fn(),
  getOpenCodeLiveProviderIds: vi.fn(),
  getClaudeDesktopStatus: vi.fn(),
  importOpenCodeFromLive: vi.fn(),
  importOpenClawFromLive: vi.fn(),
  importHermesFromLive: vi.fn(),
  importClaudeDesktopFromClaude: vi.fn(),
  importDefault: vi.fn(),
}));

vi.mock("@/lib/api/providers", () => ({
  providersApi: {
    update: (...args: unknown[]) => providerApiMocks.update(...args),
    getOpenCodeLiveProviderIds: (...args: unknown[]) =>
      providerApiMocks.getOpenCodeLiveProviderIds(...args),
    getClaudeDesktopStatus: (...args: unknown[]) =>
      providerApiMocks.getClaudeDesktopStatus(...args),
    importOpenCodeFromLive: (...args: unknown[]) =>
      providerApiMocks.importOpenCodeFromLive(...args),
    importOpenClawFromLive: (...args: unknown[]) =>
      providerApiMocks.importOpenClawFromLive(...args),
    importHermesFromLive: (...args: unknown[]) =>
      providerApiMocks.importHermesFromLive(...args),
    importClaudeDesktopFromClaude: (...args: unknown[]) =>
      providerApiMocks.importClaudeDesktopFromClaude(...args),
    importDefault: (...args: unknown[]) =>
      providerApiMocks.importDefault(...args),
  },
}));

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
      isSelected,
      onSelectedChange,
      groupCount,
      onToggleDrawer,
    } = props;

    return (
      <div data-testid={`provider-card-${provider.id}`}>
        {onSelectedChange && (
          <button
            data-testid={`select-${provider.id}`}
            data-selected={isSelected ? "true" : "false"}
            onClick={() => onSelectedChange(!isSelected)}
          >
            select
          </button>
        )}
        {onToggleDrawer && (
          <button
            data-testid={`drawer-${provider.id}`}
            onClick={() => onToggleDrawer()}
          >
            drawer {groupCount}
          </button>
        )}
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
  Object.values(providerApiMocks).forEach((mock) => mock.mockReset());
  providerApiMocks.update.mockResolvedValue(true);
  providerApiMocks.getOpenCodeLiveProviderIds.mockResolvedValue([]);
  providerApiMocks.getClaudeDesktopStatus.mockResolvedValue({
    supported: true,
    configured: false,
    proxyRunning: false,
    staleRawModels: false,
    missingRouteMappings: false,
    gatewayTokenConfigured: true,
  });
  providerApiMocks.importOpenCodeFromLive.mockResolvedValue(0);
  providerApiMocks.importOpenClawFromLive.mockResolvedValue(0);
  providerApiMocks.importHermesFromLive.mockResolvedValue(0);
  providerApiMocks.importClaudeDesktopFromClaude.mockResolvedValue(0);
  providerApiMocks.importDefault.mockResolvedValue(false);

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

  it("filters providers with the visible search input across id, base URL, and model", () => {
    const providerAlpha = createProvider({
      id: "alpha",
      name: "Alpha Labs",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://alpha.example.com/v1",
          ANTHROPIC_DEFAULT_SONNET_MODEL: "sonnet-alpha",
        },
      },
    });
    const providerBeta = createProvider({
      id: "beta",
      name: "Beta Works",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://beta.example.com/v1",
          ANTHROPIC_DEFAULT_OPUS_MODEL: "opus-beta",
        },
      },
    });

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

    const searchInput = screen.getByPlaceholderText(
      "Search providers, URLs, models, or key fingerprints...",
    );

    // Initially both providers are rendered
    expect(screen.getByTestId("provider-card-alpha")).toBeInTheDocument();
    expect(screen.getByTestId("provider-card-beta")).toBeInTheDocument();

    fireEvent.change(searchInput, { target: { value: "opus-beta" } });
    expect(screen.queryByTestId("provider-card-alpha")).not.toBeInTheDocument();
    expect(screen.getByTestId("provider-card-beta")).toBeInTheDocument();

    fireEvent.change(searchInput, { target: { value: "alpha.example.com" } });
    expect(screen.getByTestId("provider-card-alpha")).toBeInTheDocument();
    expect(screen.queryByTestId("provider-card-beta")).not.toBeInTheDocument();

    fireEvent.change(searchInput, { target: { value: "gamma" } });
    expect(screen.queryByTestId("provider-card-alpha")).not.toBeInTheDocument();
    expect(screen.queryByTestId("provider-card-beta")).not.toBeInTheDocument();
    expect(
      screen.getByText("No providers match your search."),
    ).toBeInTheDocument();
  });

  it("switches to compact mode and renders compact rows", () => {
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

    fireEvent.click(screen.getByRole("button", { name: /Compact/ }));

    expect(
      screen.getByTestId("provider-compact-row-alpha"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("provider-compact-row-beta")).toBeInTheDocument();
    expect(screen.queryByTestId("provider-card-alpha")).not.toBeInTheDocument();
  });

  it("selects visible providers and shows the selected count", () => {
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

    fireEvent.click(screen.getByTestId("select-alpha"));

    expect(screen.getByText("1 selected")).toBeInTheDocument();
    expect(screen.getByTestId("select-alpha")).toHaveAttribute(
      "data-selected",
      "true",
    );
  });

  it("opens a provider group drawer with safe sub-config summaries", () => {
    const providerAlpha = createProvider({
      id: "alpha",
      name: "Minimax API A",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://api.minimax.test/v1",
          ANTHROPIC_AUTH_TOKEN: "sk-alpha-secret-123456",
          ANTHROPIC_DEFAULT_SONNET_MODEL: "minimax-2.5",
        },
      },
      meta: { providerGroup: "Minimax" },
    });
    const providerBeta = createProvider({
      id: "beta",
      name: "Minimax API B",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://api.minimax.test/v1",
          ANTHROPIC_AUTH_TOKEN: "sk-beta-secret-654321",
          ANTHROPIC_DEFAULT_OPUS_MODEL: "minimax-2.7",
        },
      },
      meta: { providerGroup: "Minimax" },
    });

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

    fireEvent.click(screen.getByTestId("drawer-alpha"));

    const drawer = screen.getByTestId("provider-config-drawer-group:minimax");
    const alphaRow = screen.getByTestId("provider-config-drawer-row-alpha");
    expect(within(drawer).getByText("Minimax API A")).toBeInTheDocument();
    expect(within(drawer).getByText("Minimax API B")).toBeInTheDocument();
    expect(within(alphaRow).getByText("sk-alp...3456")).toBeInTheDocument();
    expect(
      screen.queryByText("sk-alpha-secret-123456"),
    ).not.toBeInTheDocument();
  });

  it("applies group common API key without storing raw secrets in metadata", async () => {
    const providerAlpha = createProvider({
      id: "alpha",
      name: "Minimax API A",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://api.minimax.test/v1",
          ANTHROPIC_AUTH_TOKEN: "sk-alpha-secret-123456",
        },
      },
      meta: { providerGroup: "Minimax" },
    });
    const providerBeta = createProvider({
      id: "beta",
      name: "Minimax API B",
      settingsConfig: { env: {} },
      meta: { providerGroup: "Minimax" },
    });

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

    fireEvent.click(screen.getByTestId("drawer-alpha"));
    fireEvent.click(
      screen.getByRole("checkbox", {
        name: "Use group API key for Minimax API B",
      }),
    );

    await waitFor(() => expect(providerApiMocks.update).toHaveBeenCalled());
    const [updatedProvider, appId, originalId] =
      providerApiMocks.update.mock.calls[0];

    expect(appId).toBe("claude");
    expect(originalId).toBe("beta");
    expect(updatedProvider.settingsConfig.env.ANTHROPIC_AUTH_TOKEN).toBe(
      "sk-alpha-secret-123456",
    );
    expect(updatedProvider.meta.groupCommonConfigEnabled).toEqual({
      apiKey: true,
    });
    expect(JSON.stringify(updatedProvider.meta)).not.toContain(
      "sk-alpha-secret-123456",
    );
  });

  it("confirms and forwards batch delete for selected providers", () => {
    const providerAlpha = createProvider({ id: "alpha", name: "Alpha Labs" });
    const providerBeta = createProvider({ id: "beta", name: "Beta Works" });
    const handleDelete = vi.fn();

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
        onDelete={handleDelete}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByTestId("select-alpha"));
    fireEvent.click(screen.getByTestId("select-beta"));
    fireEvent.click(screen.getByRole("button", { name: "Delete selected" }));
    fireEvent.click(
      screen.getByRole("button", { name: "Delete selected providers" }),
    );

    expect(handleDelete).toHaveBeenCalledWith(providerAlpha);
    expect(handleDelete).toHaveBeenCalledWith(providerBeta);
  });
});
