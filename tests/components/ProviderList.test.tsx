import { render, screen, fireEvent, act } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { ReactElement } from "react";
import type { Provider } from "@/types";
import { ProviderList } from "@/components/providers/ProviderList";

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

// Stub the per-app takeover mutation so the routing guard's toggle ordering is
// observable; settings get/save are stubbed to keep the auto-* flags false
// (so the guard takes the confirm-dialog path) and avoid hitting the backend.
const proxyMocks = vi.hoisted(() => ({ takeoverMutateAsync: vi.fn() }));

vi.mock("@/lib/query/proxy", async () => {
  const actual = await vi.importActual<any>("@/lib/query/proxy");
  return {
    ...actual,
    useSetProxyTakeoverForApp: () => ({
      mutateAsync: proxyMocks.takeoverMutateAsync,
      isPending: false,
    }),
  };
});

vi.mock("@/lib/api/settings", async () => {
  const actual = await vi.importActual<any>("@/lib/api/settings");
  return {
    ...actual,
    settingsApi: {
      ...actual.settingsApi,
      get: vi.fn().mockResolvedValue({}),
      save: vi.fn().mockResolvedValue(undefined),
    },
  };
});

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

  // The pure decision table is unit-tested elsewhere; these cover the wiring:
  // toggle ordering, the ban-safety invariant (failed switch never enables
  // takeover), and the in-flight flag being held across the awaited switch.
  describe("routing auto-toggle guard", () => {
    function deferred<T>() {
      let resolve!: (value: T) => void;
      const promise = new Promise<T>((r) => {
        resolve = r;
      });
      return { promise, resolve };
    }

    const lastCardProps = () =>
      providerCardRenderSpy.mock.calls.at(-1)?.[0] as Record<string, unknown>;

    beforeEach(() => {
      proxyMocks.takeoverMutateAsync.mockReset().mockResolvedValue(undefined);
    });

    // Official under takeover: must disable routing BEFORE switching, and the
    // switch must be awaited so the in-flight flag isn't released early.
    it("disable path: disables takeover before switching, and holds in-flight until the switch resolves", async () => {
      const calls: string[] = [];
      proxyMocks.takeoverMutateAsync.mockImplementation(
        async (args: { enabled: boolean }) => {
          calls.push(`takeover:${args.enabled}`);
        },
      );
      const official = createProvider({
        id: "official",
        name: "Official",
        category: "official",
        settingsConfig: {},
      });
      useDragSortMock.mockReturnValue({
        sortedProviders: [official],
        sensors: [],
        handleDragEnd: vi.fn(),
      });
      const switchPromise = deferred<boolean>();
      const onSwitch = vi.fn(() => {
        calls.push("switch");
        return switchPromise.promise;
      });

      renderWithQueryClient(
        <ProviderList
          providers={{ official }}
          currentProviderId=""
          appId="claude"
          isProxyTakeover
          onSwitch={onSwitch}
          onEdit={vi.fn()}
          onDelete={vi.fn()}
          onDuplicate={vi.fn()}
          onOpenWebsite={vi.fn()}
        />,
      );

      fireEvent.click(screen.getByTestId("switch-official"));
      const confirmBtn = await screen.findByRole("button", {
        name: "关闭路由并切换",
      });
      await act(async () => {
        fireEvent.click(confirmBtn);
      });

      expect(calls).toEqual(["takeover:false", "switch"]);
      expect(onSwitch).toHaveBeenCalledWith(official, {
        fromRoutingGuard: true,
      });
      // Switch still pending → guard stays in-flight (regression: missing await
      // would let `finally` clear the flag here).
      expect(lastCardProps().isRoutingSwitchPending).toBe(true);

      // A second trigger while in-flight is ignored.
      fireEvent.click(screen.getByTestId("switch-official"));
      expect(calls).toEqual(["takeover:false", "switch"]);

      await act(async () => {
        switchPromise.resolve(true);
      });
      expect(lastCardProps().isRoutingSwitchPending).toBe(false);
    });

    // Official detection is the explicit category ONLY — an empty config (no
    // base URL / key) must NOT be treated as official: it can't be told apart
    // from a custom provider that just isn't filled in yet.
    it("does not treat a category-less empty-config provider as official", async () => {
      const incomplete = createProvider({
        id: "incomplete",
        name: "Incomplete",
        settingsConfig: {},
      });
      useDragSortMock.mockReturnValue({
        sortedProviders: [incomplete],
        sensors: [],
        handleDragEnd: vi.fn(),
      });
      const onSwitch = vi.fn(async () => true);

      renderWithQueryClient(
        <ProviderList
          providers={{ incomplete }}
          currentProviderId=""
          appId="claude"
          isProxyTakeover
          onSwitch={onSwitch}
          onEdit={vi.fn()}
          onDelete={vi.fn()}
          onDuplicate={vi.fn()}
          onOpenWebsite={vi.fn()}
        />,
      );

      await act(async () => {
        fireEvent.click(screen.getByTestId("switch-incomplete"));
      });

      // Direct switch: no confirm dialog, no takeover toggle.
      expect(
        screen.queryByRole("button", { name: "关闭路由并切换" }),
      ).toBeNull();
      expect(proxyMocks.takeoverMutateAsync).not.toHaveBeenCalled();
      expect(onSwitch).toHaveBeenCalledWith(incomplete);
    });

    // Needs-routing, not yet routed: must switch FIRST, then enable takeover —
    // only when the switch reports success.
    it("enable path: switches first, then enables takeover when the switch succeeds", async () => {
      const calls: string[] = [];
      proxyMocks.takeoverMutateAsync.mockImplementation(
        async (args: { enabled: boolean }) => {
          calls.push(`takeover:${args.enabled}`);
        },
      );
      const routed = createProvider({
        id: "routed",
        name: "Routed",
        settingsConfig: { env: { ANTHROPIC_BASE_URL: "https://example.com" } },
        meta: { apiFormat: "openai_chat" } as Provider["meta"],
      });
      useDragSortMock.mockReturnValue({
        sortedProviders: [routed],
        sensors: [],
        handleDragEnd: vi.fn(),
      });
      const onSwitch = vi.fn(async () => {
        calls.push("switch");
        return true;
      });

      renderWithQueryClient(
        <ProviderList
          providers={{ routed }}
          currentProviderId=""
          appId="claude"
          isProxyTakeover={false}
          onSwitch={onSwitch}
          onEdit={vi.fn()}
          onDelete={vi.fn()}
          onDuplicate={vi.fn()}
          onOpenWebsite={vi.fn()}
        />,
      );

      fireEvent.click(screen.getByTestId("switch-routed"));
      const confirmBtn = await screen.findByRole("button", {
        name: "开启路由并启用",
      });
      await act(async () => {
        fireEvent.click(confirmBtn);
      });

      expect(calls).toEqual(["switch", "takeover:true"]);
    });

    // Ban-safety: if the switch fails, takeover must NOT be enabled (otherwise an
    // official provider could keep running under the proxy).
    it("enable path: does not enable takeover when the switch fails", async () => {
      const routed = createProvider({
        id: "routed",
        name: "Routed",
        settingsConfig: { env: { ANTHROPIC_BASE_URL: "https://example.com" } },
        meta: { apiFormat: "openai_chat" } as Provider["meta"],
      });
      useDragSortMock.mockReturnValue({
        sortedProviders: [routed],
        sensors: [],
        handleDragEnd: vi.fn(),
      });
      const onSwitch = vi.fn(async () => false);

      renderWithQueryClient(
        <ProviderList
          providers={{ routed }}
          currentProviderId=""
          appId="claude"
          isProxyTakeover={false}
          onSwitch={onSwitch}
          onEdit={vi.fn()}
          onDelete={vi.fn()}
          onDuplicate={vi.fn()}
          onOpenWebsite={vi.fn()}
        />,
      );

      fireEvent.click(screen.getByTestId("switch-routed"));
      const confirmBtn = await screen.findByRole("button", {
        name: "开启路由并启用",
      });
      await act(async () => {
        fireEvent.click(confirmBtn);
      });

      expect(onSwitch).toHaveBeenCalledTimes(1);
      expect(proxyMocks.takeoverMutateAsync).not.toHaveBeenCalled();
    });
  });
});
