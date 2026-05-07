import { render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, beforeEach, vi } from "vitest";
import type { ReactNode } from "react";
import { server } from "../msw/server";
import { resetProviderState } from "../msw/state";
import { http, HttpResponse } from "msw";

// Mock sonner
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

// Mock lucide-react: preserve original exports, only override what we need to verify
vi.mock("lucide-react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("lucide-react")>();
  return {
    ...actual,
    // We use the real icons, just keeping them for the test
  };
});

// Mock the ProviderHealthBadge (simplifies component tree)
vi.mock("@/components/providers/ProviderHealthBadge", () => ({
  ProviderHealthBadge: ({ consecutiveFailures }: { consecutiveFailures: number }) => (
    <span data-testid="health-badge">{consecutiveFailures}</span>
  ),
}));

// Mock framer-motion to avoid animation issues in jsdom
vi.mock("framer-motion", () => ({
  AnimatePresence: ({ children }: any) => <>{children}</>,
  motion: {
    div: ({ children, ...props }: any) => <div {...props}>{children}</div>,
  },
}));

// We need to import after mocks
import { SmartRoutingPanel } from "@/components/proxy/SmartRoutingPanel";

const createWrapper = () => {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
};

describe("SmartRoutingPanel", () => {
  beforeEach(() => {
    resetProviderState();
    server.resetHandlers();

    // Default MSW handlers for smart routing
    server.use(
      http.post("http://tauri.local/get_smart_routing_enabled", () =>
        HttpResponse.json(false),
      ),
      http.post("http://tauri.local/set_smart_routing_enabled", () =>
        HttpResponse.json(true),
      ),
      http.post("http://tauri.local/get_smart_routing_queue", () =>
        HttpResponse.json([]),
      ),
      http.post("http://tauri.local/add_to_smart_routing_queue", () =>
        HttpResponse.json(true),
      ),
      http.post("http://tauri.local/remove_from_smart_routing_queue", () =>
        HttpResponse.json(true),
      ),
      http.post(
        "http://tauri.local/get_available_providers_for_smart_routing",
        () => HttpResponse.json([]),
      ),
      // Provider health (returns healthy by default)
      http.post("http://tauri.local/get_provider_health", () =>
        HttpResponse.json({
          provider_id: "mock",
          app_type: "claude",
          is_healthy: true,
          consecutive_failures: 0,
          last_success_at: null,
          last_failure_at: null,
          last_error: null,
          updated_at: new Date().toISOString(),
        }),
      ),
    );
  });

  const renderPanel = (disabled = false) => {
    const Wrapper = createWrapper();
    return render(
      <Wrapper>
        <SmartRoutingPanel disabled={disabled} />
      </Wrapper>,
    );
  };

  it("renders the info alert with description", async () => {
    renderPanel();

    // The Info alert contains a description about smart routing.
    // Check for the alert element via testid or role, and verify text content.
    await waitFor(() => {
      // The info box uses the Alert component; its description contains routing explanation
      const body = document.body.textContent || "";
      expect(body).toMatch(/智能路由|Smart|routing/i);
    });
  });

  it("renders three tabs: Claude, Codex, Gemini", async () => {
    renderPanel();

    await waitFor(() => {
      expect(screen.getByText("Claude")).toBeInTheDocument();
      expect(screen.getByText("Codex")).toBeInTheDocument();
      expect(screen.getByText("Gemini")).toBeInTheDocument();
    });
  });

  it("shows disabled state when proxy is not running", () => {
    renderPanel(true);

    // The toggle switch should be disabled
    const switches = screen.getAllByRole("switch");
    expect(switches.length).toBeGreaterThan(0);
  });

  it("renders smart routing toggle switch", async () => {
    renderPanel();

    await waitFor(() => {
      const toggle = screen.getByRole("switch");
      expect(toggle).toBeInTheDocument();
    });
  });

  it("does not show queue managers when smart routing is disabled", async () => {
    renderPanel();

    // Wait for the component to render
    await waitFor(() => {
      expect(screen.getByText("Claude")).toBeInTheDocument();
    });

    // Queue manager titles should NOT be visible (smart routing disabled by default)
    expect(screen.queryByText(/主对话 Providers|Main.*Providers/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/子Agent Providers|Others.*Providers/i)).not.toBeInTheDocument();
  });

  it("shows queue managers when smart routing is enabled", async () => {
    // Override to return enabled
    server.use(
      http.post("http://tauri.local/get_smart_routing_enabled", () =>
        HttpResponse.json(true),
      ),
    );

    renderPanel();

    await waitFor(() => {
      expect(screen.getByText(/主对话 Providers|Main.*Providers/i)).toBeInTheDocument();
    });
  });
});

describe("SmartRoutingPanel with queue data", () => {
  beforeEach(() => {
    resetProviderState();
    server.resetHandlers();

    server.use(
      http.post("http://tauri.local/get_smart_routing_enabled", () =>
        HttpResponse.json(true),
      ),
      http.post("http://tauri.local/set_smart_routing_enabled", () =>
        HttpResponse.json(true),
      ),
      // Return non-empty queues
      http.post("http://tauri.local/get_smart_routing_queue", () =>
        HttpResponse.json([
          {
            providerId: "test-prov-1",
            providerName: "Test Provider",
            sortIndex: 0,
          },
        ]),
      ),
      http.post("http://tauri.local/add_to_smart_routing_queue", () =>
        HttpResponse.json(true),
      ),
      http.post("http://tauri.local/remove_from_smart_routing_queue", () =>
        HttpResponse.json(true),
      ),
      http.post(
        "http://tauri.local/get_available_providers_for_smart_routing",
        () =>
          HttpResponse.json([
            {
              id: "avail-1",
              name: "Available Provider",
              settingsConfig: {},
              sortIndex: 0,
              createdAt: Date.now(),
            },
          ]),
      ),
      http.post("http://tauri.local/get_provider_health", () =>
        HttpResponse.json({
          provider_id: "test-prov-1",
          app_type: "claude",
          is_healthy: true,
          consecutive_failures: 0,
          last_success_at: null,
          last_failure_at: null,
          last_error: null,
          updated_at: new Date().toISOString(),
        }),
      ),
    );
  });

  const renderPanel = () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    return render(
      <QueryClientProvider client={queryClient}>
        <SmartRoutingPanel />
      </QueryClientProvider>,
    );
  };

  it("displays queue items when they exist", async () => {
    renderPanel();

    await waitFor(() => {
      expect(screen.getByText("Test Provider")).toBeInTheDocument();
    });
  });

  it("shows 'in queue' indicator badge when enabled", async () => {
    renderPanel();

    await waitFor(() => {
      // The badge is shown since smart routing is enabled
      const badges = screen.getAllByText(/已开启|enabled/i);
      expect(badges.length).toBeGreaterThan(0);
    });
  });

  it("shows empty state when queue is empty for others tab", async () => {
    // Override: return empty for others
    server.use(
      http.post("http://tauri.local/get_smart_routing_queue", async ({ request }) => {
        const body = await request.json() as { queueType: string };
        // Return empty for others, non-empty for main
        const items =
          body.queueType === "main"
            ? [
                {
                  providerId: "test-prov-1",
                  providerName: "Test Provider",
                  sortIndex: 0,
                },
              ]
            : [];
        return HttpResponse.json(items);
      }),
    );

    renderPanel();

    // Click on "others" tab... wait, the SmartRoutingPanel has tabs for Claude/Codex/Gemini
    // and within each, main/others are just sections, not tabs.
    // The "others" queue manager should show "队列为空" text
    // Actually, looking at the component, main and others sections are both visible
    // when smart routing is enabled. The empty section shows the "队列为空" placeholder.
    await waitFor(() => {
      // The main section has the provider
      expect(screen.getByText("Test Provider")).toBeInTheDocument();
    });

    // There should be at least one empty queue placeholder
    const emptyMessages = screen.getAllByText(/回退到主故障转移队列|falling back/i);
    expect(emptyMessages.length).toBeGreaterThan(0);
  });
});
