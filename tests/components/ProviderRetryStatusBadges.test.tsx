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

  it("keeps provider cards quiet until a non-retryable runtime hit occurs", () => {
    render(
      <ProviderRetryStatusBadges
        policy={{
          mode: "finite",
          maxRetries: 0,
          baseDelaySeconds: 3,
          maxDelaySeconds: 30,
          backoffMultiplier: 2,
        }}
      />,
    );

    expect(screen.queryByText("Non-retryable filter")).toBeNull();
  });

  it("shows the runtime keyword warning when a non-retryable keyword matches", () => {
    render(
      <ProviderRetryStatusBadges
        policy={{
          mode: "finite",
          maxRetries: 1,
          baseDelaySeconds: 3,
          maxDelaySeconds: 30,
          backoffMultiplier: 2,
          nonRetryableKeywords: [],
        }}
        retryState={{
          app_type: "claude",
          provider_id: "provider-1",
          provider_name: "Provider 1",
          mode: "finite",
          current_retry: 0,
          max_retry: 1,
          current_delay_seconds: 3,
          active: true,
          waiting: true,
          sticky_infinite: false,
          non_retryable_filter_hit: true,
          non_retryable_keyword: "invalid_api_key",
        }}
      />,
    );

    expect(screen.getByText("Blocked by invalid_api_key")).toBeInTheDocument();
  });
});
