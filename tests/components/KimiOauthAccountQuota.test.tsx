/** @fileoverview Tests for Kimi account quota presentation. */

import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import KimiOauthAccountQuota from "@/components/KimiOauthAccountQuota";

const mocks = vi.hoisted(() => ({
  useQuota: vi.fn(),
  renderQuota: vi.fn(),
}));

vi.mock("@/lib/query/subscription", () => ({
  useKimiOauthQuotaByAccountId: mocks.useQuota,
}));

vi.mock("@/components/SubscriptionQuotaFooter", () => ({
  SubscriptionQuotaView: (props: Record<string, unknown>) => {
    mocks.renderQuota(props);
    return <div data-testid="quota" />;
  },
}));

describe("KimiOauthAccountQuota", () => {
  beforeEach(() => {
    mocks.renderQuota.mockClear();
    mocks.useQuota.mockReturnValue({
      data: { success: true, tiers: [] },
      isFetching: false,
      refetch: vi.fn(),
    });
  });

  it("queries the requested account and renders an expanded quota", () => {
    render(<KimiOauthAccountQuota accountId="kimi-account" />);

    expect(mocks.useQuota).toHaveBeenCalledWith("kimi-account", {
      enabled: true,
      autoQuery: false,
    });
    expect(mocks.renderQuota).toHaveBeenCalledWith(
      expect.objectContaining({
        appIdForExpiredHint: "kimi_oauth",
        inline: false,
        loading: false,
      }),
    );
  });

  it("shows a spinner before the first result", () => {
    mocks.useQuota.mockReturnValue({
      data: undefined,
      isFetching: true,
      refetch: vi.fn(),
    });

    const { container } = render(
      <KimiOauthAccountQuota accountId="kimi-account" />,
    );

    expect(container.querySelector(".animate-spin")).not.toBeNull();
    expect(screen.queryByTestId("quota")).not.toBeInTheDocument();
  });
});
