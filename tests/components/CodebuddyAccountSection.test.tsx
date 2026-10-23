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
import {
  CodebuddyAccountSection,
  codebuddyRegion,
} from "@/components/providers/forms/CodebuddyAccountSection";
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
  cancelAuth: vi.fn(),
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

    expect(mocks.useManagedAuth).toHaveBeenCalledWith("codebuddy_oauth");
    const current = screen.getByText("alice").parentElement!;
    expect(within(current).getByText("CodeBuddy 在用")).toBeInTheDocument();
    expect(screen.getAllByText(/国内版/).length).toBeGreaterThan(0);
    expect(screen.getByText(/UID 300/)).toBeInTheDocument();
    expect(screen.getByText(/登录已过期，需要重新登录/)).toBeInTheDocument();

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

    expect(screen.getByText(/输入 \/login 完成登录/)).toBeInTheDocument();
    expect(screen.queryByText(/已在浏览器打开/)).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "重新打开终端" }));
    expect(mocks.openTerminal).toHaveBeenCalledOnce();
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
