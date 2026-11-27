import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgyOAuthSection } from "@/components/providers/forms/AgyOAuthSection";

const mocks = vi.hoisted(() => ({
  useManagedAuth: vi.fn(),
  openExternal: vi.fn(),
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
  };
});

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
  });

  it("marks the account agy is signed in with and switches from the menu", async () => {
    const auth = baseAuth();
    auth.accounts = [
      account("a", "a@example.com"),
      account("b", "b@example.com"),
    ];
    auth.defaultAccountId = "a";
    mocks.useManagedAuth.mockReturnValue(auth);
    render(<AgyOAuthSection />);

    expect(mocks.useManagedAuth).toHaveBeenCalledWith("agy_oauth");
    const current = screen.getByText("a@example.com").parentElement!;
    expect(within(current).getByText("agy 在用")).toBeInTheDocument();

    // 当前那个号的菜单里没有「切换」，另一个号有
    await userEvent.click(
      screen.getByRole("button", { name: "b@example.com 的更多操作" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "切换 agy 到这个账号" }),
    );
    expect(auth.setDefaultAccount).toHaveBeenCalledWith("b");
  });

  it("shows the browser sign-in hint instead of a device code", async () => {
    const auth = baseAuth();
    Object.assign(auth, {
      isPolling: true,
      isAddingAccount: true,
      pollingState: "polling",
      deviceCode: {
        provider: "agy_oauth",
        device_code: "flow",
        user_code: "",
        verification_uri: "https://accounts.google.com/o/oauth2/v2/auth?x=1",
        expires_in: 600,
        interval: 1,
      },
    });
    mocks.useManagedAuth.mockReturnValue(auth);
    render(<AgyOAuthSection />);

    expect(
      screen.getByText(/已在浏览器打开 Antigravity 的登录页/),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("在浏览器中输入验证码："),
    ).not.toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "重新打开登录页" }),
    );
    expect(mocks.openExternal).toHaveBeenCalledWith(
      "https://accounts.google.com/o/oauth2/v2/auth?x=1",
    );
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
    render(<AgyOAuthSection />);

    const quotas = screen
      .getAllByTestId("agy-quota")
      .map((el) => el.textContent);
    // 不只是 agy 在用的号，其他号也查；登录失效的不查
    expect(quotas).toEqual(["a", "c"]);
  });

  it("versions Antigravity avatar URLs by authenticated_at and requires_reauth", () => {
    const auth = baseAuth();
    auth.accounts = [
      {
        ...account("ok", "ok@example.com"),
        avatar_url: "https://lh3.googleusercontent.com/a/public-photo=s96-c",
        authenticated_at: 1700000001,
      },
      {
        ...account("expired", "expired@example.com"),
        avatar_url: "https://lh3.googleusercontent.com/a/expired-photo=s96-c",
        authenticated_at: 1700000002,
        requires_reauth: true,
      },
      {
        ...account("query", "query@example.com"),
        avatar_url:
          "https://lh3.googleusercontent.com/a-/private-photo=s96-c?sz=96&v=stale",
        authenticated_at: 1700000003,
      },
      {
        ...account("non-finite", "nonfinite@example.com"),
        avatar_url: "https://lh3.googleusercontent.com/a/no-ts=s96-c",
        authenticated_at: Number.NaN,
      },
      {
        ...account("blank", "blank@example.com"),
        avatar_url: "   ",
        authenticated_at: 1700000004,
      },
    ];
    mocks.useManagedAuth.mockReturnValue(auth);
    const { container, rerender } = render(<AgyOAuthSection />);

    const srcs = Array.from(
      container.querySelectorAll(".auth-account-row img"),
    ).map((img) => img.getAttribute("src"));
    expect(srcs).toEqual([
      "https://lh3.googleusercontent.com/a/public-photo=s96-c?v=1700000001",
      "https://lh3.googleusercontent.com/a/expired-photo=s96-c?v=1700000002-reauth",
      "https://lh3.googleusercontent.com/a-/private-photo=s96-c?sz=96&v=1700000003",
      "https://lh3.googleusercontent.com/a/no-ts=s96-c",
    ]);
    for (const src of srcs) {
      expect(src).not.toContain("-ns");
    }

    // 过期账号重新登录后 authenticated_at 更新且去掉 -reauth，立即切换 URL 绕开旧缓存
    mocks.useManagedAuth.mockReturnValue({
      ...auth,
      accounts: [
        {
          ...account("expired", "expired@example.com"),
          avatar_url: "https://lh3.googleusercontent.com/a/expired-photo=s96-c",
          authenticated_at: 1700009999,
          requires_reauth: false,
        },
      ],
    });
    rerender(<AgyOAuthSection />);
    expect(
      container.querySelector(".auth-account-row img")?.getAttribute("src"),
    ).toBe(
      "https://lh3.googleusercontent.com/a/expired-photo=s96-c?v=1700009999",
    );
  });
});
