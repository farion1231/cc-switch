import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  ClaudeAccountSelect,
  ClaudeBoundAccountLine,
  ClaudeOfficialSection,
} from "@/components/providers/forms/ClaudeOfficialSection";

const mocks = vi.hoisted(() => ({
  useManagedAuth: vi.fn(),
  openExternal: vi.fn(),
  submitPaste: vi.fn(),
  getLoginSwitch: vi.fn(),
  getStatus: vi.fn(),
}));

vi.mock("@/components/providers/forms/hooks/useManagedAuth", () => ({
  useManagedAuth: mocks.useManagedAuth,
}));
// 「N 个供应商在用」要读供应商列表（React Query）；这里不关心，给空
vi.mock("@/components/providers/forms/hooks/useManagedAccountUsers", () => ({
  useManagedAccountUsers: () => () => [],
}));
// 额度列单独查询（React Query）；这里只看每个号有没有挂上
vi.mock("@/components/settings/auth/AccountQuota", () => ({
  ClaudeAccountQuota: ({ accountId }: { accountId: string }) => (
    <span data-testid="claude-quota">{accountId}</span>
  ),
}));
vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    settingsApi: { ...actual.settingsApi, openExternal: mocks.openExternal },
    authApi: {
      ...actual.authApi,
      authGetStatus: mocks.getStatus,
      claudeAccountSubmitPaste: mocks.submitPaste,
      getClaudeLoginSwitch: mocks.getLoginSwitch,
    },
  };
});

const baseAuth = () => ({
  accounts: [] as unknown[],
  isStatusSuccess: true,
  isStatusError: false,
  pollingState: "idle" as const,
  deviceCode: null as unknown,
  error: null,
  isPolling: false,
  isAddingAccount: false,
  isRemovingAccount: false,
  addAccount: vi.fn(),
  removeAccount: vi.fn(),
  cancelAuth: vi.fn(),
  logout: vi.fn(),
  refetchStatus: vi.fn(),
});

const account = (id: string, login: string) => ({
  id,
  provider: "claude_official",
  login,
  avatar_url: null,
  authenticated_at: 0,
  is_default: false,
  github_domain: "claude.ai",
  requires_reauth: false,
});

const loginSwitch = (overrides: Record<string, unknown> = {}) => {
  const status = {
    blocked: null,
    live: null,
    ...overrides,
  };
  // 和后端一样：没被挡住就会切换
  return { active: status.blocked === null, ...status };
};

// 组件读 React Query（切换状态、账号列表），每个用例一个新的 client
const withQuery = (node: React.ReactNode) =>
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      {node}
    </QueryClientProvider>,
  );
const renderSection = () => withQuery(<ClaudeOfficialSection />);
const renderSelect = () =>
  withQuery(<ClaudeAccountSelect value={null} onChange={vi.fn()} />);

describe("ClaudeOfficialSection", () => {
  beforeEach(() => {
    mocks.useManagedAuth.mockReset();
    mocks.openExternal.mockReset();
    mocks.submitPaste.mockReset();
    mocks.getLoginSwitch.mockReset().mockResolvedValue(loginSwitch());
    mocks.getStatus.mockReset();
  });

  it("has no default account and no re-login while the login is valid", async () => {
    const auth = baseAuth();
    auth.accounts = [account("acc:org", "me@example.com · Acme")];
    mocks.useManagedAuth.mockReturnValue(auth);
    renderSection();

    expect(mocks.useManagedAuth).toHaveBeenCalledWith("claude_official");
    expect(screen.queryByText("默认")).not.toBeInTheDocument();

    await userEvent.click(
      screen.getByRole("button", { name: "me@example.com · Acme 的更多操作" }),
    );
    expect(
      await screen.findByRole("menuitem", { name: "删除账号…" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("menuitem", { name: "设为默认" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("menuitem", { name: "重新登录" }),
    ).not.toBeInTheDocument();
  });

  it("offers re-login once the login has expired", async () => {
    const auth = baseAuth();
    auth.accounts = [
      { ...account("acc:org", "me@example.com · Acme"), requires_reauth: true },
    ];
    mocks.useManagedAuth.mockReturnValue(auth);
    renderSection();

    await userEvent.click(screen.getByRole("button", { name: "重新登录" }));
    expect(auth.addAccount).toHaveBeenCalled();
  });

  it("warns a week before a parked account's sign-in expires", async () => {
    const day = 24 * 60 * 60 * 1000;
    const auth = baseAuth();
    auth.accounts = [
      {
        ...account("soon:org", "soon@example.com"),
        login_expires_at: Date.now() + 2 * day,
      },
      {
        ...account("later:org", "later@example.com"),
        login_expires_at: Date.now() + 20 * day,
      },
    ];
    mocks.useManagedAuth.mockReturnValue(auth);
    renderSection();

    expect(
      screen.getAllByText(/过期，之后要重新登录|loginExpiresOn/),
    ).toHaveLength(1);
    expect(
      screen.queryByRole("button", { name: "重新登录" }),
    ).not.toBeInTheDocument();
  });

  it("offers re-login when a parked account's sign-in may have expired", async () => {
    const auth = baseAuth();
    auth.accounts = [
      {
        ...account("old:org", "old@example.com"),
        login_expires_at: Date.now() - 1000,
      },
    ];
    mocks.useManagedAuth.mockReturnValue(auth);
    renderSection();

    expect(
      screen.getByText(/登录可能已过期|loginMaybeExpired/),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "重新登录" }));
    expect(auth.addAccount).toHaveBeenCalled();
  });

  it("shows quota for every account that can still sign in", () => {
    const auth = baseAuth();
    auth.accounts = [
      account("ok:org", "ok@example.com"),
      { ...account("bad:org", "bad@example.com"), requires_reauth: true },
      {
        ...account("old:org", "old@example.com"),
        login_expires_at: Date.now() - 1000,
      },
    ];
    mocks.useManagedAuth.mockReturnValue(auth);
    renderSection();

    expect(
      screen.getAllByTestId("claude-quota").map((el) => el.textContent),
    ).toEqual(["ok:org"]);
  });

  it("shows each account's plan in the auth center", () => {
    const auth = baseAuth();
    auth.accounts = [
      {
        ...account("max:org", "max@example.com"),
        plan: "max",
        rate_limit_tier: "default_claude_max_20x",
      },
      account("unknown:org", "unknown@example.com"),
    ];
    mocks.useManagedAuth.mockReturnValue(auth);
    renderSection();

    expect(screen.getByText(/Max 20x/)).toBeInTheDocument();
    expect(screen.queryByText(/^Pro$|^Max$/)).not.toBeInTheDocument();
  });

  it("lets the user paste the code when the browser can't come back", async () => {
    const auth = baseAuth();
    Object.assign(auth, {
      isPolling: true,
      isAddingAccount: true,
      pollingState: "polling",
      deviceCode: {
        provider: "claude_official",
        device_code: "flow-1",
        user_code: "",
        verification_uri: "https://claude.ai/oauth/authorize?x=1",
        expires_in: 600,
        interval: 1,
      },
    });
    mocks.useManagedAuth.mockReturnValue(auth);
    mocks.submitPaste.mockResolvedValue(undefined);
    renderSection();

    expect(
      screen.getByText(/已在浏览器打开 Claude 的登录页/),
    ).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "授权后没有回到这里？" }),
    );
    await userEvent.type(
      screen.getByRole("textbox", { name: "授权代码或地址" }),
      "abc#state",
    );
    await userEvent.click(screen.getByRole("button", { name: "提交" }));

    expect(mocks.submitPaste).toHaveBeenCalledWith("flow-1", "abc#state");
    expect(
      await screen.findByText("已提交，正在完成登录…"),
    ).toBeInTheDocument();
  });

  it("says switching to the card changes the sign-in", async () => {
    mocks.useManagedAuth.mockReturnValue(baseAuth());
    renderSelect();
    expect(
      await screen.findByText(/Claude Code 的登录会实时换成选中的账号/),
    ).toBeInTheDocument();
  });

  it("lays out the sign-in choices like the Codex card", async () => {
    // jsdom 没有 scrollIntoView，下拉打开时会调
    Element.prototype.scrollIntoView = vi.fn();
    const user = userEvent.setup();
    mocks.useManagedAuth.mockReturnValue(baseAuth());
    mocks.getStatus.mockResolvedValue({
      accounts: [account("work:org", "work@example.com")],
    });
    const onChange = vi.fn();
    const onManageAccounts = vi.fn();
    withQuery(
      <ClaudeAccountSelect
        value={null}
        onChange={onChange}
        onManageAccounts={onManageAccounts}
      />,
    );

    const select = screen.getByRole("combobox", { name: "登录方式" });
    expect(select).toHaveTextContent("跟随 Claude Code 登录");
    await user.click(select);
    expect(
      (await screen.findAllByRole("option")).map((el) => el.textContent),
    ).toEqual([
      "work@example.com",
      "添加或管理 Claude 账号",
      "跟随 Claude Code 登录账号会随 Claude Code 当前登录变化",
    ]);
    await user.click(screen.getByRole("option", { name: /添加或管理/ }));
    expect(onManageAccounts).toHaveBeenCalled();
    expect(onChange).not.toHaveBeenCalled();

    await user.click(select);
    await user.click(
      await screen.findByRole("option", { name: "work@example.com" }),
    );
    expect(onChange).toHaveBeenCalledWith("work:org");
  });

  it("points to Open Terminal when a config dir override blocks switching", async () => {
    mocks.getLoginSwitch.mockResolvedValue(
      loginSwitch({ blocked: "overrideDir" }),
    );
    mocks.useManagedAuth.mockReturnValue(baseAuth());
    renderSelect();
    expect(
      await screen.findByText(/请从卡片「打开终端」使用选中的账号/),
    ).toBeInTheDocument();
  });

  it("greys out sign-in when the Claude config dir is in WSL", async () => {
    mocks.getLoginSwitch.mockResolvedValue(loginSwitch({ blocked: "wsl" }));
    const auth = baseAuth();
    auth.accounts = [
      {
        ...account("old:org", "old@example.com"),
        requires_reauth: true,
      },
    ];
    mocks.useManagedAuth.mockReturnValue(auth);
    renderSection();

    expect(
      await screen.findByText(
        "Claude 配置目录在 WSL 里，暂不支持添加和切换 Claude 账号。",
      ),
    ).toBeInTheDocument();
    const add = screen.getByRole("button", { name: "添加账号" });
    expect(add).toHaveAttribute("aria-disabled", "true");
    await userEvent.click(add);
    const reauth = screen.getByRole("button", { name: "重新登录" });
    expect(reauth).toHaveAttribute("aria-disabled", "true");
    await userEvent.click(reauth);
    expect(auth.addAccount).not.toHaveBeenCalled();
  });

  it("says WSL isn't supported on the card form", async () => {
    mocks.getLoginSwitch.mockResolvedValue(loginSwitch({ blocked: "wsl" }));
    mocks.useManagedAuth.mockReturnValue(baseAuth());
    renderSelect();
    expect(
      await screen.findByText(/Claude 配置目录在 WSL 里，暂不支持 Claude 账号/),
    ).toBeInTheDocument();
  });

  it("words the card subtitle like the Codex card", async () => {
    const day = 24 * 60 * 60 * 1000;
    mocks.getStatus.mockResolvedValue({
      provider: "claude_official",
      authenticated: true,
      default_account_id: null,
      accounts: [
        account("plain:org", "plain@example.com"),
        {
          ...account("old:org", "old@example.com"),
          login_expires_at: Date.now() - day,
        },
        account("named:org", "named@example.com"),
        { ...account("pro:org", "pro@example.com"), plan: "pro" },
      ],
    });
    withQuery(
      <>
        <ClaudeBoundAccountLine accountId="plain:org" providerName="Work" />
        <ClaudeBoundAccountLine accountId="old:org" providerName="Old" />
        <ClaudeBoundAccountLine
          accountId="named:org"
          providerName="Claude (named@example.com)"
        />
        <ClaudeBoundAccountLine
          accountId="plain:org"
          providerName="Noted"
          note="团队号"
        />
        <ClaudeBoundAccountLine accountId="gone:org" providerName="Gone" />
        <ClaudeBoundAccountLine
          accountId="pro:org"
          providerName="Pro card"
          showPlan
        />
        <ClaudeBoundAccountLine
          accountId="pro:org"
          providerName="Usage off"
          note="额度关了"
        />
      </>,
    );
    // 不带「账号：」前缀
    expect(await screen.findByText("plain@example.com")).toBeInTheDocument();
    // 登录过期跟一个标记
    expect(screen.getByText("old@example.com")).toBeInTheDocument();
    expect(screen.getAllByText(/需要重新登录|reauthBadge/)).toHaveLength(1);
    // 卡名里已经有账号就不重复
    expect(screen.queryByText("named@example.com")).not.toBeInTheDocument();
    expect(
      screen.getByText(/Claude 账号|claudeAccount\.claudeAccount/),
    ).toBeInTheDocument();
    // 备注优先
    expect(screen.getByText("团队号")).toBeInTheDocument();
    expect(screen.getByText(/不在这台电脑上的账号/)).toBeInTheDocument();
    // 套餐跟在账号后面；读不到套餐的号不写
    expect(screen.getByText("pro@example.com")).toBeInTheDocument();
    expect(screen.getAllByText(/· /)).toHaveLength(1);
    // 额度查询关着的卡不写套餐
    expect(screen.getAllByText("· Pro")).toHaveLength(1);
    expect(screen.getByText("额度关了")).toBeInTheDocument();
  });
});
