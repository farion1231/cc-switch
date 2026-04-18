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

  it("hides the finite config badge when maxRetries is zero", () => {
    const { container } = render(
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

    expect(container).toBeEmptyDOMElement();
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
});
