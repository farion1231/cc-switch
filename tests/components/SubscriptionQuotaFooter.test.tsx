import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SubscriptionQuotaView } from "@/components/SubscriptionQuotaFooter";
import type { SubscriptionQuota } from "@/types/subscription";

function createQuota(
  overrides: Partial<SubscriptionQuota> = {},
): SubscriptionQuota {
  return {
    tool: "claude",
    credentialStatus: "valid",
    credentialMessage: null,
    success: true,
    tiers: [],
    extraUsage: null,
    error: null,
    queriedAt: Date.now(),
    ...overrides,
  };
}

describe("SubscriptionQuotaView", () => {
  it("hides unknown tiers in inline mode while keeping known tiers visible", () => {
    const quota = createQuota({
      tiers: [
        {
          name: "five_hour",
          utilization: 25,
          resetsAt: null,
        },
        {
          name: "seven_day_omelette",
          utilization: 40,
          resetsAt: null,
        },
      ],
    });

    render(
      <SubscriptionQuotaView
        quota={quota}
        loading={false}
        refetch={vi.fn()}
        appIdForExpiredHint="claude"
        inline
      />,
    );

    expect(screen.getByText("subscription.fiveHour:")).toBeInTheDocument();
    expect(screen.queryByText("seven_day_omelette:")).not.toBeInTheDocument();
  });
});
