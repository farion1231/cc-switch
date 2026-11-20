import { QueryClientProvider } from "@tanstack/react-query";
import { render as rtlRender, screen } from "@testing-library/react";
import type { ReactElement } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import CopilotCliQuotaFooter from "@/components/CopilotCliQuotaFooter";
import { createTestQueryClient } from "../utils/testQueryClient";

const render = (ui: ReactElement) =>
  rtlRender(
    <QueryClientProvider client={createTestQueryClient()}>
      {ui}
    </QueryClientProvider>,
  );

const quotaCalls = vi.hoisted(() => [] as Array<string | null>);
const signedIn = vi.hoisted(() => ({
  value: undefined as
    | { login: string; githubDomain: string; accountId: string | null }
    | null
    | undefined,
}));

vi.mock("@/lib/query/copilot", () => ({
  useCopilotQuota: (accountId: string | null) => {
    quotaCalls.push(accountId);
    return {
      data: {
        success: true,
        plan: "individual",
        resetDate: null,
        tiers: [{ name: "premium", utilization: 25, resetsAt: null }],
        error: null,
        queriedAt: 1,
      },
      isFetching: false,
      refetch: vi.fn(),
    };
  },
  useCopilotCliSignedInAccount: (enabled: boolean) => ({
    data: enabled ? signedIn.value : undefined,
    isPending: enabled && signedIn.value === undefined,
    isFetching: false,
    refetch: vi.fn(),
  }),
}));

describe("CopilotCliQuotaFooter", () => {
  beforeEach(() => {
    quotaCalls.length = 0;
    signedIn.value = undefined;
  });

  it("queries the bound Auth Center account", () => {
    render(
      <CopilotCliQuotaFooter
        meta={{
          authBinding: {
            source: "managed_account",
            authProvider: "github_copilot",
            accountId: "bound-1",
          },
        }}
      />,
    );
    expect(quotaCalls).toContain("bound-1");
  });

  it("follows the account Copilot CLI is signed in with", () => {
    signedIn.value = {
      login: "octo",
      githubDomain: "github.com",
      accountId: "123",
    };
    render(<CopilotCliQuotaFooter meta={{}} />);
    expect(quotaCalls).toContain("123");
  });

  it("explains when the signed-in account is not in the Auth Center", () => {
    signedIn.value = {
      login: "octo",
      githubDomain: "github.com",
      accountId: null,
    };
    render(<CopilotCliQuotaFooter meta={{}} />);
    expect(quotaCalls).toHaveLength(0);
    expect(document.body.textContent).toContain("quota.failed");
  });

  it("explains when Copilot CLI is not signed in", () => {
    signedIn.value = null;
    render(<CopilotCliQuotaFooter meta={{}} />);
    expect(quotaCalls).toHaveLength(0);
    expect(screen.getByText(/quota\.failed/)).toBeTruthy();
  });
});
