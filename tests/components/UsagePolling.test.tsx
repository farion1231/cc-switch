import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import {
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  vi,
  type MockInstance,
} from "vitest";
import UsageFooter from "@/components/UsageFooter";
import { ProviderCard } from "@/components/providers/ProviderCard";
import { usageApi, type AppId } from "@/lib/api";
import type { Provider } from "@/types";

vi.mock("@/components/ProviderIcon", () => ({ ProviderIcon: () => null }));
vi.mock("@/components/quota/useQuotaDisplay", () => ({
  useQuotaDisplay: () => "left",
}));
vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: undefined }),
}));

const advance = (ms: number) => act(() => vi.advanceTimersByTimeAsync(ms));

describe.each(["footer", "collapsed card"] as const)(
  "%s usage polling",
  (view) => {
    let client: QueryClient;
    let query: MockInstance<typeof usageApi.query>;

    beforeEach(() => {
      vi.useFakeTimers();
      client = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      query = vi.spyOn(usageApi, "query").mockResolvedValue({
        success: true,
        data: [
          { planName: "Plan A", remaining: 10, unit: "USD" },
          { planName: "Plan B", remaining: 20, unit: "USD" },
        ],
      });
    });

    afterEach(() => {
      cleanup();
      client.clear();
      vi.restoreAllMocks();
      vi.useRealTimers();
    });

    async function renderUsage({
      appId = "claude",
      isCurrent = false,
      interval = 1,
      enabled = true,
    }: {
      appId?: AppId;
      isCurrent?: boolean;
      interval?: number | null;
      enabled?: boolean;
    } = {}) {
      const provider: Provider = {
        id: "usage-provider",
        name: "Usage provider",
        category: "custom",
        settingsConfig: {
          env: { ANTHROPIC_BASE_URL: "https://example.com" },
        },
        meta: {
          usage_script: {
            enabled,
            language: "javascript",
            code: "",
            autoQueryInterval: interval ?? undefined,
          },
        },
      };

      render(
        <QueryClientProvider client={client}>
          {view === "footer" ? (
            <UsageFooter
              provider={provider}
              providerId={provider.id}
              appId={appId}
              usageEnabled={enabled}
            />
          ) : (
            <ProviderCard
              provider={provider}
              appId={appId}
              isCurrent={isCurrent}
              presentation={{ chips: [], buttons: [] }}
              onEdit={vi.fn()}
              onDelete={vi.fn()}
              onConfigureUsage={vi.fn()}
              onOpenWebsite={vi.fn()}
            />
          )}
        </QueryClientProvider>,
      );
      await advance(1);
      if (view === "collapsed card" && enabled) {
        // With the footer unmounted, the card must keep its own query polling.
        fireEvent.click(screen.getByRole("button", { name: "收起" }));
      }
    }

    it.each([
      { appId: "claude" as const, isCurrent: false },
      { appId: "claude" as const, isCurrent: true },
      { appId: "opencode" as const, isCurrent: false },
    ])("refreshes every minute for %j", async (options) => {
      await renderUsage(options);
      expect(query).toHaveBeenCalledTimes(1);
      expect(query).toHaveBeenCalledWith("usage-provider", options.appId);

      await advance(60_000);
      expect(query).toHaveBeenCalledTimes(2);
      await advance(60_000);
      expect(query).toHaveBeenCalledTimes(3);
    });

    it.each([0, null])(
      "does not poll with a zero or unset interval (%s)",
      async (interval) => {
        await renderUsage({ interval });
        expect(query).toHaveBeenCalledTimes(1);
        await advance(180_000);
        expect(query).toHaveBeenCalledTimes(1);
      },
    );

    it("does not query when usage is disabled", async () => {
      await renderUsage({ enabled: false });
      await advance(180_000);
      expect(query).not.toHaveBeenCalled();
    });
  },
);
