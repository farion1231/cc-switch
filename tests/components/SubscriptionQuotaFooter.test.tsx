import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  OfficialSubscriptionDetailsView,
  ResetCreditsCard,
  SubscriptionQuotaView,
} from "@/components/SubscriptionQuotaFooter";
import type { SubscriptionQuota } from "@/types/subscription";

describe("SubscriptionQuotaView", () => {
  it("shows Codex reset credits together with official usage windows", () => {
    const quota: SubscriptionQuota = {
      tool: "codex",
      credentialStatus: "valid",
      credentialMessage: null,
      success: true,
      tiers: [
        {
          name: "five_hour",
          utilization: 25,
          resetsAt: "2030-01-01T00:00:00Z",
        },
      ],
      extraUsage: null,
      resetCredits: {
        availableCount: 2,
        credits: [
          {
            grantedAt: "2029-01-01T00:00:00Z",
            expiresAt: "2030-01-02T00:00:00Z",
          },
          {
            grantedAt: "2029-01-02T00:00:00Z",
            expiresAt: "2030-01-03T00:00:00Z",
          },
        ],
      },
      error: null,
      queriedAt: Date.now(),
    };

    render(
      <SubscriptionQuotaView
        quota={quota}
        loading={false}
        refetch={vi.fn()}
        appIdForExpiredHint="codex"
        inline
      />,
    );

    expect(screen.getByText("subscription.fiveHour:")).toBeInTheDocument();
    expect(screen.getByText("subscription.resetCredits:")).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument();
  });
});

describe("ResetCreditsCard", () => {
  it("shows every reset opportunity with acquired and expiry times", () => {
    const { container } = render(
      <ResetCreditsCard
        resetCredits={{
          availableCount: 1,
          credits: [
            {
              grantedAt: "2026-06-19T02:00:00Z",
              expiresAt: "2026-06-26T02:00:00Z",
            },
          ],
        }}
      />,
    );

    expect(screen.getByText("subscription.resetOpportunities")).toBeTruthy();
    expect(screen.getByText(/subscription.grantedAt/)).toBeTruthy();
    expect(screen.getByText(/subscription.expiresAt/)).toBeTruthy();
    expect(container.querySelector("svg")).toBeNull();
    expect(container.querySelector("[class*='divide-']")).toBeNull();
    expect(container.firstElementChild).toHaveClass(
      "border-border-default",
      "bg-card",
    );
  });
});

describe("OfficialSubscriptionDetailsView", () => {
  it("does not render the details divider before quota data exists", () => {
    const { container } = render(
      <OfficialSubscriptionDetailsView
        quota={undefined}
        loading={true}
        refetch={vi.fn()}
        appId="codex"
        includeResetCredits={true}
      />,
    );

    expect(container).toBeEmptyDOMElement();
  });
});
