/** @fileoverview UI contracts for Kimi managed-account authentication controls. */

import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { KimiOAuthSection } from "@/components/providers/forms/KimiOAuthSection";

const mockUseKimiOauth = vi.hoisted(() => vi.fn());
const mockRenderAccountQuota = vi.hoisted(() => vi.fn());

vi.mock("@/components/providers/forms/hooks/useKimiOauth", () => ({
  useKimiOauth: mockUseKimiOauth,
}));

vi.mock("@/components/KimiOauthAccountQuota", () => ({
  default: ({ accountId }: { accountId: string }) => {
    mockRenderAccountQuota(accountId);
    return <div data-testid="kimi-account-quota">{accountId}</div>;
  },
}));

describe("KimiOAuthSection", () => {
  beforeEach(() => {
    mockRenderAccountQuota.mockClear();
    mockUseKimiOauth.mockReturnValue({
      accounts: [
        {
          id: "expired-account",
          login: "expired@example.com",
          avatar_url: null,
          authenticated_at: 1,
          github_domain: "kimi.com",
          requires_reauth: true,
        },
        {
          id: "usable-account",
          login: "usable@example.com",
          avatar_url: null,
          authenticated_at: 2,
          github_domain: "kimi.com",
          requires_reauth: false,
        },
      ],
      defaultAccountId: "usable-account",
      hasAnyAccount: true,
      isAuthenticated: true,
      pollingState: "idle",
      deviceCode: null,
      error: null,
      isPolling: false,
      isAddingAccount: false,
      isRemovingAccount: false,
      isSettingDefaultAccount: false,
      addAccount: vi.fn(),
      removeAccount: vi.fn(),
      setDefaultAccount: vi.fn(),
      cancelAuth: vi.fn(),
      logout: vi.fn(),
    });
  });

  it("keeps a selected account visible when it requires reauthentication", () => {
    render(
      <KimiOAuthSection
        selectedAccountId="expired-account"
        onAccountSelect={vi.fn()}
      />,
    );

    expect(screen.getByRole("combobox")).toHaveTextContent(
      "expired@example.com",
    );
    expect(screen.getByRole("combobox")).toHaveTextContent(
      "Credentials expired",
    );
  });

  it("shows account quota only when requested", () => {
    const { rerender } = render(<KimiOAuthSection />);

    expect(mockRenderAccountQuota).not.toHaveBeenCalled();

    rerender(<KimiOAuthSection showAccountQuota />);

    expect(mockRenderAccountQuota).toHaveBeenCalledTimes(2);
    expect(mockRenderAccountQuota).toHaveBeenNthCalledWith(
      1,
      "expired-account",
    );
    expect(mockRenderAccountQuota).toHaveBeenNthCalledWith(2, "usable-account");
  });

  it("disables retry while a login attempt is already starting", () => {
    mockUseKimiOauth.mockReturnValue({
      ...mockUseKimiOauth(),
      pollingState: "error",
      error: "Device Code does not exist",
      isAddingAccount: true,
    });

    render(<KimiOAuthSection />);

    expect(screen.getByRole("button", { name: "Retry" })).toBeDisabled();
  });
});
