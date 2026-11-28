import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgyOAuthSection } from "@/components/providers/forms/AgyOAuthSection";
import { createTestQueryClient } from "../utils/testQueryClient";

const mocks = vi.hoisted(() => ({
  useManagedAuth: vi.fn(),
  openExternal: vi.fn(),
  openLoginTerminal: vi.fn(() => Promise.resolve()),
  agyAccountRunningState: vi.fn(),
  authSetDefaultAccount: vi.fn(),
  toastError: vi.fn(),
  toastSuccess: vi.fn(),
  toastWarning: vi.fn(),
}));

vi.mock("@/lib/toast", () => ({
  toast: {
    error: mocks.toastError,
    success: mocks.toastSuccess,
    warning: mocks.toastWarning,
    info: vi.fn(),
  },
}));

vi.mock("@/lib/query/accountPlans", () => ({
  useAccountQuotaPlans: () => ({}),
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
  AgyAccountQuota: ({ accountId }: { accountId: string }) => (
    <span data-testid="agy-quota">{accountId}</span>
  ),
}));
vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    settingsApi: { ...actual.settingsApi, openExternal: mocks.openExternal },
    authApi: {
      ...actual.authApi,
      agyAccountOpenLoginTerminal: mocks.openLoginTerminal,
      agyAccountRunningState: mocks.agyAccountRunningState,
      authSetDefaultAccount: mocks.authSetDefaultAccount,
    },
  };
});

const renderSection = (client = createTestQueryClient()) =>
  render(
    <QueryClientProvider client={client}>
      <AgyOAuthSection />
    </QueryClientProvider>,
  );

const baseAuth = () => ({
  accounts: [] as unknown[],
  defaultAccountId: null as string | null,
  isStatusSuccess: true,
  isStatusError: false,
  pollingState: "idle" as "idle" | "polling" | "success" | "error",
  deviceCode: null as unknown,
  error: null as string | null,
  lastCompletedLogin: null as {
    account: { id: string; login: string };
    created: boolean;
  } | null,
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

const account = (id: string, login: string) => ({
  id,
  provider: "agy_oauth",
  login,
  avatar_url: null,
  authenticated_at: 0,
  is_default: false,
  github_domain: "google.com",
  requires_reauth: false,
});

describe("AgyOAuthSection", () => {
  beforeEach(() => {
    mocks.useManagedAuth.mockReset();
    mocks.openExternal.mockReset();
    mocks.openLoginTerminal.mockReset();
    mocks.openLoginTerminal.mockResolvedValue(undefined);
    mocks.agyAccountRunningState.mockReset();
    mocks.authSetDefaultAccount.mockReset();
    mocks.toastError.mockReset();
    mocks.toastSuccess.mockReset();
    mocks.toastWarning.mockReset();
    mocks.agyAccountRunningState.mockResolvedValue({
      cli: false,
      desktop: false,
    });
    mocks.authSetDefaultAccount.mockResolvedValue(undefined);
  });

  it("marks the account Antigravity is signed in with and switches from the menu", async () => {
    const auth = baseAuth();
    auth.accounts = [
      account("a", "a@example.com"),
      account("b", "b@example.com"),
    ];
    auth.defaultAccountId = "a";
    mocks.useManagedAuth.mockReturnValue(auth);
    renderSection();

    expect(mocks.useManagedAuth).toHaveBeenCalledWith("agy_oauth");
    const current = screen.getByText("a@example.com").parentElement!;
    expect(within(current).getByText("Antigravity 在用")).toBeInTheDocument();

    // 当前那个号的菜单里没有「切换」，另一个号有
    await userEvent.click(
      screen.getByRole("button", { name: "b@example.com 的更多操作" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", {
        name: "切换 Antigravity 到这个账号",
      }),
    );
    await waitFor(() => {
      expect(mocks.authSetDefaultAccount).toHaveBeenCalledWith(
        "agy_oauth",
        "b",
      );
    });
    expect(mocks.toastSuccess).toHaveBeenCalledTimes(1);
  });

  it("blocks switching and disables login actions when Antigravity Desktop is running", async () => {
    mocks.agyAccountRunningState.mockResolvedValue({
      cli: false,
      desktop: true,
    });
    const auth = baseAuth();
    auth.accounts = [
      account("a", "a@example.com"),
      account("b", "b@example.com"),
    ];
    auth.defaultAccountId = "a";
    mocks.useManagedAuth.mockReturnValue(auth);
    renderSection();

    expect(
      await screen.findByText("请先完全退出 Antigravity 桌面端再切换账号"),
    ).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "添加账号" }));
    expect(auth.addAccount).not.toHaveBeenCalled();

    await userEvent.click(
      screen.getByRole("button", { name: "b@example.com 的更多操作" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", {
        name: "切换 Antigravity 到这个账号",
      }),
    );

    await waitFor(() => {
      expect(mocks.toastError).toHaveBeenCalledTimes(1);
    });
    expect(mocks.authSetDefaultAccount).not.toHaveBeenCalled();
  });

  it("asks to sign in within agy in the terminal instead of a browser page", async () => {
    const auth = baseAuth();
    Object.assign(auth, {
      isPolling: true,
      isAddingAccount: true,
      pollingState: "polling",
      deviceCode: {
        provider: "agy_oauth",
        device_code: "flow",
        user_code: "",
        verification_uri: "",
        expires_in: 600,
        interval: 2,
      },
    });
    mocks.useManagedAuth.mockReturnValue(auth);
    renderSection();

    expect(screen.getByText(/已在终端打开 agy/)).toBeInTheDocument();
    expect(screen.queryByText(/已在浏览器打开/)).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "复制登录链接" }),
    ).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "重新打开终端" }));
    expect(mocks.openLoginTerminal).toHaveBeenCalled();
    expect(mocks.openExternal).not.toHaveBeenCalled();
  });

  it("shows quota for every account that can still sign in", () => {
    const auth = baseAuth();
    auth.accounts = [
      account("a", "a@example.com"),
      { ...account("b", "b@example.com"), requires_reauth: true },
      account("c", "c@example.com"),
    ];
    auth.defaultAccountId = "a";
    mocks.useManagedAuth.mockReturnValue(auth);
    renderSection();

    const quotas = screen
      .getAllByTestId("agy-quota")
      .map((el) => el.textContent);
    // 不只是 agy 在用的号，其他号也查；登录失效的不查
    expect(quotas).toEqual(["a", "c"]);
  });

  it("forwards target accountId on reauth and uses retryAuth on error retry", async () => {
    const auth = baseAuth();
    auth.accounts = [
      account("a", "a@example.com"),
      account("b", "b@example.com"),
    ];
    auth.defaultAccountId = "a";
    mocks.useManagedAuth.mockReturnValue(auth);
    const client = createTestQueryClient();
    const { rerender } = renderSection(client);

    await userEvent.click(
      screen.getByRole("button", { name: "b@example.com 的更多操作" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "重新登录" }),
    );
    expect(auth.reauthAccount).toHaveBeenCalledWith("b");

    // Failed state -> Retry button calls retryAuth (preserving target), not addAccount.
    auth.pollingState = "error";
    auth.error = "login expired";
    mocks.useManagedAuth.mockReturnValue({ ...auth });
    rerender(
      <QueryClientProvider client={client}>
        <AgyOAuthSection />
      </QueryClientProvider>,
    );

    await userEvent.click(screen.getByRole("button", { name: "重试" }));
    expect(auth.retryAuth).toHaveBeenCalledTimes(1);
    expect(auth.addAccount).not.toHaveBeenCalled();
  });

  it.each([
    {
      name: "same account reauth succeeds",
      completed: {
        account: { id: "b", login: "b@example.com" },
        created: false,
      },
      expectedSuccess: "b@example.com 已重新登录。",
      expectedWarning: null,
    },
    {
      name: "different existing account warns updated",
      completed: {
        account: { id: "a", login: "a@example.com" },
        created: false,
      },
      expectedSuccess: null,
      expectedWarning:
        "登录的不是 b@example.com，a@example.com 已在列表里，副本已更新。",
    },
    {
      name: "different new account warns added",
      completed: {
        account: { id: "c", login: "c@example.com" },
        created: true,
      },
      expectedSuccess: null,
      expectedWarning:
        "登录的不是 b@example.com，已把 c@example.com 作为新账号收录。",
    },
  ])(
    "handles reauth outcome: $name",
    async ({ completed, expectedSuccess, expectedWarning }) => {
      const auth = baseAuth();
      auth.accounts = [
        account("a", "a@example.com"),
        account("b", "b@example.com"),
      ];
      auth.defaultAccountId = "a";
      mocks.useManagedAuth.mockReturnValue(auth);
      const client = createTestQueryClient();
      const { rerender } = renderSection(client);

      await userEvent.click(
        screen.getByRole("button", { name: "b@example.com 的更多操作" }),
      );
      await userEvent.click(
        await screen.findByRole("menuitem", { name: "重新登录" }),
      );

      mocks.useManagedAuth.mockReturnValue({
        ...auth,
        pollingState: "success",
        lastCompletedLogin: completed,
      });
      rerender(
        <QueryClientProvider client={client}>
          <AgyOAuthSection />
        </QueryClientProvider>,
      );

      if (expectedSuccess) {
        expect(mocks.toastSuccess).toHaveBeenCalledWith(expectedSuccess);
        expect(mocks.toastWarning).not.toHaveBeenCalled();
      }
      if (expectedWarning) {
        expect(mocks.toastWarning).toHaveBeenCalledWith(expectedWarning);
        expect(mocks.toastSuccess).not.toHaveBeenCalled();
      }
    },
  );

  it("clears reauthTarget when cancelling pending login or closing an error notice", async () => {
    const auth = baseAuth();
    auth.accounts = [
      account("a", "a@example.com"),
      account("b", "b@example.com"),
    ];
    auth.defaultAccountId = "a";
    mocks.useManagedAuth.mockReturnValue(auth);
    const client = createTestQueryClient();
    const { rerender } = renderSection(client);
    const rerenderWith = (next: ReturnType<typeof baseAuth>) => {
      mocks.useManagedAuth.mockReturnValue(next);
      rerender(
        <QueryClientProvider client={client}>
          <AgyOAuthSection />
        </QueryClientProvider>,
      );
    };

    await userEvent.click(
      screen.getByRole("button", { name: "b@example.com 的更多操作" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "重新登录" }),
    );
    rerenderWith({
      ...auth,
      pollingState: "error",
      error: "boom",
    });
    await userEvent.click(screen.getByRole("button", { name: "关闭" }));
    expect(auth.cancelAuth).toHaveBeenCalledTimes(1);

    rerenderWith({
      ...auth,
      pollingState: "success",
      lastCompletedLogin: {
        account: { id: "d", login: "d@example.com" },
        created: true,
      },
    });
    expect(mocks.toastSuccess).toHaveBeenCalledWith(
      "已添加 Antigravity 账号。",
    );
  });
});
