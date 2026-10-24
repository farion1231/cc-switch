import type { ReactElement } from "react";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { WorkBuddyAccountSection } from "@/components/settings/auth/WorkBuddyAccountSection";

const mocks = vi.hoisted(() => ({
  useManagedAuth: vi.fn(),
  liveState: vi.fn(),
  importCurrent: vi.fn(),
  prepareNewLogin: vi.fn(),
  openApp: vi.fn(),
  setDefault: vi.fn(),
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
      workbuddyAccountLiveState: mocks.liveState,
      workbuddyAccountImportCurrent: mocks.importCurrent,
      workbuddyAccountPrepareNewLogin: mocks.prepareNewLogin,
      workbuddyAccountOpenApp: mocks.openApp,
      authSetDefaultAccount: mocks.setDefault,
      workbuddyAccountQuota: mocks.quota,
    },
  };
});

const renderWithClient = (ui: ReactElement) =>
  render(
    <QueryClientProvider
      client={
        new QueryClient({
          defaultOptions: {
            queries: { retry: false },
            mutations: { retry: false },
          },
        })
      }
    >
      {ui}
    </QueryClientProvider>,
  );

const baseAuth = () => ({
  accounts: [] as unknown[],
  defaultAccountId: null as string | null,
  isStatusSuccess: true,
  isStatusError: false,
  isRemovingAccount: false,
  removeAccount: vi.fn(),
  logout: vi.fn(),
  refetchStatus: vi.fn(),
});

const account = (id: string, extra: Record<string, unknown> = {}) => ({
  id,
  provider: "workbuddy_account",
  login: id,
  avatar_url: null,
  authenticated_at: 0,
  is_default: false,
  github_domain: "workbuddy",
  requires_reauth: false,
  ...extra,
});

const live = (patch: Record<string, boolean> = {}) => ({
  running: false,
  signedIn: true,
  recognized: true,
  imported: true,
  ...patch,
});

describe("WorkBuddyAccountSection", () => {
  beforeEach(() => {
    for (const mock of Object.values(mocks)) mock.mockReset();
    mocks.liveState.mockResolvedValue(live());
    mocks.openApp.mockResolvedValue(undefined);
    mocks.quota.mockResolvedValue({
      snapshot: null,
      current: false,
      unavailable: "not_current",
      error: null,
    });
  });

  it("shows each account's plan and credits, and says when it is the last result", async () => {
    const auth = baseAuth();
    auth.accounts = [account("u-1"), account("u-2")];
    auth.defaultAccountId = "u-1";
    mocks.useManagedAuth.mockReturnValue(auth);
    mocks.quota.mockImplementation(async (id: string) =>
      id === "u-1"
        ? {
            snapshot: {
              planName: "体验版",
              isPaid: false,
              total: 1500,
              remaining: 750,
              used: 750,
              queriedAt: Date.now() - 60_000,
            },
            current: true,
            unavailable: "not_running",
            error: null,
          }
        : {
            snapshot: null,
            current: false,
            unavailable: "not_current",
            error: null,
          },
    );
    renderWithClient(<WorkBuddyAccountSection />);

    const meter = await screen.findByRole("meter", { name: "体验版: 750" });
    expect(meter.closest("[title]")?.getAttribute("title")).toContain(
      "workbuddy.quota.staleNotRunning",
    );
    // 没查到过的非当前账号不显示额度
    expect(screen.getAllByRole("meter")).toHaveLength(1);
    expect(mocks.quota).toHaveBeenCalledWith("u-1", false);
    expect(mocks.quota).toHaveBeenCalledWith("u-2", false);

    // ↻ 跳过缓存
    await userEvent.click(
      screen.getByRole("button", { name: "刷新 u-1 的额度" }),
    );
    await waitFor(() => expect(mocks.quota).toHaveBeenCalledWith("u-1", true));
  });

  it("marks the account WorkBuddy uses and leaves switching to the provider list", async () => {
    const auth = baseAuth();
    auth.accounts = [
      account("u-1", { account_kind: "personal" }),
      account("u-2"),
    ];
    auth.defaultAccountId = "u-1";
    mocks.useManagedAuth.mockReturnValue(auth);
    renderWithClient(<WorkBuddyAccountSection />);

    expect(mocks.useManagedAuth).toHaveBeenCalledWith("workbuddy_account");
    const current = screen.getByText("u-1").parentElement!;
    expect(within(current).getByText("WorkBuddy 在用")).toBeInTheDocument();
    expect(screen.getByText(/personal/)).toBeInTheDocument();
    expect(
      screen.getByText(
        "快照只在这台电脑上有效（登录凭据绑定本机），不会随云同步，换电脑后需要重新收录。",
      ),
    ).toBeInTheDocument();

    // 授权中心不再切换：⋯ 菜单里只有删除
    await userEvent.click(
      screen.getByRole("button", { name: "u-2 的更多操作" }),
    );
    expect(
      await screen.findByRole("menuitem", { name: /删除/ }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("menuitem", { name: /切换/ }),
    ).not.toBeInTheDocument();
    expect(mocks.setDefault).not.toHaveBeenCalled();
  });

  it("imports the current sign-in from the empty state", async () => {
    mocks.useManagedAuth.mockReturnValue(baseAuth());
    mocks.importCurrent.mockResolvedValue({
      account: account("u-1"),
      created: true,
    });
    renderWithClient(<WorkBuddyAccountSection />);

    await userEvent.click(screen.getByRole("button", { name: /收录当前登录/ }));
    await waitFor(() => expect(mocks.importCurrent).toHaveBeenCalledOnce());
  });

  it("explains how to add another account when the current one is known", async () => {
    const auth = baseAuth();
    auth.accounts = [account("u-1")];
    auth.defaultAccountId = "u-1";
    mocks.useManagedAuth.mockReturnValue(auth);
    mocks.importCurrent.mockResolvedValue({
      account: account("u-1"),
      created: false,
    });
    mocks.prepareNewLogin.mockResolvedValue(account("u-1"));
    renderWithClient(<WorkBuddyAccountSection />);

    await userEvent.click(screen.getByRole("button", { name: /添加账号/ }));
    expect(
      await screen.findByText("当前登录的账号已在列表里（副本已更新）"),
    ).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "登录另一个账号" }),
    );
    await waitFor(() => expect(mocks.prepareNewLogin).toHaveBeenCalledOnce());
    await waitFor(() => expect(mocks.openApp).toHaveBeenCalledOnce());
  });

  it("offers to open WorkBuddy when it isn't signed in", async () => {
    mocks.useManagedAuth.mockReturnValue(baseAuth());
    mocks.liveState.mockResolvedValue(live({ signedIn: false }));
    mocks.importCurrent.mockRejectedValue(new Error("not signed in"));
    renderWithClient(<WorkBuddyAccountSection />);

    await userEvent.click(screen.getByRole("button", { name: /收录当前登录/ }));
    expect(
      await screen.findByText("WorkBuddy 当前没有登录"),
    ).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "打开 WorkBuddy" }),
    );
    expect(mocks.openApp).toHaveBeenCalledOnce();
  });

  it("warns while WorkBuddy is running", async () => {
    const auth = baseAuth();
    auth.accounts = [account("u-1")];
    mocks.useManagedAuth.mockReturnValue(auth);
    mocks.liveState.mockResolvedValue(live({ running: true }));
    renderWithClient(<WorkBuddyAccountSection />);

    expect(await screen.findByText("WorkBuddy 正在运行")).toBeInTheDocument();
  });
});
