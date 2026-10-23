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
  });

  it("marks the account WorkBuddy uses and switches from the menu", async () => {
    const auth = baseAuth();
    auth.accounts = [
      account("u-1", { account_kind: "personal" }),
      account("u-2"),
    ];
    auth.defaultAccountId = "u-1";
    mocks.useManagedAuth.mockReturnValue(auth);
    mocks.setDefault.mockResolvedValue(undefined);
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

    await userEvent.click(
      screen.getByRole("button", { name: "u-2 的更多操作" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", {
        name: "切换 WorkBuddy 到这个账号",
      }),
    );
    await waitFor(() =>
      expect(mocks.setDefault).toHaveBeenCalledWith("workbuddy_account", "u-2"),
    );
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
