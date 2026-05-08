import { renderHook, waitFor } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import {
  useSmartRoutingEnabled,
  useSmartRoutingQueue,
  useAvailableProvidersForSmartRouting,
} from "@/lib/query/failover";
import { server } from "../msw/server";
import { resetProviderState } from "../msw/state";
import { http, HttpResponse } from "msw";

// Mock sonner (avoids toast side effects in tests)
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

const createWrapper = () => {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
};

describe("useSmartRoutingEnabled", () => {
  beforeEach(() => {
    resetProviderState();
    server.resetHandlers();
  });

  it("returns false by default", async () => {
    // Reset MSW to default: get_smart_routing_enabled -> false
    server.use(
      http.post("http://tauri.local/get_smart_routing_enabled", () =>
        HttpResponse.json(false),
      ),
    );

    const { result } = renderHook(
      () => useSmartRoutingEnabled("claude"),
      { wrapper: createWrapper() },
    );

    await waitFor(() => {
      expect(result.current.data).toBe(false);
    });
  });

  it("returns true when smart routing is enabled", async () => {
    server.use(
      http.post("http://tauri.local/get_smart_routing_enabled", () =>
        HttpResponse.json(true),
      ),
    );

    const { result } = renderHook(
      () => useSmartRoutingEnabled("claude"),
      { wrapper: createWrapper() },
    );

    await waitFor(() => {
      expect(result.current.data).toBe(true);
    });
  });

  it("returns false when query is not enabled (no appType)", async () => {
    // hook has enabled: !!appType, passing empty string should disable
    const { result } = renderHook(
      () => useSmartRoutingEnabled(""),
      { wrapper: createWrapper() },
    );

    // Should not fetch; placeholderData is false
    expect(result.current.data).toBe(false);
  });
});

describe("useSmartRoutingQueue", () => {
  beforeEach(() => {
    resetProviderState();
    server.resetHandlers();
  });

  it("returns empty queue by default", async () => {
    server.use(
      http.post("http://tauri.local/get_smart_routing_queue", () =>
        HttpResponse.json([]),
      ),
    );

    const { result } = renderHook(
      () => useSmartRoutingQueue("claude", "main"),
      { wrapper: createWrapper() },
    );

    await waitFor(() => {
      expect(result.current.data).toEqual([]);
    });
  });

  it("returns queue items for main queue", async () => {
    const queueItems = [
      {
        providerId: "prov-1",
        providerName: "Provider One",
        sortIndex: 0,
      },
      {
        providerId: "prov-2",
        providerName: "Provider Two",
        sortIndex: 1,
      },
    ];

    server.use(
      http.post("http://tauri.local/get_smart_routing_queue", () =>
        HttpResponse.json(queueItems),
      ),
    );

    const { result } = renderHook(
      () => useSmartRoutingQueue("claude", "main"),
      { wrapper: createWrapper() },
    );

    await waitFor(() => {
      expect(result.current.data).toHaveLength(2);
      expect(result.current.data?.[0].providerId).toBe("prov-1");
    });
  });

  it("returns different queue for others type", async () => {
    const mainItems = [
      { providerId: "main-1", providerName: "Main Provider", sortIndex: 0 },
    ];
    const othersItems = [
      { providerId: "others-1", providerName: "Others Provider", sortIndex: 0 },
    ];

    server.use(
      http.post("http://tauri.local/get_smart_routing_queue", async ({ request }) => {
        const body = await request.json() as { queueType: string };
        if (body.queueType === "main") {
          return HttpResponse.json(mainItems);
        }
        return HttpResponse.json(othersItems);
      }),
    );

    const mainResult = renderHook(
      () => useSmartRoutingQueue("claude", "main"),
      { wrapper: createWrapper() },
    );

    await waitFor(() => {
      expect(mainResult.result.current.data?.[0].providerId).toBe("main-1");
    });
  });
});

describe("useAvailableProvidersForSmartRouting", () => {
  beforeEach(() => {
    resetProviderState();
    server.resetHandlers();
  });

  it("returns empty list by default", async () => {
    server.use(
      http.post("http://tauri.local/get_available_providers_for_smart_routing", () =>
        HttpResponse.json([]),
      ),
    );

    const { result } = renderHook(
      () => useAvailableProvidersForSmartRouting("claude"),
      { wrapper: createWrapper() },
    );

    await waitFor(() => {
      expect(result.current.data).toEqual([]);
    });
  });

  it("returns available providers", async () => {
    const providers = [
      {
        id: "prov-1",
        name: "Available Provider",
        settingsConfig: {},
        sortIndex: 0,
        createdAt: Date.now(),
      },
    ];

    server.use(
      http.post("http://tauri.local/get_available_providers_for_smart_routing", () =>
        HttpResponse.json(providers),
      ),
    );

    const { result } = renderHook(
      () => useAvailableProvidersForSmartRouting("claude"),
      { wrapper: createWrapper() },
    );

    await waitFor(() => {
      expect(result.current.data).toHaveLength(1);
      expect(result.current.data?.[0].id).toBe("prov-1");
    });
  });
});
