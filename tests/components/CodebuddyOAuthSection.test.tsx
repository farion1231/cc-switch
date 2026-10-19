import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodebuddyOAuthSection } from "@/components/providers/forms/CodebuddyOAuthSection";

const mocks = vi.hoisted(() => ({
  useCodebuddyOauth: vi.fn(),
}));

vi.mock("@/components/providers/forms/hooks/useCodebuddyOauth", () => ({
  useCodebuddyOauth: mocks.useCodebuddyOauth,
}));
vi.mock("@/components/providers/forms/hooks/useManagedAccountUsers", () => ({
  useManagedAccountUsers: () => () => [],
}));
// 额度各自走 React Query；这里只看账号区和登录等待块本身
vi.mock("@/components/settings/auth/AccountQuota", async (importOriginal) => {
  const actual =
    await importOriginal<
      typeof import("@/components/settings/auth/AccountQuota")
    >();
  return {
    ...actual,
    CodebuddyAccountQuota: ({ accountId }: { accountId: string }) => (
      <span data-testid="codebuddy-quota">{accountId}</span>
    ),
  };
});

const baseAuth = () => ({
  accounts: [],
  defaultAccountId: null,
  isStatusSuccess: true,
  isStatusError: false,
  pollingState: "idle" as const,
  deviceCode: null,
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

const account = (extra: Record<string, unknown> = {}) => ({
  id: "cb-account",
  provider: "codebuddy_oauth",
  login: "腾讯小哥",
  avatar_url: null,
  authenticated_at: 1_730_000_000,
  is_default: true,
  github_domain: "copilot.tencent.com",
  reauth_required: false,
  requires_reauth: false,
  ...extra,
});

describe("CodebuddyOAuthSection", () => {
  beforeEach(() => {
    mocks.useCodebuddyOauth.mockReturnValue(baseAuth());
  });

  it("shows the signed-in date, enterprise name, email and quota", () => {
    mocks.useCodebuddyOauth.mockReturnValue({
      ...baseAuth(),
      accounts: [
        account({
          email: "someone@example.com",
          enterprise_name: "腾讯云",
          account_type: "enterprise",
        }),
      ],
      defaultAccountId: "cb-account",
    });
    render(<CodebuddyOAuthSection />);

    expect(
      screen.getByText(/腾讯云 · someone@example\.com/),
    ).toBeInTheDocument();
    expect(screen.getByTestId("codebuddy-quota")).toHaveTextContent(
      "cb-account",
    );
  });

  it("hides the quota and explains re-login for an expired account", () => {
    mocks.useCodebuddyOauth.mockReturnValue({
      ...baseAuth(),
      accounts: [account({ requires_reauth: true })],
      defaultAccountId: "cb-account",
    });
    render(<CodebuddyOAuthSection />);

    expect(
      screen.getByText("登录凭据已失效，请重新登录 CodeBuddy 账号"),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("codebuddy-quota")).not.toBeInTheDocument();
    expect(screen.getByText("需要重新登录")).toBeInTheDocument();
  });

  it("omits the code block and copy button when the provider has no user code", () => {
    mocks.useCodebuddyOauth.mockReturnValue({
      ...baseAuth(),
      isAddingAccount: true,
      isPolling: true,
      pollingState: "polling",
      deviceCode: {
        provider: "codebuddy_oauth",
        device_code: "auth-state",
        user_code: "",
        verification_uri: "https://copilot.tencent.com/auth/state",
        expires_in: 600,
        interval: 5,
      },
    });
    render(<CodebuddyOAuthSection />);

    // CodeBuddy 只有浏览器授权链接：不显示验证码和复制按钮
    expect(
      screen.queryByText("在浏览器中输入验证码："),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "复制验证码" }),
    ).not.toBeInTheDocument();
    // 但保留重新打开授权页面的入口和说明
    expect(
      screen.getByRole("button", { name: "打开授权页面" }),
    ).toBeInTheDocument();
    expect(screen.getByText(/等待授权中/)).toBeInTheDocument();
    expect(screen.getByText(/已在浏览器打开授权页面/)).toBeInTheDocument();
  });

  it("renders the empty state with a CodeBuddy login button", () => {
    render(<CodebuddyOAuthSection />);

    expect(screen.getByText("还没有登录 CodeBuddy 账号。")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "使用 CodeBuddy 登录" }),
    ).toBeInTheDocument();
  });
});
