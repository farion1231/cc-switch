import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { ReactElement } from "react";
import { http, HttpResponse } from "msw";
import type { Provider } from "@/types";
import { ProviderList } from "@/components/providers/ProviderList";
import { server } from "../msw/server";

const TAURI_ENDPOINT = "http://tauri.local";

const useDragSortMock = vi.fn();
const useSortableMock = vi.fn();
const providerCardRenderSpy = vi.fn();

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
});
