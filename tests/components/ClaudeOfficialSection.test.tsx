import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  ClaudeAccountSelect,
  ClaudeOfficialSection,
} from "@/components/providers/forms/ClaudeOfficialSection";

const mocks = vi.hoisted(() => ({
  useManagedAuth: vi.fn(),
  openExternal: vi.fn(),
  submitPaste: vi.fn(),
  getLoginSwitch: vi.fn(),
  setLoginSwitch: vi.fn(),
}));

vi.mock("@/components/providers/forms/hooks/useManagedAuth", () => ({
  useManagedAuth: mocks.useManagedAuth,
}));
// 「N 个供应商在用」要读供应商列表（React Query）；这里不关心，给空
vi.mock("@/components/providers/forms/hooks/useManagedAccountUsers", () => ({
  useManagedAccountUsers: () => () => [],
}));
vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    settingsApi: { ...actual.settingsApi, openExternal: mocks.openExternal },
    authApi: {
      ...actual.authApi,
      claudeAccountSubmitPaste: mocks.submitPaste,
      getClaudeLoginSwitch: mocks.getLoginSwitch,
      setClaudeLoginSwitch: mocks.setLoginSwitch,
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
    enabled: false,
    riskAcknowledged: false,
    blocked: null,
    live: null,
    ...overrides,
  };
  // 和后端一样：开着、没被挡住
  return {
    active: status.enabled === true && status.blocked === null,
    ...status,
  };
};

// 组件读 React Query（开关状态、账号列表），每个用例一个新的 client
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
    mocks.setLoginSwitch.mockReset();
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

  it("points to Open Terminal until switching cards switches the sign-in", async () => {
    mocks.useManagedAuth.mockReturnValue(baseAuth());
    renderSelect();
    expect(
      await screen.findByText(/请从卡片「打开终端」使用选中的账号/),
    ).toBeInTheDocument();
  });

  it("says switching changes the sign-in once the toggle is on", async () => {
    mocks.getLoginSwitch.mockResolvedValue(loginSwitch({ enabled: true }));
    mocks.useManagedAuth.mockReturnValue(baseAuth());
    renderSelect();
    expect(
      await screen.findByText(/Claude Code 的登录会换成选中的账号/),
    ).toBeInTheDocument();
  });

  it("explains the risk before turning the sign-in switch on the first time", async () => {
    mocks.setLoginSwitch.mockResolvedValue(
      loginSwitch({ enabled: true, riskAcknowledged: true }),
    );
    mocks.useManagedAuth.mockReturnValue(baseAuth());
    renderSection();

    const toggle = await screen.findByRole("switch", {
      name: "切卡时切换 Claude Code 的登录",
    });
    await userEvent.click(toggle);
    expect(mocks.setLoginSwitch).not.toHaveBeenCalled();
    await userEvent.click(await screen.findByRole("button", { name: "打开" }));
    expect(mocks.setLoginSwitch).toHaveBeenCalledWith(true);
    await waitFor(() => expect(toggle).toBeChecked());
  });

  it("turns the switch back on without asking again", async () => {
    mocks.getLoginSwitch.mockResolvedValue(
      loginSwitch({ riskAcknowledged: true }),
    );
    mocks.setLoginSwitch.mockResolvedValue(
      loginSwitch({ enabled: true, riskAcknowledged: true }),
    );
    mocks.useManagedAuth.mockReturnValue(baseAuth());
    renderSection();

    await userEvent.click(
      await screen.findByRole("switch", {
        name: "切卡时切换 Claude Code 的登录",
      }),
    );
    expect(mocks.setLoginSwitch).toHaveBeenCalledWith(true);
    expect(screen.queryByRole("button", { name: "打开" })).toBeNull();
  });

  it("treats a toggle blocked by a config dir override as off", async () => {
    mocks.getLoginSwitch.mockResolvedValue(
      loginSwitch({ enabled: true, blocked: "overrideDir" }),
    );
    mocks.useManagedAuth.mockReturnValue(baseAuth());
    renderSelect();
    expect(
      await screen.findByText(/请从卡片「打开终端」使用选中的账号/),
    ).toBeInTheDocument();
  });

  it("shows why an enabled toggle is blocked and keeps it switchable off", async () => {
    mocks.getLoginSwitch.mockResolvedValue(
      loginSwitch({ enabled: true, blocked: "overrideDir" }),
    );
    mocks.useManagedAuth.mockReturnValue(baseAuth());
    renderSection();

    const toggle = await screen.findByRole("switch", {
      name: "切卡时切换 Claude Code 的登录",
    });
    expect(toggle).toBeEnabled();
    expect(
      screen.getByText("设置里覆盖了 Claude 配置目录，不能使用。"),
    ).toBeInTheDocument();
  });

  it("can't be turned on with a Claude config dir override", async () => {
    mocks.getLoginSwitch.mockResolvedValue(
      loginSwitch({ blocked: "overrideDir" }),
    );
    mocks.useManagedAuth.mockReturnValue(baseAuth());
    renderSection();

    expect(
      await screen.findByRole("switch", {
        name: "切卡时切换 Claude Code 的登录",
      }),
    ).toBeDisabled();
    expect(
      screen.getByText("设置里覆盖了 Claude 配置目录，不能使用。"),
    ).toBeInTheDocument();
  });
});
