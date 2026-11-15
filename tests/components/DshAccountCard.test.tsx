import { render, screen } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { DshAccountCard } from "@/components/providers/DshAccount";
import type { DshAccountStatus } from "@/lib/api/dsh";
import { createTestQueryClient } from "../utils/testQueryClient";

const signedIn: DshAccountStatus = {
  signedIn: true,
  expired: false,
  issuer: "https://platform.deepseek.com",
  profile: { id: "u1", name: "Jason", contact: null, avatarUrl: null },
  wallets: [{ currency: "CNY", balance: "102.11" }],
  bonusWallets: [],
  usageUrl: null,
  topUpUrl: null,
  error: null,
};

function renderCard(status: DshAccountStatus) {
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <DshAccountCard status={status} />
    </QueryClientProvider>,
  );
}

describe("DshAccountCard", () => {
  it("shows the account as an official card with its balance and no add or remove action", () => {
    renderCard(signedIn);
    expect(screen.getByRole("heading", { name: "Jason" })).toBeInTheDocument();
    expect(screen.getByText("providerCard.chip.official")).toBeInTheDocument();
    expect(screen.getByText("dsh.account.balance")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", {
        name: /providerCard.action.(add|remove)/,
      }),
    ).not.toBeInTheDocument();
  });

  it("flags an expired sign-in and hides the stale balance", () => {
    renderCard({ ...signedIn, expired: true });
    expect(screen.getByText("dsh.account.expiredChip")).toBeInTheDocument();
    expect(screen.queryByText("dsh.account.balance")).not.toBeInTheDocument();
  });
});
