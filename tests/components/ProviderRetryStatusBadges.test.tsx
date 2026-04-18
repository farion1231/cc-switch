import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ProviderRetryStatusBadges } from "@/components/providers/ProviderRetryStatusBadges";

describe("ProviderRetryStatusBadges", () => {
  it("shows the static infinite retry badge from provider policy", () => {
    render(
      <ProviderRetryStatusBadges
        policy={{
          mode: "infinite",
        }}
      />,
    );

    expect(screen.getByText("Infinite retry")).toBeInTheDocument();
  });

  it("shows the sticky runtime warning when infinite retry is actively blocking failover", () => {
    render(
      <ProviderRetryStatusBadges
        policy={{
          mode: "infinite",
        }}
        retryState={{
          app_type: "claude",
          provider_id: "provider-1",
          provider_name: "Provider 1",
          mode: "infinite",
          current_retry: 4,
          max_retry: null,
          current_delay_seconds: 12,
          active: true,
          waiting: true,
          sticky_infinite: true,
        }}
      />,
    );

    expect(screen.getByText(/Stuck here/i)).toBeInTheDocument();
  });
});
