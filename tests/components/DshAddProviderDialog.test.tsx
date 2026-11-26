import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { AddProviderDialog } from "@/components/providers/AddProviderDialog";
import { dshApi, type DshAccountStatus } from "@/lib/api/dsh";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/lib/api/dsh", () => ({
  dshApi: {
    accountStatus: vi.fn(),
    accountSignOut: vi.fn(),
    loginStart: vi.fn(),
    loginPoll: vi.fn(),
    loginCancel: vi.fn(),
  },
}));
vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: () => null,
}));
vi.mock("@/components/providers/forms/CodexOAuthSection", () => ({
  CodexOAuthSection: () => null,
}));
vi.mock("@/components/providers/forms/XaiOAuthSection", () => ({
  XaiOAuthSection: () => null,
}));
vi.mock("@/components/providers/forms/AgyOAuthSection", () => ({
  AgyOAuthSection: () => null,
}));
vi.mock("@/components/providers/KimiAccountLogin", () => ({
  KimiAccountLogin: () => null,
}));
vi.mock("@/components/JsonEditor", () => ({
  default: () => null,
}));

beforeAll(() => {
  Element.prototype.scrollIntoView ??= vi.fn();
  window.matchMedia ??= vi.fn().mockReturnValue({
    matches: false,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  });
});

const signedOut: DshAccountStatus = {
  signedIn: false,
  expired: false,
  issuer: null,
  profile: null,
  wallets: [],
  bonusWallets: [],
  usageUrl: null,
  topUpUrl: null,
  error: null,
};

describe("DeepSeek account entry in the DeepSeek Harness add-provider dialog", () => {
  beforeEach(() => {
    vi.mocked(dshApi.accountStatus).mockResolvedValue(signedOut);
  });

  it("opens Auth Center directly on official sign-in without an intermediate form or add footer", async () => {
    const onSubmit = vi.fn();
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <AddProviderDialog
          open
          appId="dsh"
          onOpenChange={vi.fn()}
          onSubmit={onSubmit}
        />
      </QueryClientProvider>,
    );

    fireEvent.click(
      await screen.findByRole("button", { name: "dsh.account.presetName" }),
    );
    expect(
      await screen.findByRole("region", { name: "DeepSeek Harness" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "common.add" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", {
        name: "provider.form.officialManaged.manageAccounts",
      }),
    ).not.toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();
  });
});
