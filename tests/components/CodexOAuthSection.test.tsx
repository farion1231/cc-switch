import { useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CodexOAuthSection } from "@/components/providers/forms/CodexOAuthSection";
import { AuthCenterPanel } from "@/components/settings/AuthCenterPanel";

const mocks = vi.hoisted(() => ({
  useCodexOauth: vi.fn(),
  renderAccountQuota: vi.fn(),
}));

vi.mock("@/components/providers/forms/hooks/useCodexOauth", () => ({
  useCodexOauth: mocks.useCodexOauth,
}));

vi.mock("@/components/CodexOauthAccountQuota", () => ({
  default: ({ accountId }: { accountId: string }) => {
    mocks.renderAccountQuota(accountId);
    return <div data-testid="account-quota">{accountId}</div>;
  },
}));

vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: () => <div />,
}));

vi.mock("@/components/providers/forms/XaiOAuthSection", () => ({
  XaiOAuthSection: () => <div />,
}));

describe("CodexOAuthSection", () => {
  let scrollIntoViewDescriptor: PropertyDescriptor | undefined;

  beforeEach(() => {
    scrollIntoViewDescriptor = Object.getOwnPropertyDescriptor(
      HTMLElement.prototype,
      "scrollIntoView",
    );
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: vi.fn(),
    });
    mocks.useCodexOauth.mockReturnValue({
      accounts: [
        {
          id: "account-1",
          provider: "codex_oauth",
          login: "user@example.com",
          avatar_url: null,
          authenticated_at: 0,
          is_default: true,
          github_domain: "",
          reauth_required: false,
          requires_reauth: false,
        },
        {
          id: "account-2",
          provider: "codex_oauth",
          login: "second@example.com",
          avatar_url: null,
          authenticated_at: 1,
          is_default: false,
          github_domain: "",
          reauth_required: false,
          requires_reauth: false,
        },
      ],
      defaultAccountId: "account-1",
      isStatusSuccess: true,
      isStatusError: false,
      hasAnyAccount: true,
      pollingState: "idle",
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
  });

  afterEach(() => {
    if (scrollIntoViewDescriptor) {
      Object.defineProperty(
        HTMLElement.prototype,
        "scrollIntoView",
        scrollIntoViewDescriptor,
      );
    } else {
      Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
    }
  });

  it("does not render account quota by default", () => {
    render(<CodexOAuthSection />);

    expect(mocks.renderAccountQuota).not.toHaveBeenCalled();
    expect(screen.queryByTestId("account-quota")).not.toBeInTheDocument();
  });

  it("renders account quota in Auth Center", () => {
    render(<AuthCenterPanel />);

    expect(mocks.renderAccountQuota).toHaveBeenCalledWith("account-1");
    expect(mocks.renderAccountQuota).toHaveBeenCalledWith("account-2");
    expect(
      screen.getAllByTestId("account-quota").map((quota) => quota.textContent),
    ).toEqual(["account-1", "account-2"]);
  });

  it("selects a specific account when multiple accounts are managed", async () => {
    const user = userEvent.setup();
    const onAccountSelect = vi.fn();
    const ControlledSection = () => {
      const [selectedAccountId, setSelectedAccountId] = useState<string | null>(
        "account-1",
      );
      return (
        <CodexOAuthSection
          mode="select"
          selectedAccountId={selectedAccountId}
          onAccountSelect={(accountId) => {
            onAccountSelect(accountId);
            setSelectedAccountId(accountId);
          }}
        />
      );
    };
    render(<ControlledSection />);

    await user.click(screen.getByRole("combobox"));
    await user.click(
      await screen.findByRole("option", { name: /second@example\.com/ }),
    );

    expect(onAccountSelect).toHaveBeenCalledWith("account-2");
    expect(screen.getByRole("combobox")).toHaveTextContent(
      "second@example.com",
    );
  });

  it("locks the native card to the current Codex login", async () => {
    const user = userEvent.setup();
    render(
      <CodexOAuthSection
        mode="select"
        selectedAccountId={null}
        onAccountSelect={vi.fn()}
        noneOptionLabel="Use Codex current login"
        nativeLoginOnly
      />,
    );

    const selector = screen.getByRole("combobox");
    expect(selector).toBeDisabled();
    expect(selector).toHaveTextContent("Use Codex current login");
    await user.click(selector);
    expect(
      screen.queryByRole("option", { name: /user@example\.com/ }),
    ).not.toBeInTheDocument();
  });

  it("requires a managed account on managed Official cards", async () => {
    const user = userEvent.setup();
    render(
      <CodexOAuthSection
        mode="select"
        selectedAccountId="account-1"
        onAccountSelect={vi.fn()}
        noneOptionLabel="Use Codex current login"
        allowUnboundSelection={false}
      />,
    );

    await user.click(screen.getByRole("combobox"));
    expect(
      screen.queryByRole("option", { name: "Use Codex current login" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("option", { name: /second@example\.com/ }),
    ).toBeInTheDocument();
  });

  it("shows a disabled account prompt before a managed account is selected", () => {
    render(
      <CodexOAuthSection
        mode="select"
        selectedAccountId={null}
        onAccountSelect={vi.fn()}
        allowUnboundSelection={false}
      />,
    );

    expect(screen.getByRole("combobox")).toHaveTextContent(
      "选择一个 ChatGPT 账号",
    );
  });
});
