import type { ReactElement } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import {
  render as rtlRender,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodebuddyAccountSection } from "@/components/providers/forms/CodebuddyAccountSection";
import { codebuddyRegion } from "@/components/providers/forms/CodebuddyLogin";
import { createTestQueryClient } from "../utils/testQueryClient";

const mocks = vi.hoisted(() => ({
  useManagedAuth: vi.fn(),
  live: vi.fn(),
  importCurrent: vi.fn(),
  openTerminal: vi.fn(),
  quota: vi.fn(),
}));

vi.mock("@/components/providers/forms/hooks/useManagedAuth", () => ({
  useManagedAuth: mocks.useManagedAuth,
}));
vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    authApi: {
      ...actual.authApi,
      codebuddyAccountLive: mocks.live,
      codebuddyAccountImportCurrent: mocks.importCurrent,
      codebuddyAccountOpenLoginTerminal: mocks.openTerminal,
      codebuddyAccountQuota: mocks.quota,
    },
  };
});

const render = (ui: ReactElement) =>
  rtlRender(
    <QueryClientProvider client={createTestQueryClient()}>
      {ui}
    </QueryClientProvider>,
  );

const baseAuth = () => ({
  accounts: [] as unknown[],
  defaultAccountId: null as string | null,
  isStatusSuccess: true,
  isStatusError: false,
  pollingState: "idle" as const,
  deviceCode: null as unknown,
  error: null,
  isPolling: false,
  isAddingAccount: false,
  isRemovingAccount: false,
  isSettingDefaultAccount: false,
  addAccount: vi.fn(),
  removeAccount: vi.fn(),
  setDefaultAccount: vi.fn(),
  reauthAccount: vi.fn(),
  cancelAuth: vi.fn(),
  cancelAuthAndWait: vi.fn(() => Promise.resolve()),
  logout: vi.fn(),
  refetchStatus: vi.fn(),
});

const account = (uid: string, domain: string, login: string) => ({
  id: `${uid}@${domain}`,
  provider: "codebuddy_oauth",
  login,
  avatar_url: null,
  authenticated_at: 0,
  is_default: false,
  github_domain: domain,
  requires_reauth: false,
});

const liveState = (state: string, extra = {}) => ({
  state,
  account_id: null,
  label: null,
  domain: null,
  ...extra,
});

const quotaResult = (extra = {}) => ({
  credentialStatus: "valid",
  success: true,
  planName: "体验版",
  isPaid: false,
  enterprise: false,
  unlimited: false,
  total: 200,
  remaining: 150,
  used: 50,
  error: null,
  queriedAt: Date.now(),
  ...extra,
});

describe("CodebuddyAccountSection", () => {
  beforeEach(() => {
    mocks.useManagedAuth.mockReset();
    mocks.live.mockReset();
    mocks.importCurrent.mockReset();
    mocks.openTerminal.mockReset();
    mocks.quota.mockReset();
    mocks.live.mockResolvedValue(liveState("collected"));
    mocks.quota.mockResolvedValue(quotaResult());
  });

  it("lists accounts with their site and leaves switching to the provider list", async () => {
    const auth = baseAuth();
    auth.accounts = [
      account("100", "www.codebuddy.cn", "alice"),
      { ...account("200", "www.codebuddy.ai", "bob"), requires_reauth: true },
      account("300", "www.codebuddy.ai", "carol"),
    ];
    auth.defaultAccountId = "100@www.codebuddy.cn";
    mocks.useManagedAuth.mockReturnValue(auth);
    render(<CodebuddyAccountSection />);

    expect(mocks.useManagedAuth).toHaveBeenCalledWith(
      "codebuddy_oauth",
      undefined,
      undefined,
      expect.objectContaining({ onLoginComplete: expect.any(Function) }),
    );
    const current = screen.getByText("alice").parentElement!;
    expect(within(current).getByText("CodeBuddy 在用")).toBeInTheDocument();
    expect(screen.getAllByText(/国内版/).length).toBeGreaterThan(0);
    expect(screen.getByText(/UID 300/)).toBeInTheDocument();
    expect(screen.getByText(/登录已过期，需要重新登录/)).toBeInTheDocument();

    // 分组没有「删除全部账号」：只能逐个删除
    expect(
      screen.queryByRole("button", { name: "CodeBuddy Code 的更多操作" }),
    ).not.toBeInTheDocument();

    // 切换改在供应商列表的官方登录卡上做，账号菜单里只剩重新登录、删除
    await userEvent.click(
      screen.getByRole("button", { name: "carol 的更多操作" }),
    );
    expect(
      await screen.findByRole("menuitem", { name: /删除/ }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("menuitem", { name: /切换/ }),
    ).not.toBeInTheDocument();
    expect(auth.setDefaultAccount).not.toHaveBeenCalled();
  });

  it("offers to collect an uncollected sign-in", async () => {
    mocks.useManagedAuth.mockReturnValue(baseAuth());
    mocks.live.mockResolvedValue(
      liveState("uncollected", {
        account_id: "100@www.codebuddy.ai",
        label: "alice",
        domain: "www.codebuddy.ai",
      }),
    );
    mocks.importCurrent.mockResolvedValue(
      account("100", "www.codebuddy.ai", "alice"),
    );
    render(<CodebuddyAccountSection />);

    expect(
      await screen.findByText(/CodeBuddy 现在登着 alice · 国际版/),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "收录当前账号" }));
    await waitFor(() => expect(mocks.importCurrent).toHaveBeenCalledOnce());
  });

  it("warns when the sign-in file can't be read", async () => {
    mocks.useManagedAuth.mockReturnValue(baseAuth());
    mocks.live.mockResolvedValue(liveState("unrecognized"));
    render(<CodebuddyAccountSection />);
    expect(
      await screen.findByText("CodeBuddy 的登录文件格式认不出"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "收录当前账号" }),
    ).not.toBeInTheDocument();
  });

  it("shows the terminal sign-in hint and can reopen the terminal", async () => {
    const auth = baseAuth();
    Object.assign(auth, {
      isPolling: true,
      isAddingAccount: true,
      pollingState: "polling",
      deviceCode: {
        provider: "codebuddy_oauth",
        device_code: "flow",
        user_code: "",
        verification_uri: "",
        expires_in: 900,
        interval: 2,
      },
    });
    mocks.useManagedAuth.mockReturnValue(auth);
    mocks.openTerminal.mockResolvedValue(undefined);
    render(<CodebuddyAccountSection />);

    expect(
      screen.getByText(/没有出现登录界面就输入 \/login/),
    ).toBeInTheDocument();
    // 这次登录没有让 CodeBuddy 退出原来的登录：按钮就是普通的取消
    expect(screen.getByRole("button", { name: "取消" })).toBeInTheDocument();
    expect(screen.queryByText(/已在浏览器打开/)).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "重新打开终端" }));
    expect(mocks.openTerminal).toHaveBeenCalledOnce();
  });

  it("signs in directly when CodeBuddy is not signed in", async () => {
    const auth = baseAuth();
    mocks.useManagedAuth.mockReturnValue(auth);
    mocks.live.mockResolvedValue(liveState("none"));
    render(<CodebuddyAccountSection />);
    await waitFor(() => expect(mocks.live).toHaveBeenCalled());
    // 空状态不再有登录按钮，标题行的「添加账号」始终在
    await userEvent.click(
      await screen.findByRole("button", { name: /添加账号/ }),
    );
    expect(auth.addAccount).toHaveBeenCalledOnce();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("confirms before signing CodeBuddy out to add an account, and can restore it", async () => {
    const auth = baseAuth();
    auth.accounts = [account("100", "www.codebuddy.cn", "alice")];
    auth.defaultAccountId = "100@www.codebuddy.cn";
    // 开始登录后进入等待状态
    auth.addAccount.mockImplementation(() => {
      Object.assign(auth, {
        isPolling: true,
        isAddingAccount: true,
        pollingState: "polling",
        deviceCode: {
          provider: "codebuddy_oauth",
          device_code: "flow",
          user_code: "",
          verification_uri: "",
          expires_in: 900,
          interval: 2,
        },
      });
    });
    auth.cancelAuthAndWait.mockImplementation(() => {
      Object.assign(auth, {
        isPolling: false,
        isAddingAccount: false,
        pollingState: "idle",
        deviceCode: null,
      });
      return Promise.resolve();
    });
    mocks.useManagedAuth.mockImplementation(() => auth);
    mocks.live.mockResolvedValue(
      liveState("collected", {
        account_id: "100@www.codebuddy.cn",
        label: "alice",
        domain: "www.codebuddy.cn",
      }),
    );
    render(<CodebuddyAccountSection />);
    await screen.findByText("alice");
    // 等凭据状态读回来（已登录），授权中心标题行的「添加账号」才会先确认
    await waitFor(() => expect(mocks.live).toHaveBeenCalled());
    await new Promise((resolve) => setTimeout(resolve, 0));
    await userEvent.click(screen.getByRole("button", { name: /添加账号/ }));
    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByText(/CodeBuddy 现在登着 alice · 国内版/),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByText(/正在运行的 CodeBuddy 会话会暂时变为未登录/),
    ).toBeInTheDocument();
    // 还没确认就不动 CodeBuddy
    expect(auth.addAccount).not.toHaveBeenCalled();
    await userEvent.click(
      within(dialog).getByRole("button", { name: "继续登录" }),
    );
    expect(auth.addAccount).toHaveBeenCalledOnce();

    expect(
      await screen.findByText(/已让 CodeBuddy 暂时退出原来的登录/),
    ).toBeInTheDocument();
    const restore = screen.getByRole("button", { name: "恢复原来的登录" });
    const liveCalls = mocks.live.mock.calls.length;
    await userEvent.click(restore);
    expect(auth.cancelAuthAndWait).toHaveBeenCalledOnce();
    // 恢复后重新读一次凭据状态
    await waitFor(() =>
      expect(mocks.live.mock.calls.length).toBeGreaterThan(liveCalls),
    );
  });

  it("re-signing in an account also confirms first", async () => {
    const auth = baseAuth();
    auth.accounts = [account("100", "www.codebuddy.cn", "alice")];
    mocks.useManagedAuth.mockReturnValue(auth);
    mocks.live.mockResolvedValue(liveState("uncollected", { label: "bob" }));
    render(<CodebuddyAccountSection />);
    await waitFor(() => expect(mocks.live).toHaveBeenCalled());
    await screen.findByText(/CodeBuddy 现在登着 bob/);
    await userEvent.click(
      screen.getByRole("button", { name: "alice 的更多操作" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: /重新登录/ }),
    );
    const dialog = await screen.findByRole("dialog");
    await userEvent.click(
      within(dialog).getByRole("button", { name: "继续登录" }),
    );
    expect(auth.reauthAccount).toHaveBeenCalledWith("100@www.codebuddy.cn");
  });

  it("shows each account's plan and credits, and refreshes past the cache", async () => {
    const auth = baseAuth();
    auth.accounts = [
      account("100", "www.codebuddy.cn", "alice"),
      account("200", "www.codebuddy.cn", "bob"),
    ];
    mocks.useManagedAuth.mockReturnValue(auth);
    mocks.quota.mockImplementation(async (accountId: string) =>
      accountId.startsWith("200")
        ? quotaResult({
            success: false,
            credentialStatus: "refresh_pending",
            planName: null,
          })
        : quotaResult(),
    );
    render(<CodebuddyAccountSection />);

    expect(await screen.findByText("体验版")).toBeInTheDocument();
    expect(
      screen.getByRole("meter", { name: "体验版: 150" }),
    ).toBeInTheDocument();
    expect(screen.getByText(/令牌待 CodeBuddy 续期/)).toBeInTheDocument();
    expect(mocks.quota).toHaveBeenCalledWith(
      "100@www.codebuddy.cn",
      "zh",
      false,
    );

    await userEvent.click(
      screen.getByRole("button", { name: "刷新 alice 的额度" }),
    );
    await waitFor(() =>
      expect(mocks.quota).toHaveBeenCalledWith(
        "100@www.codebuddy.cn",
        "zh",
        true,
      ),
    );
  });

  it("classifies CodeBuddy domains", () => {
    expect(codebuddyRegion("www.codebuddy.ai")).toBe("global");
    expect(codebuddyRegion("https://www.codebuddy.cn")).toBe("mainland");
    expect(codebuddyRegion("copilot.tencent.com")).toBe("mainland");
    expect(codebuddyRegion("example.com")).toBeNull();
    expect(codebuddyRegion("")).toBeNull();
  });
});
