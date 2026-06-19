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
const streamCheckMocks = vi.hoisted(() => ({
  checkProvider: vi.fn(),
  isChecking: vi.fn(),
}));
const settingsApiMocks = vi.hoisted(() => ({
  get: vi.fn(),
  save: vi.fn(),
}));
const providerApiMocks = vi.hoisted(() => ({
  update: vi.fn(),
  updateSortOrder: vi.fn(),
  updateTrayMenu: vi.fn(),
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
    updateSortOrder: (...args: unknown[]) =>
      providerApiMocks.updateSortOrder(...args),
    updateTrayMenu: (...args: unknown[]) =>
      providerApiMocks.updateTrayMenu(...args),
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

vi.mock("@/lib/api/settings", () => ({
  settingsApi: {
    get: (...args: unknown[]) => settingsApiMocks.get(...args),
    save: (...args: unknown[]) => settingsApiMocks.save(...args),
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
    checkProvider: (...args: unknown[]) =>
      streamCheckMocks.checkProvider(...args),
    isChecking: (...args: unknown[]) => streamCheckMocks.isChecking(...args),
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
  Object.values(settingsApiMocks).forEach((mock) => mock.mockReset());
  Object.values(streamCheckMocks).forEach((mock) => mock.mockReset());
  providerApiMocks.update.mockResolvedValue(true);
  providerApiMocks.updateSortOrder.mockResolvedValue(true);
  providerApiMocks.updateTrayMenu.mockResolvedValue(true);
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
  settingsApiMocks.get.mockResolvedValue({ streamCheckConfirmed: true });
  settingsApiMocks.save.mockResolvedValue(true);
  streamCheckMocks.isChecking.mockReturnValue(false);

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

  it("uses remove-from-config when compact row remove is clicked", async () => {
    const providerAlpha = createProvider({ id: "alpha", name: "Alpha Labs" });
    const handleRemoveFromConfig = vi.fn();
    const handleDelete = vi.fn();

    providerApiMocks.getOpenCodeLiveProviderIds.mockResolvedValue(["alpha"]);
    useDragSortMock.mockReturnValue({
      sortedProviders: [providerAlpha],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    renderWithQueryClient(
      <ProviderList
        providers={{ alpha: providerAlpha }}
        currentProviderId=""
        appId="opencode"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={handleDelete}
        onRemoveFromConfig={handleRemoveFromConfig}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /Compact/ }));

    const removeButton = await screen.findByRole("button", {
      name: /Remove|移除/,
    });
    fireEvent.click(removeButton);

    expect(handleRemoveFromConfig).toHaveBeenCalledWith(providerAlpha);
    expect(handleDelete).not.toHaveBeenCalled();
  });

  it("does not steal Ctrl+F from an already focused input", () => {
    const providerAlpha = createProvider({ id: "alpha", name: "Alpha Labs" });

    useDragSortMock.mockReturnValue({
      sortedProviders: [providerAlpha],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    renderWithQueryClient(
      <>
        <input aria-label="Dialog search" />
        <ProviderList
          providers={{ alpha: providerAlpha }}
          currentProviderId=""
          appId="claude"
          onSwitch={vi.fn()}
          onEdit={vi.fn()}
          onDelete={vi.fn()}
          onDuplicate={vi.fn()}
          onOpenWebsite={vi.fn()}
        />
      </>,
    );

    const dialogSearch = screen.getByRole("textbox", {
      name: "Dialog search",
    });
    const providerSearch = screen.getByRole("textbox", {
      name: "Search providers",
    });
    dialogSearch.focus();

    fireEvent.keyDown(window, { key: "f", ctrlKey: true });

    expect(dialogSearch).toHaveFocus();
    expect(providerSearch).not.toHaveFocus();
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

  it("uses the active provider as the folded group surface and common-config source", async () => {
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
        currentProviderId="beta"
        appId="claude"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    expect(screen.queryByTestId("provider-card-alpha")).not.toBeInTheDocument();
    expect(screen.getByTestId("provider-card-beta")).toBeInTheDocument();
    expect(providerCardRenderSpy).toHaveBeenCalledTimes(1);
    expect(providerCardRenderSpy.mock.calls[0][0].provider.id).toBe("beta");
    expect(screen.getByTestId("is-current-beta")).toHaveTextContent("current");

    fireEvent.click(screen.getByTestId("drawer-beta"));

    const drawer = screen.getByTestId("provider-config-drawer-group:minimax");
    expect(within(drawer).getByText("Minimax API A")).toBeInTheDocument();
    expect(within(drawer).getByText("Minimax API B")).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("checkbox", {
        name: "Use group API key for Minimax API A",
      }),
    );

    await waitFor(() => expect(providerApiMocks.update).toHaveBeenCalled());
    const [updatedProvider, appId, originalId] =
      providerApiMocks.update.mock.calls[0];

    expect(appId).toBe("claude");
    expect(originalId).toBe("alpha");
    expect(updatedProvider.settingsConfig.env.ANTHROPIC_AUTH_TOKEN).toBe(
      "sk-beta-secret-654321",
    );
    expect(JSON.stringify(updatedProvider.meta)).not.toContain(
      "sk-beta-secret-654321",
    );
  });

  it("folds duplicate branded providers with the same name into one drawer", () => {
    const providerAlpha = createProvider({
      id: "kimi-a",
      name: "Kimi For Coding",
      category: "cn_official",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://api-a.example.com/coding",
          ANTHROPIC_AUTH_TOKEN: "sk-alpha-secret-123456",
          ANTHROPIC_DEFAULT_SONNET_MODEL: "kimi-for-coding",
        },
      },
    });
    const providerBeta = createProvider({
      id: "kimi-b",
      name: "Kimi For Coding",
      category: "cn_official",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://api-b.example.com/coding",
          ANTHROPIC_AUTH_TOKEN: "sk-beta-secret-654321",
          ANTHROPIC_DEFAULT_OPUS_MODEL: "kimi-for-coding",
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
        providers={{ "kimi-a": providerAlpha, "kimi-b": providerBeta }}
        currentProviderId="kimi-b"
        appId="claude"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    expect(
      screen.queryByTestId("provider-card-kimi-a"),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("provider-card-kimi-b")).toBeInTheDocument();
    expect(screen.getByTestId("drawer-kimi-b")).toHaveTextContent("2");

    fireEvent.click(screen.getByTestId("drawer-kimi-b"));

    const drawer = screen.getByTestId(
      "provider-config-drawer-name:kimi-for-coding",
    );
    expect(
      screen.getByTestId("provider-config-drawer-row-kimi-a"),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId("provider-config-drawer-row-kimi-b"),
    ).toBeInTheDocument();
    expect(within(drawer).getByText("api-a.example.com")).toBeInTheDocument();
    expect(within(drawer).getByText("api-b.example.com")).toBeInTheDocument();
  });

  it("uses the folded group id as the sortable id for grouped providers", () => {
    const providerAlpha = createProvider({
      id: "kimi-a",
      name: "Kimi For Coding",
      category: "cn_official",
    });
    const providerBeta = createProvider({
      id: "kimi-b",
      name: "Kimi For Coding",
      category: "cn_official",
    });

    useDragSortMock.mockReturnValue({
      sortedProviders: [providerAlpha, providerBeta],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    renderWithQueryClient(
      <ProviderList
        providers={{ "kimi-a": providerAlpha, "kimi-b": providerBeta }}
        currentProviderId="kimi-b"
        appId="claude"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    expect(screen.getByTestId("drag-attr-kimi-b")).toHaveTextContent(
      "name:kimi-for-coding",
    );
    expect(useSortableMock).toHaveBeenCalledWith(
      expect.objectContaining({ id: "name:kimi-for-coding" }),
    );
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

  it("confirms and forwards batch delete through the batch callback", () => {
    const providerAlpha = createProvider({ id: "alpha", name: "Alpha Labs" });
    const providerBeta = createProvider({ id: "beta", name: "Beta Works" });
    const handleDelete = vi.fn();
    const handleBatchDelete = vi.fn();

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
        onBatchDelete={handleBatchDelete}
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

    expect(handleBatchDelete).toHaveBeenCalledTimes(1);
    expect(handleBatchDelete).toHaveBeenCalledWith([
      providerAlpha,
      providerBeta,
    ]);
    expect(handleDelete).not.toHaveBeenCalled();
  });

  it("does not offer batch delete without a batch delete callback", () => {
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
    fireEvent.click(screen.getByTestId("select-beta"));

    expect(
      screen.queryByRole("button", { name: "Delete selected" }),
    ).not.toBeInTheDocument();
  });

  it("adds selected providers to live config sequentially", async () => {
    const providerAlpha = createProvider({ id: "alpha", name: "Alpha Labs" });
    const providerBeta = createProvider({ id: "beta", name: "Beta Works" });
    let resolveAlpha!: () => void;
    const alphaSwitch = new Promise<void>((resolve) => {
      resolveAlpha = resolve;
    });
    const handleSwitch = vi.fn((provider: Provider) =>
      provider.id === "alpha" ? alphaSwitch : Promise.resolve(),
    );

    useDragSortMock.mockReturnValue({
      sortedProviders: [providerAlpha, providerBeta],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    renderWithQueryClient(
      <ProviderList
        providers={{ alpha: providerAlpha, beta: providerBeta }}
        currentProviderId=""
        appId="opencode"
        onSwitch={handleSwitch}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    await screen.findByRole("button", { name: "Cards" });
    fireEvent.click(screen.getByTestId("select-alpha"));
    fireEvent.click(screen.getByTestId("select-beta"));

    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Add selected" }),
      ).toBeInTheDocument(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Add selected" }));

    expect(handleSwitch).toHaveBeenCalledTimes(1);
    expect(handleSwitch).toHaveBeenCalledWith(providerAlpha);

    resolveAlpha();

    await waitFor(() => expect(handleSwitch).toHaveBeenCalledTimes(2));
    expect(handleSwitch).toHaveBeenNthCalledWith(2, providerBeta);
  });

  it("forwards batch remove-from-config through the batch callback", async () => {
    const providerAlpha = createProvider({ id: "alpha", name: "Alpha Labs" });
    const providerBeta = createProvider({ id: "beta", name: "Beta Works" });
    const handleRemoveFromConfig = vi.fn();
    const handleBatchRemoveFromConfig = vi.fn();

    providerApiMocks.getOpenCodeLiveProviderIds.mockResolvedValue([
      "alpha",
      "beta",
    ]);
    useDragSortMock.mockReturnValue({
      sortedProviders: [providerAlpha, providerBeta],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    renderWithQueryClient(
      <ProviderList
        providers={{ alpha: providerAlpha, beta: providerBeta }}
        currentProviderId=""
        appId="opencode"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onRemoveFromConfig={handleRemoveFromConfig}
        onBatchRemoveFromConfig={handleBatchRemoveFromConfig}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    await screen.findByRole("button", { name: "Cards" });
    fireEvent.click(screen.getByTestId("select-alpha"));
    fireEvent.click(screen.getByTestId("select-beta"));

    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Remove selected" }),
      ).toBeInTheDocument(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Remove selected" }));

    expect(handleBatchRemoveFromConfig).toHaveBeenCalledTimes(1);
    expect(handleBatchRemoveFromConfig).toHaveBeenCalledWith([
      providerAlpha,
      providerBeta,
    ]);
    expect(handleRemoveFromConfig).not.toHaveBeenCalled();
  });

  it("does not offer batch remove-from-config without a batch callback", async () => {
    const providerAlpha = createProvider({ id: "alpha", name: "Alpha Labs" });
    const providerBeta = createProvider({ id: "beta", name: "Beta Works" });

    providerApiMocks.getOpenCodeLiveProviderIds.mockResolvedValue([
      "alpha",
      "beta",
    ]);
    useDragSortMock.mockReturnValue({
      sortedProviders: [providerAlpha, providerBeta],
      sensors: [],
      handleDragEnd: vi.fn(),
    });

    renderWithQueryClient(
      <ProviderList
        providers={{ alpha: providerAlpha, beta: providerBeta }}
        currentProviderId=""
        appId="opencode"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onRemoveFromConfig={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />,
    );

    await screen.findByRole("button", { name: "Cards" });
    fireEvent.click(screen.getByTestId("select-alpha"));
    fireEvent.click(screen.getByTestId("select-beta"));

    await waitFor(() =>
      expect(screen.getByText("2 selected")).toBeInTheDocument(),
    );
    expect(
      screen.queryByRole("button", { name: "Remove selected" }),
    ).not.toBeInTheDocument();
  });

  it("tests every selected provider directly", async () => {
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
    fireEvent.click(screen.getByTestId("select-beta"));
    fireEvent.click(screen.getByRole("button", { name: "Test selected" }));

    await waitFor(() =>
      expect(streamCheckMocks.checkProvider).toHaveBeenCalledTimes(2),
    );
    expect(streamCheckMocks.checkProvider).toHaveBeenNthCalledWith(
      1,
      "alpha",
      "Alpha Labs",
    );
    expect(streamCheckMocks.checkProvider).toHaveBeenNthCalledWith(
      2,
      "beta",
      "Beta Works",
    );
  });
});
