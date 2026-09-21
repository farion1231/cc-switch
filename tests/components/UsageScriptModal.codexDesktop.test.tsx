import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import UsageScriptModal from "@/components/UsageScriptModal";
import { subscriptionApi } from "@/lib/api/subscription";
import { createUsageScript } from "@/types";
import type { SubscriptionQuota } from "@/types/subscription";

describe("Codex official usage settings", () => {
  afterEach(() => vi.restoreAllMocks());

  it.each(["codex", "codex-desktop"] as const)(
    "queries and caches official quota for %s independently",
    async (appId) => {
      const quota: SubscriptionQuota = {
        tool: appId,
        credentialStatus: "valid",
        credentialMessage: null,
        success: true,
        tiers: [{ name: "Weekly", utilization: 25, resetsAt: null }],
        extraUsage: null,
        error: null,
        queriedAt: 0,
      };
      const getQuota = vi
        .spyOn(subscriptionApi, "getQuota")
        .mockResolvedValue(quota);
      const client = new QueryClient();
      render(
        <QueryClientProvider client={client}>
          <UsageScriptModal
            isOpen
            appId={appId}
            provider={{
              id: `${appId}-official`,
              name: "OpenAI Official",
              category: "official",
              settingsConfig: { auth: {}, config: "" },
              meta: {
                usage_script: createUsageScript({
                  enabled: true,
                  templateType: "official_subscription",
                }),
              },
            }}
            onClose={vi.fn()}
            onSave={vi.fn()}
          />
        </QueryClientProvider>,
      );
      await waitFor(() =>
        expect(
          screen.getByText("usageScript.officialSubscriptionHint"),
        ).toBeVisible(),
      );
      expect(screen.queryByText("usageScript.templateCustom")).toBeNull();
      fireEvent.click(
        screen.getByRole("button", { name: "usageScript.testScript" }),
      );
      await waitFor(() => {
        expect(getQuota).toHaveBeenCalledWith(appId);
        expect(client.getQueryData(["subscription", "quota", appId])).toEqual(
          quota,
        );
      });
      const otherApp = appId === "codex" ? "codex-desktop" : "codex";
      expect(
        client.getQueryData(["subscription", "quota", otherApp]),
      ).toBeUndefined();
      client.clear();
    },
  );
});
