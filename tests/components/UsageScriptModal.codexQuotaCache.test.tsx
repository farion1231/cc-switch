import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import UsageScriptModal from "@/components/UsageScriptModal";
import { subscriptionApi } from "@/lib/api/subscription";
import { createUsageScript, type Provider } from "@/types";
import {
  codexQuotaActivationStatesKey,
  publishCodexOauthQuota,
} from "@/lib/query/subscription";
import type { SubscriptionQuota } from "@/types/subscription";

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    children,
    footer,
    isOpen,
  }: {
    children: ReactNode;
    footer: ReactNode;
    isOpen: boolean;
  }) =>
    isOpen ? (
      <div>
        {children}
        {footer}
      </div>
    ) : null,
}));
vi.mock("@/components/JsonEditor", () => ({ default: () => null }));
vi.mock("@/hooks/useDarkMode", () => ({ useDarkMode: () => false }));
vi.mock("@/lib/query", () => ({
  useSettingsQuery: () => ({ data: undefined }),
}));
vi.mock("@/lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

function quota(queriedAt: number, utilization: number): SubscriptionQuota {
  return {
    tool: "codex_oauth",
    credentialStatus: "valid",
    credentialMessage: null,
    success: true,
    tiers: [{ name: "five_hour", utilization, resetsAt: null }],
    extraUsage: null,
    error: null,
    queriedAt,
  };
}

const provider: Provider = {
  id: "official",
  name: "Official Codex",
  category: "official",
  settingsConfig: { auth: {}, config: "" },
  meta: {
    providerType: "codex_oauth",
    authBinding: {
      source: "managed_account",
      authProvider: "codex_oauth",
      accountId: "account-a",
    },
    usage_script: createUsageScript({
      enabled: true,
      templateType: "official_subscription",
    }),
  },
};

describe("UsageScriptModal managed Codex quota cache boundary", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("uses the shared timestamp publisher for a late official test result", async () => {
    const user = userEvent.setup();
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    client.setQueryData(codexQuotaActivationStatesKey, [
      { accountId: "account-a", authorizationRevision: 7 },
    ]);
    const oldTest = deferred<SubscriptionQuota>();
    vi.spyOn(subscriptionApi, "getCodexOauthQuota").mockReturnValue(
      oldTest.promise,
    );

    render(
      <QueryClientProvider client={client}>
        <UsageScriptModal
          provider={provider}
          appId="codex"
          isOpen
          onClose={vi.fn()}
          onSave={vi.fn()}
        />
      </QueryClientProvider>,
    );
    await user.click(screen.getByRole("button", { name: /testScript|测试/ }));

    publishCodexOauthQuota(client, "account-a", quota(200, 20));
    await act(async () => {
      oldTest.resolve(quota(100, 90));
      await Promise.resolve();
    });
    await waitFor(() => {
      expect(
        client.getQueryData<SubscriptionQuota>([
          "codex_oauth",
          "quota",
          "account-a",
        ])?.queriedAt,
      ).toBe(200);
    });
  });

  it("does not publish a test result after the activation revision changes", async () => {
    const user = userEvent.setup();
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    client.setQueryData(codexQuotaActivationStatesKey, [
      { accountId: "account-a", authorizationRevision: 7 },
    ]);
    const oldTest = deferred<SubscriptionQuota>();
    vi.spyOn(subscriptionApi, "getCodexOauthQuota").mockReturnValue(
      oldTest.promise,
    );

    render(
      <QueryClientProvider client={client}>
        <UsageScriptModal
          provider={provider}
          appId="codex"
          isOpen
          onClose={vi.fn()}
          onSave={vi.fn()}
        />
      </QueryClientProvider>,
    );
    await user.click(screen.getByRole("button", { name: /testScript|测试/ }));
    client.setQueryData(codexQuotaActivationStatesKey, [
      { accountId: "account-a", authorizationRevision: 8 },
    ]);
    await act(async () => {
      oldTest.resolve(quota(100, 90));
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(
      client.getQueryData<SubscriptionQuota>([
        "codex_oauth",
        "quota",
        "account-a",
      ]),
    ).toBeUndefined();
  });

  it("does not publish an authoritative backend-superseded result", async () => {
    const user = userEvent.setup();
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    client.setQueryData(codexQuotaActivationStatesKey, [
      { accountId: "account-a", authorizationRevision: 8 },
    ]);
    client.setQueryData(
      ["codex_oauth", "quota", "account-a"],
      quota(200, 20),
    );
    vi.spyOn(subscriptionApi, "getCodexOauthQuota").mockResolvedValue({
      ...quota(100, 90),
      lifecycle: "superseded",
    });

    render(
      <QueryClientProvider client={client}>
        <UsageScriptModal
          provider={provider}
          appId="codex"
          isOpen
          onClose={vi.fn()}
          onSave={vi.fn()}
        />
      </QueryClientProvider>,
    );
    await user.click(screen.getByRole("button", { name: /testScript|测试/ }));
    await waitFor(() => {
      expect(
        client.getQueryData<SubscriptionQuota>([
          "codex_oauth",
          "quota",
          "account-a",
        ])?.queriedAt,
      ).toBe(200);
    });
  });
});
