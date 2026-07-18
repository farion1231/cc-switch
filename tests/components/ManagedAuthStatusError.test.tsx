import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodexOAuthSection } from "@/components/providers/forms/CodexOAuthSection";
import { CopilotAuthSection } from "@/components/providers/forms/CopilotAuthSection";

const authMocks = vi.hoisted(() => ({
  refetchCodex: vi.fn(),
  refetchCopilot: vi.fn(),
}));

const failedStatus = (refetchStatus: () => void) => ({
  accounts: [],
  defaultAccountId: null,
  migrationError: null,
  isStatusSuccess: false,
  isStatusError: true,
  hasAnyAccount: false,
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
  refetchStatus,
});

vi.mock("@/components/providers/forms/hooks/useCodexOauth", () => ({
  useCodexOauth: () => failedStatus(authMocks.refetchCodex),
}));

vi.mock("@/components/providers/forms/hooks/useCopilotAuth", () => ({
  useCopilotAuth: () => failedStatus(authMocks.refetchCopilot),
}));

describe("managed auth status failures", () => {
  beforeEach(() => {
    authMocks.refetchCodex.mockResolvedValue(undefined);
    authMocks.refetchCopilot.mockResolvedValue(undefined);
  });

  it("shows a retryable error instead of an empty Codex account selector", () => {
    const onAccountSelect = vi.fn();
    render(
      <CodexOAuthSection
        mode="select"
        selectedAccountId="acct-existing"
        onAccountSelect={onAccountSelect}
      />,
    );

    expect(screen.getByRole("alert")).toHaveTextContent(
      "无法加载 ChatGPT 账号状态，请重试。",
    );
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    expect(onAccountSelect).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    expect(authMocks.refetchCodex).toHaveBeenCalledTimes(1);
  });

  it("shows a retryable error instead of an empty Copilot account selector", () => {
    const onAccountSelect = vi.fn();
    render(
      <CopilotAuthSection
        mode="select"
        selectedAccountId="acct-existing"
        onAccountSelect={onAccountSelect}
      />,
    );

    expect(screen.getByRole("alert")).toHaveTextContent(
      "无法加载 GitHub Copilot 账号状态，请重试。",
    );
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    expect(onAccountSelect).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    expect(authMocks.refetchCopilot).toHaveBeenCalledTimes(1);
  });
});
