import { QueryClientProvider } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import UsageFooter from "@/components/UsageFooter";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

const usageQuery = vi.hoisted(() => vi.fn());
vi.mock("@/lib/query/queries", () => ({
  useSettingsQuery: () => ({ data: undefined }),
  useUsageQuery: (...args: unknown[]) => {
    usageQuery(...args);
    return {
      data: undefined,
      isFetching: false,
      isError: false,
      refetch: vi.fn(),
    };
  },
}));
vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: undefined }),
}));
vi.mock("@/components/ProviderIcon", () => ({ ProviderIcon: () => null }));
vi.mock("@/components/providers/ProviderCardActions", () => ({
  ProviderCardActions: () => null,
}));

const provider: Provider = {
  id: "rpi-usage",
  name: "RPI usage",
  category: "custom",
  settingsConfig: {},
  meta: {
    usage_script: {
      enabled: true,
      language: "javascript",
      code: "return {}",
      autoQueryInterval: 10,
    },
  },
};

describe("RPI usage polling", () => {
  it.each([true, false])(
    "uses the configured interval regardless of current-provider state (%s)",
    (isCurrent) => {
      usageQuery.mockClear();
      render(
        <QueryClientProvider client={createTestQueryClient()}>
          <ProviderCard
            provider={provider}
            appId="rpi"
            isCurrent={isCurrent}
            presentation={{ chips: [], buttons: [] }}
            onEdit={vi.fn()}
            onDelete={vi.fn()}
            onConfigureUsage={vi.fn()}
            onOpenWebsite={vi.fn()}
          />
          <UsageFooter
            provider={provider}
            providerId={provider.id}
            appId="rpi"
            usageEnabled={true}
          />
        </QueryClientProvider>,
      );
      expect(usageQuery.mock.calls.length).toBeGreaterThanOrEqual(2);
      for (const args of usageQuery.mock.calls) {
        expect(args).toEqual([
          provider.id,
          "rpi",
          { enabled: true, autoQueryInterval: 10 },
        ]);
      }
    },
  );
});
