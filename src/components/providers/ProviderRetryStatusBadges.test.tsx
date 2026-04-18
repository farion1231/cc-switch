import { render, screen } from "@testing-library/react";
import { ProviderRetryStatusBadges } from "@/components/providers/ProviderRetryStatusBadges";

describe("ProviderRetryStatusBadges", () => {
  it("shows a finite config badge when maxRetries is greater than zero", () => {
    render(
      <ProviderRetryStatusBadges
        policy={{
          mode: "finite",
          maxRetries: 2,
          baseDelaySeconds: 3,
          maxDelaySeconds: 30,
          backoffMultiplier: 2,
        }}
      />,
    );

    expect(screen.getByText("Retry 0/2")).toBeInTheDocument();
  });

  it("does not show non-retryable UI until a runtime hit occurs", () => {
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

    expect(screen.queryByText("Blocked by invalid_request")).toBeNull();
    expect(screen.queryByText("Non-retryable filter")).toBeNull();
  });

  it("shows runtime retry progress when a retry state exists", () => {
    render(
      <ProviderRetryStatusBadges
        policy={{
          mode: "finite",
          maxRetries: 2,
          baseDelaySeconds: 3,
          maxDelaySeconds: 30,
          backoffMultiplier: 2,
        }}
        retryState={{
          app_type: "claude",
          provider_id: "provider-1",
          provider_name: "Provider 1",
          mode: "finite",
          current_retry: 1,
          max_retry: 2,
          current_delay_seconds: 3,
          active: true,
          waiting: true,
          sticky_infinite: false,
        }}
      />,
    );

    expect(screen.getByText("Retry 1/2 · 3s")).toBeInTheDocument();
  });

  it("shows the runtime keyword warning when a non-retryable keyword matches", () => {
    render(
      <ProviderRetryStatusBadges
        policy={{
          mode: "finite",
          maxRetries: 2,
          baseDelaySeconds: 3,
          maxDelaySeconds: 30,
          backoffMultiplier: 2,
        }}
        retryState={{
          app_type: "claude",
          provider_id: "provider-1",
          provider_name: "Provider 1",
          mode: "finite",
          current_retry: 0,
          max_retry: 2,
          current_delay_seconds: 3,
          active: true,
          waiting: true,
          sticky_infinite: false,
          non_retryable_filter_hit: true,
          non_retryable_keyword: "invalid_request",
        }}
      />,
    );

    expect(screen.getByText("Blocked by invalid_request")).toBeInTheDocument();
    expect(
      screen.getByText("Skipped immediately and moved to the next provider."),
    ).toBeInTheDocument();
  });
});
