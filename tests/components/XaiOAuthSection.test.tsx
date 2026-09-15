import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { XaiOAuthSection } from "@/components/providers/forms/XaiOAuthSection";

const mockUseXaiOauth = vi.hoisted(() => vi.fn());

vi.mock("@/components/providers/forms/hooks/useXaiOauth", () => ({
  useXaiOauth: mockUseXaiOauth,
}));

describe("XaiOAuthSection", () => {
  beforeEach(() => {
    mockUseXaiOauth.mockReturnValue({
      accounts: [
        {
          id: "expired-account",
          login: "expired@example.com",
          avatar_url: null,
          authenticated_at: 1,
          github_domain: "x.ai",
          requires_reauth: true,
        },
        {
          id: "usable-account",
          login: "usable@example.com",
          avatar_url: null,
          authenticated_at: 2,
          github_domain: "x.ai",
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
      <XaiOAuthSection
        selectedAccountId="expired-account"
        onAccountSelect={vi.fn()}
      />,
    );

    expect(screen.getByRole("combobox")).toHaveTextContent(
      "expired@example.com",
    );
    expect(screen.getByRole("combobox")).toHaveTextContent("凭据已失效");
  });
});
