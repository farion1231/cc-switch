/**
 * Claude Code、Codex 的 CodeBuddy 供应商：账号选择、保存 meta.authBinding、未登录校验。
 *
 * 账号状态来自 useCodebuddyOauth（真实 hook 会走 React Query + Tauri），这里直接
 * 用替身给出登录状态；其余表单 hook 与 golden 测试保持一致。
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  ProviderForm,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import { createTestQueryClient } from "../utils/testQueryClient";

// Radix Select 在打开菜单时会调用 scrollIntoView（jsdom 没有实现）
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

const toastMocks = vi.hoisted(() => ({
  error: vi.fn(),
}));

const codebuddyAuth = vi.hoisted(() => ({
  isAuthenticated: true,
  isStatusSuccess: true,
  accounts: [] as Array<{
    id: string;
    login: string;
    requires_reauth: boolean;
  }>,
}));

vi.mock("sonner", () => ({
  toast: {
    error: toastMocks.error,
    success: vi.fn(),
    info: vi.fn(),
    warning: vi.fn(),
  },
}));

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      data-testid="json-editor"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

vi.mock("@/components/providers/forms/CodexOAuthSection", () => ({
  CodexOAuthSection: () => <div data-testid="codex-oauth-section" />,
}));

vi.mock("@/components/providers/forms/hooks/useCodebuddyOauth", () => ({
  useCodebuddyOauth: () => codebuddyAuth,
}));

vi.mock("@/components/providers/forms/hooks", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/components/providers/forms/hooks")>();
  const signedOut = {
    isAuthenticated: false,
    isStatusSuccess: true,
    isStatusError: false,
    accounts: [],
  };
  return {
    ...actual,
    useCopilotAuth: () => signedOut,
    useCodexOauth: () => ({ ...signedOut, defaultAccountId: null }),
    useXaiOauth: () => signedOut,
  };
});

vi.mock("@/lib/query", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/query")>();
  return {
    ...actual,
    useSettingsQuery: () => ({
      data: { commonConfigConfirmed: true },
    }),
  };
});

const SUBMIT_LABEL = "save-provider";

const ACCOUNTS = [
  { id: "cb-1", login: "first@example.com", requires_reauth: false },
  { id: "cb-2", login: "second@example.com", requires_reauth: false },
];

function renderForm(
  onSubmit: (values: ProviderFormValues) => void,
  appId: "claude" | "codex" = "claude",
) {
  const queryClient = createTestQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <ProviderForm
        appId={appId}
        submitLabel={SUBMIT_LABEL}
        onSubmit={onSubmit}
        onCancel={vi.fn()}
      />
    </QueryClientProvider>,
  );
}

function clickCodeBuddyPreset() {
  const matches = screen
    .getAllByRole("button")
    .filter(
      (button) =>
        button.querySelector("span.truncate")?.textContent === "CodeBuddy",
    );
  expect(matches, "预设按钮「CodeBuddy」应唯一").toHaveLength(1);
  fireEvent.click(matches[0]);
}

describe("ProviderForm CodeBuddy managed account", () => {
  beforeEach(() => {
    toastMocks.error.mockReset();
    codebuddyAuth.isAuthenticated = true;
    codebuddyAuth.isStatusSuccess = true;
    codebuddyAuth.accounts = [...ACCOUNTS];
  });

  it("saves the managed account binding and forces OpenAI Chat format", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn<(values: ProviderFormValues) => void>();
    renderForm(onSubmit);

    clickCodeBuddyPreset();

    // 预设填充了 CodeBuddy 的 base URL 与默认模型
    await waitFor(() => {
      expect(screen.getByText("CodeBuddy OAuth 认证")).toBeInTheDocument();
    });

    // 选中一个具体账号（默认是「使用默认账号」）
    await user.click(screen.getByRole("combobox"));
    await user.click(
      await screen.findByRole("option", { name: /second@example\.com/ }),
    );

    fireEvent.click(screen.getByRole("button", { name: SUBMIT_LABEL }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(toastMocks.error).not.toHaveBeenCalled();

    const values = onSubmit.mock.calls[0][0];
    expect(values.meta?.providerType).toBe("codebuddy_oauth");
    expect(values.meta?.apiFormat).toBe("openai_chat");
    expect(values.meta?.authBinding).toEqual({
      source: "managed_account",
      authProvider: "codebuddy_oauth",
      accountId: "cb-2",
    });
    expect(values.settingsConfig).toContain("https://copilot.tencent.com");
    expect(values.settingsConfig).toContain("glm-5.3");
  });

  it("blocks saving and asks to sign in when no account is logged in", async () => {
    codebuddyAuth.isAuthenticated = false;
    codebuddyAuth.accounts = [];
    const onSubmit = vi.fn<(values: ProviderFormValues) => void>();
    renderForm(onSubmit);

    clickCodeBuddyPreset();
    await waitFor(() => {
      expect(screen.getByText("CodeBuddy OAuth 认证")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: SUBMIT_LABEL }));

    await waitFor(() => {
      expect(toastMocks.error).toHaveBeenCalledWith("请先登录 CodeBuddy 账号");
    });
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("saves a Codex CodeBuddy card with the account binding and Chat format", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn<(values: ProviderFormValues) => void>();
    renderForm(onSubmit, "codex");

    clickCodeBuddyPreset();
    await waitFor(() => {
      expect(screen.getByText("CodeBuddy OAuth 认证")).toBeInTheDocument();
    });
    // 地址和 API Key 由本地代理固定，表单里不显示
    expect(screen.queryByLabelText("API Key")).not.toBeInTheDocument();

    // Codex 表单里还有其它下拉框，按当前显示的「使用默认账号」找账号选择框
    const accountSelect = screen
      .getAllByRole("combobox")
      .find((element) => element.textContent?.includes("默认账号"));
    expect(accountSelect).toBeDefined();
    await user.click(accountSelect!);
    await user.click(
      await screen.findByRole("option", { name: /first@example\.com/ }),
    );

    fireEvent.click(screen.getByRole("button", { name: SUBMIT_LABEL }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(toastMocks.error).not.toHaveBeenCalled();

    const values = onSubmit.mock.calls[0][0];
    expect(values.meta?.providerType).toBe("codebuddy_oauth");
    expect(values.meta?.apiFormat).toBe("openai_chat");
    expect(values.meta?.authBinding).toEqual({
      source: "managed_account",
      authProvider: "codebuddy_oauth",
      accountId: "cb-1",
    });
    expect(values.meta?.codexChatReasoning).toMatchObject({
      supportsEffort: true,
      effortParam: "reasoning_effort",
    });
    expect(values.settingsConfig).toContain("requires_openai_auth = false");
  });

  it("blocks saving a Codex CodeBuddy card when no account is logged in", async () => {
    codebuddyAuth.isAuthenticated = false;
    codebuddyAuth.accounts = [];
    const onSubmit = vi.fn<(values: ProviderFormValues) => void>();
    renderForm(onSubmit, "codex");

    clickCodeBuddyPreset();
    await waitFor(() => {
      expect(screen.getByText("CodeBuddy OAuth 认证")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: SUBMIT_LABEL }));

    await waitFor(() => {
      expect(toastMocks.error).toHaveBeenCalledWith("请先登录 CodeBuddy 账号");
    });
    expect(onSubmit).not.toHaveBeenCalled();
  });
});
