import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ClaudeDesktopProviderForm } from "@/components/providers/forms/ClaudeDesktopProviderForm";
import { createTestQueryClient } from "../utils/testQueryClient";

const authState = vi.hoisted(() => ({
  codexReauthRequired: false,
}));
const toastMocks = vi.hoisted(() => ({
  error: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    error: toastMocks.error,
    success: vi.fn(),
  },
}));

vi.mock("@/components/providers/forms/hooks", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/components/providers/forms/hooks")>();
  return {
    ...actual,
    useCopilotAuth: () => ({
      isAuthenticated: false,
      accounts: [],
    }),
    useCodexOauth: () => ({
      isAuthenticated: true,
      defaultAccountId: "acct-managed",
      accounts: [
        {
          id: "acct-managed",
          is_default: true,
          reauth_required: authState.codexReauthRequired,
          requires_reauth: false,
        },
      ],
    }),
    useXaiOauth: () => ({
      isAuthenticated: false,
      accounts: [],
    }),
  };
});

vi.mock("@/components/providers/forms/CodexOAuthSection", () => ({
  CodexOAuthSection: () => <div data-testid="codex-oauth-section" />,
}));

vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: () => <div data-testid="copilot-auth-section" />,
}));

vi.mock("@/components/providers/forms/XaiOAuthSection", () => ({
  XaiOAuthSection: () => <div data-testid="xai-oauth-section" />,
}));

vi.mock("@/lib/api/providers", () => ({
  providersApi: {
    getClaudeDesktopDefaultRoutes: () => Promise.resolve([]),
  },
}));

function renderForm(
  initialData: ComponentProps<typeof ClaudeDesktopProviderForm>["initialData"],
  onSubmit = vi.fn(),
) {
  const queryClient = createTestQueryClient();
  const view = render(
    <QueryClientProvider client={queryClient}>
      <ClaudeDesktopProviderForm
        submitLabel="保存"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        initialData={initialData}
      />
    </QueryClientProvider>,
  );
  return { ...view, onSubmit };
}

describe("ClaudeDesktopProviderForm", () => {
  beforeEach(() => {
    authState.codexReauthRequired = false;
  });

  it.each(["github_copilot", "codex_oauth", "xai_oauth"])(
    "托管 OAuth %s 即使旧数据是 direct 也强制开启模型映射",
    (providerType) => {
      renderForm({
        name: "Managed OAuth Provider",
        category: "third_party",
        settingsConfig: {
          env: {
            ANTHROPIC_BASE_URL: "https://api.example.com",
          },
        },
        meta: {
          providerType,
          claudeDesktopMode: "direct",
          apiFormat: "anthropic",
          claudeDesktopModelRoutes: {
            "claude-sonnet-5": { model: "upstream-model" },
          },
        },
      });

      const modelModePicker = screen.getByRole("combobox", {
        name: "接入方式",
      });
      expect(modelModePicker).toHaveTextContent("模型映射");
      expect(modelModePicker).toBeDisabled();
    },
  );

  it("新建自定义供应商默认使用直连并显示模型列表", () => {
    renderForm(undefined);

    expect(
      screen.getByRole("combobox", { name: "接入方式" }),
    ).toHaveTextContent("直连");
    expect(screen.getByText("模型列表")).toBeInTheDocument();
    expect(screen.queryByText("模型角色")).not.toBeInTheDocument();
  });

  it("直连预设保留预设模型列表", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    renderForm(undefined, onSubmit);

    await user.click(screen.getByRole("button", { name: /PackyCode/ }));

    expect(screen.getByDisplayValue("claude-sonnet-5")).toBeInTheDocument();
    expect(screen.getByDisplayValue("claude-opus-5")).toBeInTheDocument();
    expect(screen.getByDisplayValue("claude-haiku-4-5")).toBeInTheDocument();

    await user.clear(screen.getByDisplayValue("claude-sonnet-5"));
    await user.type(screen.getByLabelText("API Key"), "sk-test");
    await user.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalled());
    expect(
      onSubmit.mock.calls[0][0].meta.claudeDesktopModelRoutes,
    ).toMatchObject({
      "claude-opus-5": { model: "claude-opus-5" },
      "claude-haiku-4-5": { model: "claude-haiku-4-5" },
    });
    expect(
      onSubmit.mock.calls[0][0].meta.claudeDesktopModelRoutes,
    ).not.toHaveProperty("claude-sonnet-5");
  });

  it("直连与模型映射分别保留自己的模型列表", async () => {
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: vi.fn(),
    });
    const user = userEvent.setup();
    renderForm({
      name: "Proxy Provider",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://api.example.com",
          ANTHROPIC_AUTH_TOKEN: "sk-test",
        },
      },
      meta: {
        claudeDesktopMode: "proxy",
        claudeDesktopModelRoutes: {
          "claude-sonnet-5": {
            model: "upstream-sonnet",
          },
        },
      },
    });

    expect(screen.getByDisplayValue("upstream-sonnet")).toBeInTheDocument();

    await user.click(screen.getByRole("combobox", { name: "接入方式" }));
    await user.click(await screen.findByRole("option", { name: "直连" }));

    expect(screen.getByText("模型列表")).toBeInTheDocument();
    expect(
      screen.queryByPlaceholderText("claude-sonnet-4-6"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByDisplayValue("claude-sonnet-5"),
    ).not.toBeInTheDocument();

    await user.click(screen.getByRole("combobox", { name: "接入方式" }));
    await user.click(await screen.findByRole("option", { name: "模型映射" }));

    expect(screen.getByDisplayValue("upstream-sonnet")).toBeInTheDocument();
  });

  it("编辑模型映射的菜单显示名时保持输入框焦点", () => {
    renderForm({
      name: "Proxy Provider",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://api.example.com",
          ANTHROPIC_AUTH_TOKEN: "sk-test",
        },
      },
      meta: {
        claudeDesktopMode: "proxy",
        claudeDesktopModelRoutes: {
          "claude-old": {
            model: "upstream-old",
          },
        },
      },
    });

    // 固定四档（Sonnet / Opus / Fable / Haiku）下有四个菜单显示名输入，取 Sonnet（首个）。
    const input = screen.getAllByPlaceholderText(
      "DeepSeek V4 Pro",
    )[0] as HTMLInputElement;
    input.focus();

    fireEvent.change(input, { target: { value: "DeepSeek V4 Pro" } });

    const currentInput = screen.getAllByPlaceholderText(
      "DeepSeek V4 Pro",
    )[0] as HTMLInputElement;
    expect(currentInput).toHaveValue("DeepSeek V4 Pro");
    expect(document.activeElement).toBe(currentInput);
  });

  it("编辑直连模型列表的模型 ID 时保持输入框焦点", () => {
    renderForm({
      name: "Direct Provider",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://api.example.com",
          ANTHROPIC_AUTH_TOKEN: "sk-test",
        },
      },
      meta: {
        claudeDesktopMode: "direct",
        claudeDesktopModelRoutes: {
          "claude-old": {
            model: "claude-old",
          },
        },
      },
    });

    const input = screen.getByPlaceholderText(
      "claude-sonnet-4-6",
    ) as HTMLInputElement;
    input.focus();

    fireEvent.change(input, { target: { value: "claude-12345" } });

    const currentInput = screen.getByPlaceholderText(
      "claude-sonnet-4-6",
    ) as HTMLInputElement;
    expect(currentInput).toHaveValue("claude-12345");
    expect(document.activeElement).toBe(currentInput);
  });

  it("代理模式保留超过四条的独立模型路由", () => {
    renderForm({
      name: "Proxy Provider",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://api.example.com",
          ANTHROPIC_AUTH_TOKEN: "sk-test",
        },
      },
      meta: {
        claudeDesktopMode: "proxy",
        claudeDesktopModelRoutes: {
          "claude-sonnet-5": { model: "upstream-sonnet" },
          "claude-opus-5": { model: "upstream-opus" },
          "claude-fable-5": { model: "upstream-fable" },
          "claude-haiku-4-5": { model: "upstream-haiku" },
          "claude-sonnet-5-r2": { model: "upstream-extra" },
        },
      },
    });

    expect(screen.getAllByPlaceholderText("DeepSeek V4 Pro")).toHaveLength(5);
  });

  it("调整模型角色后按新角色保存映射", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    renderForm(
      {
        name: "Proxy Provider",
        settingsConfig: {
          env: {
            ANTHROPIC_BASE_URL: "https://api.example.com",
            ANTHROPIC_AUTH_TOKEN: "sk-test",
          },
        },
        meta: {
          claudeDesktopMode: "proxy",
          claudeDesktopModelRoutes: {
            "claude-sonnet-5": { model: "upstream-a", labelOverride: "A" },
            "claude-sonnet-5-r2": { model: "upstream-b", labelOverride: "B" },
          },
        },
      },
      onSubmit,
    );

    // 第二个模型（Sonnet 2）改选 Haiku 角色，保存后映射键应跟随角色变化。
    await user.click(screen.getByRole("combobox", { name: "模型角色 2" }));
    await user.click(
      await screen.findByRole("option", { name: /Haiku · claude-haiku-4-5$/ }),
    );

    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalled());
    expect(
      Object.keys(onSubmit.mock.calls[0][0].meta.claudeDesktopModelRoutes),
    ).toEqual(["claude-sonnet-5", "claude-haiku-4-5"]);
    expect(onSubmit.mock.calls[0][0].meta.claudeDesktopModelRoutes).toEqual({
      "claude-sonnet-5": { model: "upstream-a", labelOverride: "A" },
      "claude-haiku-4-5": { model: "upstream-b", labelOverride: "B" },
    });
  });

  it("代理模式初始无路由且默认路由未就绪时不渲染空模型", () => {
    // mock 的 getClaudeDesktopDefaultRoutes 返回 []，模拟默认路由尚未就绪。
    // 应保持空、等待 seed effect 的默认路由回填。
    renderForm({
      name: "Proxy Provider",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://api.example.com",
          ANTHROPIC_AUTH_TOKEN: "sk-test",
        },
      },
      meta: {
        claudeDesktopMode: "proxy",
        claudeDesktopModelRoutes: {},
      },
    });

    expect(screen.queryAllByPlaceholderText("DeepSeek V4 Pro")).toHaveLength(0);
  });

  it("保存模型映射时只保留已配置的模型", async () => {
    const onSubmit = vi.fn();
    renderForm(
      {
        name: "Proxy Provider",
        settingsConfig: {
          env: {
            ANTHROPIC_BASE_URL: "https://api.example.com",
            ANTHROPIC_AUTH_TOKEN: "sk-test",
          },
        },
        meta: {
          claudeDesktopMode: "proxy",
          claudeDesktopModelRoutes: {
            "claude-old": {
              model: "upstream-old",
            },
          },
        },
      },
      onSubmit,
    );

    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalled());
    const submitted = onSubmit.mock.calls[0][0];
    // 旧的非安全路由迁移到安全 Sonnet 别名，但不再伪造其它角色的重复模型。
    expect(submitted.meta.claudeDesktopModelRoutes).toEqual({
      "claude-sonnet-5": {
        model: "upstream-old",
        labelOverride: "upstream-old",
      },
    });
  });

  it("保存单一 1M 模型时保留其 1M 声明", async () => {
    const onSubmit = vi.fn();
    renderForm(
      {
        name: "Proxy Provider",
        settingsConfig: {
          env: {
            ANTHROPIC_BASE_URL: "https://api.example.com",
            ANTHROPIC_AUTH_TOKEN: "sk-test",
          },
        },
        meta: {
          claudeDesktopMode: "proxy",
          claudeDesktopModelRoutes: {
            "claude-sonnet-4-6": { model: "deepseek-v4-pro", supports1m: true },
          },
        },
      },
      onSubmit,
    );

    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalled());
    const routes = onSubmit.mock.calls[0][0].meta.claudeDesktopModelRoutes;
    // 合法的 claude-* 别名（含旧版官方 ID）原样保留，不再复制模型到其它角色；
    // 后端会把缺少角色的后台请求回退到此主模型。
    expect(routes["claude-sonnet-4-6"]).toMatchObject({
      model: "deepseek-v4-pro",
      supports1m: true,
    });
    expect(Object.keys(routes)).toEqual(["claude-sonnet-4-6"]);
  });

  it("保存直连模型列表时不会保留旧 route 作为隐藏映射目标", async () => {
    const onSubmit = vi.fn();
    renderForm(
      {
        name: "Direct Provider",
        settingsConfig: {
          env: {
            ANTHROPIC_BASE_URL: "https://api.example.com",
            ANTHROPIC_AUTH_TOKEN: "sk-test",
          },
        },
        meta: {
          claudeDesktopMode: "direct",
          claudeDesktopModelRoutes: {
            "claude-old": {
              model: "claude-old",
            },
          },
        },
      },
      onSubmit,
    );

    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalled());
    const submitted = onSubmit.mock.calls[0][0];
    expect(submitted.meta.claudeDesktopModelRoutes).toMatchObject({
      "claude-sonnet-5": {
        model: "claude-sonnet-5",
      },
    });
  });

  it("不允许保存需要重新登录的 Codex OAuth 账号", async () => {
    authState.codexReauthRequired = true;
    const onSubmit = vi.fn();
    renderForm(
      {
        name: "Codex OAuth Provider",
        category: "third_party",
        settingsConfig: { env: {} },
        meta: {
          providerType: "codex_oauth",
          authBinding: {
            source: "managed_account",
            authProvider: "codex_oauth",
            accountId: "acct-managed",
          },
          claudeDesktopMode: "proxy",
          claudeDesktopModelRoutes: {
            "claude-sonnet-5": { model: "upstream-model" },
          },
        },
      },
      onSubmit,
    );

    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() =>
      expect(toastMocks.error).toHaveBeenCalledWith(
        "已绑定账号不存在或需要重新登录，请重新选择账号",
      ),
    );
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("未选择账号时不允许保存需要重新登录的 Codex OAuth 默认账号", async () => {
    authState.codexReauthRequired = true;
    const onSubmit = vi.fn();
    renderForm(
      {
        name: "Codex OAuth Default Account Provider",
        category: "third_party",
        settingsConfig: { env: {} },
        meta: {
          providerType: "codex_oauth",
          claudeDesktopMode: "proxy",
          claudeDesktopModelRoutes: {
            "claude-sonnet-5": { model: "upstream-model" },
          },
        },
      },
      onSubmit,
    );

    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() =>
      expect(toastMocks.error).toHaveBeenCalledWith(
        "已绑定账号不存在或需要重新登录，请重新选择账号",
      ),
    );
    expect(onSubmit).not.toHaveBeenCalled();
  });
});
