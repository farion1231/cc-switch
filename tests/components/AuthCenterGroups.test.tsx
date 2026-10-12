import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { TFunction } from "i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CopilotAuthSection } from "@/components/providers/forms/CopilotAuthSection";
import { CodexOAuthSection } from "@/components/providers/forms/CodexOAuthSection";
import { XaiOAuthSection } from "@/components/providers/forms/XaiOAuthSection";

const mocks = vi.hoisted(() => ({
  useCopilotAuth: vi.fn(),
  useCodexOauth: vi.fn(),
  useXaiOauth: vi.fn(),
}));

vi.mock("@/components/providers/forms/hooks/useCopilotAuth", () => ({
  useCopilotAuth: mocks.useCopilotAuth,
}));
vi.mock("@/components/providers/forms/hooks/useCodexOauth", () => ({
  useCodexOauth: mocks.useCodexOauth,
}));
vi.mock("@/components/providers/forms/hooks/useXaiOauth", () => ({
  useXaiOauth: mocks.useXaiOauth,
}));
// 「N 个供应商在用」要读供应商列表（React Query）；这里不关心，给空
vi.mock("@/components/providers/forms/hooks/useManagedAccountUsers", () => ({
  useManagedAccountUsers: () => () => [],
}));
// 额度各自走 React Query；这里只看账号区本身
vi.mock("@/components/settings/auth/AccountQuota", async (importOriginal) => {
  const actual =
    await importOriginal<
      typeof import("@/components/settings/auth/AccountQuota")
    >();
  return {
    ...actual,
    CopilotAccountQuota: ({ accountId }: { accountId: string }) => (
      <span data-testid="copilot-quota">{accountId}</span>
    ),
    XaiAccountQuota: ({ accountId }: { accountId: string }) => (
      <span data-testid="xai-quota">{accountId}</span>
    ),
  };
});

const baseAuth = () => ({
  accounts: [],
  defaultAccountId: null,
  migrationError: null,
  isStatusSuccess: true,
  isStatusError: false,
  hasAnyAccount: false,
  isAuthenticated: false,
  pollingState: "idle" as const,
  deviceCode: null,
  error: null,
  isPolling: false,
  isAddingAccount: false,
  isRemovingAccount: false,
  isSettingDefaultAccount: false,
  addAccount: vi.fn(),
  reauthAccount: vi.fn(),
  retryAuth: vi.fn(),
  removeAccount: vi.fn(),
  setDefaultAccount: vi.fn(),
  cancelAuth: vi.fn(),
  logout: vi.fn(),
  refetchStatus: vi.fn(),
});

const account = (
  id: string,
  login: string,
  extra: Record<string, unknown> = {},
) => ({
  id,
  provider: "github_copilot",
  login,
  avatar_url: null,
  authenticated_at: 0,
  is_default: false,
  github_domain: "github.com",
  reauth_required: false,
  requires_reauth: false,
  ...extra,
});

describe("Auth Center account groups", () => {
  beforeEach(() => {
    mocks.useCopilotAuth.mockReturnValue(baseAuth());
    mocks.useCodexOauth.mockReturnValue(baseAuth());
    mocks.useXaiOauth.mockReturnValue(baseAuth());
  });

  it("asks for the GitHub deployment before signing in to Copilot", async () => {
    const user = userEvent.setup();
    const auth = {
      ...baseAuth(),
      hasAnyAccount: true,
      accounts: [
        account("octocat", "octocat"),
        account("corp", "corp-dev", { github_domain: "ghe.example.com" }),
      ],
      defaultAccountId: "octocat",
    };
    mocks.useCopilotAuth.mockReturnValue(auth);
    render(<CopilotAuthSection />);

    // 第二行写部署位置；企业版带「Enterprise Server」
    expect(
      screen.getByText(/ghe\.example\.com · Enterprise Server/),
    ).toBeInTheDocument();
    expect(screen.getAllByTestId("copilot-quota")).toHaveLength(2);

    await user.click(screen.getByRole("button", { name: "添加账号" }));
    const deploy = screen.getByRole("group", { name: "GitHub 部署类型" });
    await user.click(
      within(deploy).getByRole("button", { name: "GitHub Enterprise Server" }),
    );

    // 企业域名没填：不登录，字段下面写怎么改
    await user.click(screen.getByRole("button", { name: "使用 GitHub 登录" }));
    expect(auth.addAccount).not.toHaveBeenCalled();
    expect(
      screen.getByText("先填企业域名，例如 company.ghe.com"),
    ).toBeInTheDocument();

    await user.type(
      screen.getByPlaceholderText("例如：company.ghe.com"),
      "https://company.ghe.com/",
    );
    expect(mocks.useCopilotAuth).toHaveBeenLastCalledWith("company.ghe.com");
    await user.click(screen.getByRole("button", { name: "使用 GitHub 登录" }));
    expect(auth.addAccount).toHaveBeenCalledTimes(1);
  });

  it("keeps Copilot re-login out of the account menu", async () => {
    const user = userEvent.setup();
    mocks.useCopilotAuth.mockReturnValue({
      ...baseAuth(),
      hasAnyAccount: true,
      accounts: [account("octocat", "octocat")],
      defaultAccountId: "octocat",
    });
    render(<CopilotAuthSection />);

    // 只有一个账号时没有「删除全部」的 ⋯
    expect(
      screen.queryByRole("button", { name: "GitHub Copilot 的更多操作" }),
    ).not.toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "octocat 的更多操作" }),
    );
    expect(
      await screen.findByRole("menuitem", { name: "删除账号…" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("menuitem", { name: "重新登录" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("menuitem", { name: "设为默认" }),
    ).not.toBeInTheDocument();
  });

  it("offers a retry when the account status fails to load", async () => {
    const user = userEvent.setup();
    const auth = { ...baseAuth(), isStatusSuccess: false, isStatusError: true };
    mocks.useCopilotAuth.mockReturnValue(auth);
    render(<CopilotAuthSection />);

    expect(
      screen.getByText("无法加载 GitHub Copilot 账号状态，请重试。"),
    ).toBeInTheDocument();
    // 页面一打开就在的提示不用 role=alert
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "重试" }));
    expect(auth.refetchStatus).toHaveBeenCalledTimes(1);
  });

  it("shows the empty state with a sign-in button", async () => {
    const user = userEvent.setup();
    const auth = baseAuth();
    mocks.useCodexOauth.mockReturnValue(auth);
    render(<CodexOAuthSection showAccountQuota />);

    expect(screen.getByText("还没有登录 ChatGPT 账号。")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "添加账号" }),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /使用 ChatGPT 登录/ }));
    expect(auth.addAccount).toHaveBeenCalledTimes(1);
  });

  it("re-signs an expired xAI account and hides its quota", async () => {
    const user = userEvent.setup();
    const auth = {
      ...baseAuth(),
      hasAnyAccount: true,
      isAuthenticated: true,
      accounts: [
        account("ok", "me@example.com", { provider: "xai_oauth" }),
        account("expired", "old@example.com", {
          provider: "xai_oauth",
          requires_reauth: true,
        }),
      ],
      defaultAccountId: "ok",
    };
    mocks.useXaiOauth.mockReturnValue(auth);
    render(<XaiOAuthSection mode="manage" />);

    expect(
      screen.getAllByTestId("xai-quota").map((q) => q.textContent),
    ).toEqual(["ok"]);
    expect(screen.getByText("需要重新登录")).toBeInTheDocument();
    expect(
      screen.getByText("登录凭据已失效，用到它的供应商无法使用"),
    ).toBeInTheDocument();

    // xAI 不能指定账号重新登录：发起一次普通登录（同一个账号登录会替换旧凭据）
    await user.click(screen.getByRole("button", { name: "重新登录" }));
    expect(auth.addAccount).toHaveBeenCalledTimes(1);
  });

  it("uses the form layout when a provider form passes onAccountSelect", () => {
    mocks.useXaiOauth.mockReturnValue({
      ...baseAuth(),
      hasAnyAccount: true,
      isAuthenticated: true,
      accounts: [account("ok", "me@example.com", { provider: "xai_oauth" })],
      defaultAccountId: "ok",
    });
    render(
      <XaiOAuthSection selectedAccountId={null} onAccountSelect={vi.fn()} />,
    );

    expect(screen.getByRole("combobox")).toBeInTheDocument();
    expect(screen.queryByTestId("xai-quota")).not.toBeInTheDocument();
  });
});

describe("subscriptionQuotaState", () => {
  const t = ((key: string, opts?: { defaultValue?: string }) =>
    opts?.defaultValue ?? key) as unknown as TFunction;

  it("writes remaining quota per tier and flags a failed query", async () => {
    const { subscriptionQuotaState } = await import(
      "@/components/settings/auth/AccountQuota"
    );
    const ok = subscriptionQuotaState(
      t,
      {
        tool: "codex",
        credentialStatus: "valid",
        credentialMessage: null,
        success: true,
        tiers: [
          { name: "five_hour", utilization: 38, resetsAt: null },
          { name: "seven_day", utilization: 95, resetsAt: null },
        ],
        extraUsage: null,
        error: null,
        queriedAt: 1,
      },
      false,
      "zh",
    );
    expect(ok?.kind).toBe("rows");
    if (ok?.kind === "rows") {
      expect(ok.rows.map((row) => [row.line.left, row.line.tone])).toEqual([
        [62, "normal"],
        [5, "warning"],
      ]);
    }

    expect(
      subscriptionQuotaState(
        t,
        {
          tool: "codex",
          credentialStatus: "valid",
          credentialMessage: null,
          success: false,
          tiers: [],
          extraUsage: null,
          error: "HTTP 500",
          queriedAt: 1,
        },
        false,
        "zh",
      ),
    ).toEqual({ kind: "failed", reason: "HTTP 500" });
    expect(subscriptionQuotaState(t, undefined, true, "zh")).toEqual({
      kind: "loading",
    });
  });
});

describe("AccountQuotaColumn", () => {
  it("opens the saved resets to list when each one expires", async () => {
    const user = userEvent.setup();
    const { AccountQuotaColumn } = await import(
      "@/components/settings/auth/AccountQuota"
    );
    render(
      <AccountQuotaColumn
        login="me@example.com"
        loading={false}
        onRefresh={vi.fn()}
        state={{
          kind: "rows",
          rows: [
            {
              label: "重置",
              line: {
                key: "reset_credits",
                text: "重置剩余 3 次",
                value: "剩余 3 次",
                caption: "10月6日到期",
                tone: "warning",
                left: Infinity,
                breakdown: {
                  title: "存下的限额重置",
                  openLabel: "查看 3 次重置各自的到期时间",
                  items: [
                    {
                      key: "a",
                      label: "10月6日",
                      hint: "2d0h后",
                      value: "2 次",
                      tone: "warning",
                    },
                    { key: "b", label: "不会过期", value: "1 次", tone: "normal" },
                  ],
                },
              },
            },
          ],
        }}
      />,
    );

    expect(screen.queryByText("存下的限额重置")).not.toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "查看 3 次重置各自的到期时间" }),
    );
    const dialog = await screen.findByRole("dialog", { name: "存下的限额重置" });
    expect(
      within(dialog)
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(["10月6日2d0h后2 次", "不会过期1 次"]);
  });
});
