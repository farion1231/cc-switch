import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodexQuotaRefreshControls } from "@/components/settings/auth/CodexQuotaRefreshControls";
import { subscriptionApi } from "@/lib/api/subscription";
import { authApi, settingsApi } from "@/lib/api";
import type { ManagedAuthStatus } from "@/lib/api";
import type { SubscriptionQuota } from "@/types/subscription";
import type { Settings } from "@/types";

vi.mock("@/lib/api/subscription", () => ({
  subscriptionApi: { refreshCodexOauthQuotas: vi.fn() },
}));
vi.mock("@/lib/api", () => ({
  authApi: { authGetStatus: vi.fn() },
  settingsApi: { get: vi.fn(), save: vi.fn() },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: unknown) =>
      `${key}${values ? JSON.stringify(values) : ""}`,
  }),
}));
vi.mock("@/components/ui/select", () => ({
  Select: ({
    value,
    disabled,
    onValueChange,
    children,
  }: {
    value: string;
    disabled: boolean;
    onValueChange: (value: string) => void;
    children: React.ReactNode;
  }) => (
    <select
      aria-label="interval"
      value={value}
      disabled={disabled}
      onChange={(event) => onValueChange(event.target.value)}
    >
      {children}
    </select>
  ),
  SelectTrigger: () => null,
  SelectValue: () => null,
  SelectContent: ({ children }: { children: React.ReactNode }) => (
    <>{children}</>
  ),
  SelectItem: ({
    value,
    children,
  }: {
    value: string;
    children: React.ReactNode;
  }) => <option value={value}>{children}</option>,
}));

const quota: SubscriptionQuota = {
  tool: "codex",
  credentialStatus: "valid",
  credentialMessage: null,
  success: true,
  tiers: [],
  extraUsage: null,
  error: null,
  queriedAt: 123,
};
const status: ManagedAuthStatus = {
  provider: "codex_oauth",
  authenticated: true,
  default_account_id: "a",
  accounts: [
    {
      id: "a",
      login: "alice",
      provider: "codex_oauth",
      avatar_url: null,
      authenticated_at: 0,
      is_default: true,
      github_domain: "",
      requires_reauth: false,
    },
  ],
};
const settings = { codexAccountQuotaRefreshMinutes: 0 } as Settings;
function mount() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <CodexQuotaRefreshControls />
    </QueryClientProvider>,
  );
  return client;
}
async function refreshButton() {
  const button = screen.getByRole("button", {
    name: "codexOauth.quotaRefresh.refreshAll",
  });
  await waitFor(() => expect(button).not.toBeDisabled());
  return button;
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(settingsApi.get).mockResolvedValue(settings);
  vi.mocked(settingsApi.save).mockResolvedValue(true);
  vi.mocked(authApi.authGetStatus).mockResolvedValue(status);
});

describe("Codex quota batch controls", () => {
  it("does not overwrite newer account caches or restore a removed account from batch results", async () => {
    const bQuota = { ...quota, queriedAt: 456 };
    vi.mocked(subscriptionApi.refreshCodexOauthQuotas).mockResolvedValue([
      { accountId: "b", quota: bQuota, error: null },
      { accountId: "a", quota, error: null },
    ]);
    const client = mount();
    const newerQuota = { ...quota, queriedAt: 999 };
    client.setQueryData(["codex_oauth", "quota", "a"], newerQuota);
    const invalidate = vi.spyOn(client, "invalidateQueries");
    fireEvent.click(await refreshButton());
    await screen.findByText('codexOauth.quotaRefresh.complete{"count":2}');
    expect(client.getQueryData(["codex_oauth", "quota", "a"])).toEqual(
      newerQuota,
    );
    expect(client.getQueryData(["codex_oauth", "quota", "b"])).toBeUndefined();
    expect(invalidate.mock.calls).toEqual([
      [{ queryKey: ["managed-auth-status", "codex_oauth"] }],
    ]);
  });

  it("reports transport and structured quota failures as partial failure", async () => {
    vi.mocked(subscriptionApi.refreshCodexOauthQuotas).mockResolvedValue([
      { accountId: "a", quota, error: null },
      { accountId: "b", quota: null, error: "offline" },
      { accountId: "c", quota: { ...quota, success: false }, error: null },
    ]);
    mount();
    fireEvent.click(await refreshButton());
    await screen.findByText(
      'codexOauth.quotaRefresh.partial{"succeeded":1,"failed":2}',
    );
  });

  it("disables refresh while a batch is pending", async () => {
    let finish!: (value: []) => void;
    vi.mocked(subscriptionApi.refreshCodexOauthQuotas).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    mount();
    fireEvent.click(await refreshButton());
    expect(
      await screen.findByRole("button", {
        name: "codexOauth.quotaRefresh.refreshing",
      }),
    ).toBeDisabled();
    await act(async () => finish([]));
    await screen.findByText("codexOauth.quotaRefresh.noEligible");
  });

  it("disables refresh for accounts requiring sign-in", async () => {
    vi.mocked(authApi.authGetStatus).mockResolvedValue({
      ...status,
      accounts: [{ ...status.accounts[0], reauth_required: true }],
    });
    mount();
    await screen.findByText("codexOauth.quotaRefresh.noEligible");
    expect(
      screen.getByRole("button", {
        name: "codexOauth.quotaRefresh.refreshAll",
      }),
    ).toBeDisabled();
    expect(subscriptionApi.refreshCodexOauthQuotas).not.toHaveBeenCalled();
  });

  it.each(["reject", "false"])(
    "keeps persisted selection on save %s",
    async (mode) => {
      if (mode === "reject")
        vi.mocked(settingsApi.save).mockRejectedValue(new Error("disk full"));
      else vi.mocked(settingsApi.save).mockResolvedValue(false);
      mount();
      const select = screen.getByRole("combobox");
      await waitFor(() => expect(select).not.toBeDisabled());
      fireEvent.change(select, { target: { value: "5" } });
      await screen.findByRole("alert");
      expect(select).toHaveValue("0");
    },
  );

  it("saves with fresh settings and reflects success", async () => {
    mount();
    const select = screen.getByRole("combobox");
    await waitFor(() => expect(select).not.toBeDisabled());
    vi.mocked(settingsApi.get).mockResolvedValue({
      ...settings,
      language: "ja",
    });
    fireEvent.change(select, { target: { value: "15" } });
    await waitFor(() => expect(select).toHaveValue("15"));
    expect(settingsApi.save).toHaveBeenCalledWith({
      ...settings,
      language: "ja",
      codexAccountQuotaRefreshMinutes: 15,
    });
  });
});
