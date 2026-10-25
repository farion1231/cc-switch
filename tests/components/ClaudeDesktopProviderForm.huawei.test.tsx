import { lazy } from "react";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { ClaudeDesktopProviderForm } from "@/components/providers/forms/ClaudeDesktopProviderForm";
import { registerFormExtension } from "@/config/formExtensions";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";
import { huaweiDesktopPresets } from "@/huawei/huaweiDesktopPresets";
import { huaweiFormBehavior } from "@/huawei/formBehavior";
import { injectHuaweiProviderTypes } from "@/huawei/constants";
import { createTestQueryClient } from "../utils/testQueryClient";

const api = vi.hoisted(() => ({
  listAccounts: vi.fn(),
  startLogin: vi.fn(),
  removeAccount: vi.fn(),
  setDefaultAccount: vi.fn(),
  logout: vi.fn(),
  fetchModels: vi.fn(),
  fetchQuota: vi.fn(),
}));
vi.mock("@/huawei/huaweiCodeAgent2Api", () => ({
  huaweiCodeAgent2Api: api,
  fetchHuaweiCodeAgent2Models: api.fetchModels,
  fetchHuaweiQuota: api.fetchQuota,
  fetchHuaweiModelQuota: vi.fn(),
}));
vi.mock("@/components/providers/forms/hooks", async (importOriginal) => ({
  ...(await importOriginal<
    typeof import("@/components/providers/forms/hooks")
  >()),
  useCopilotAuth: () => ({ isAuthenticated: false, accounts: [] }),
  useCodexOauth: () => ({ isAuthenticated: false, accounts: [] }),
  useXaiOauth: () => ({ isAuthenticated: false, accounts: [] }),
}));
vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: () => <div>Copilot auth</div>,
}));
vi.mock("@/components/providers/forms/CodexOAuthSection", () => ({
  CodexOAuthSection: () => <div>ChatGPT auth</div>,
}));
vi.mock("@/components/providers/forms/XaiOAuthSection", () => ({
  XaiOAuthSection: () => <div data-testid="xai-auth">xAI auth</div>,
}));
vi.mock("@/lib/api/providers", () => ({
  providersApi: { getClaudeDesktopDefaultRoutes: async () => [] },
}));

const authHeaders = [
  { name: "x-auth-token", tokenSource: "huawei_codeagent_2" },
];
beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
    configurable: true,
    value: vi.fn(),
  });
  injectHuaweiProviderTypes();
  claudeDesktopProviderPresets.push(...huaweiDesktopPresets);
  for (const providerType of ["huawei_codeagent_2", "huawei_codeagent_3"]) {
    registerFormExtension({
      providerType,
      ...huaweiFormBehavior,
      skipApiKeyRequired: true,
      AuthSection: lazy(() =>
        import("@/huawei/HuaweiCodeAgentOAuthSection").then((m) => ({
          default: m.HuaweiCodeAgentOAuthSection,
        })),
      ),
      defaultMetaFields: {
        extraAuthHeaders: authHeaders,
        headerMode: "whitelist",
      },
      fetchModels: api.fetchModels,
    });
  }
});
beforeEach(() => {
  vi.clearAllMocks();
  api.listAccounts.mockResolvedValue([
    {
      id: "account-one",
      user: "test-huawei-user",
      is_default: true,
      requires_reauth: false,
    },
  ]);
  api.startLogin.mockResolvedValue({});
  api.fetchModels.mockResolvedValue([{ id: "test-model", ownedBy: null }]);
});

function renderForm(providerType?: string) {
  const onSubmit = vi.fn();
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ClaudeDesktopProviderForm
        submitLabel="保存"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        initialData={
          providerType
            ? {
                name: "CodeAgent",
                category: "cloud_provider",
                settingsConfig: {
                  env: { ANTHROPIC_BASE_URL: "https://example.test" },
                },
                meta: {
                  providerType,
                  selectedAccountId: "account-one",
                  claudeDesktopMode: "proxy",
                  claudeDesktopModelRoutes: {
                    "claude-sonnet-5": { model: "test-model" },
                  },
                },
              }
            : undefined
        }
      />
    </QueryClientProvider>,
  );
  return onSubmit;
}

describe("CodeAgent Desktop authentication", () => {
  it("queries models and quota for the selected Huawei account", async () => {
    api.fetchQuota.mockResolvedValue({
      quotas: [{ type: "CPM", used_quota: 25, total_quota: 100 }],
    });
    renderForm("huawei_codeagent_3");
    await screen.findByPlaceholderText("用户名");
    fireEvent.click(screen.getByRole("button", { name: /获取模型/ }));
    await waitFor(() =>
      expect(api.fetchModels).toHaveBeenCalledWith("account-one"),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "查询华为额度用量" }),
    );
    await screen.findByText("25 / 100");
    expect(api.fetchQuota).toHaveBeenCalledWith("account-one");
  });

  it("uses a registered save policy for an unrelated provider ID", async () => {
    const onSaveMeta = vi.fn(({ appId, selectedAccountId }) => ({
      extensionTarget: appId,
      selectedAccountId,
    }));
    registerFormExtension({
      providerType: "test-custom-extension",
      skipApiKeyRequired: true,
      forceDesktopProxy: true,
      AuthSection: lazy(async () => ({
        default: () => <div>Custom auth</div>,
      })),
      onSaveMeta,
    });
    const onSubmit = renderForm("test-custom-extension");
    await screen.findByText("Custom auth");
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSaveMeta).toHaveBeenCalledWith({
      appId: "claude-desktop",
      selectedAccountId: "account-one",
    });
    expect(onSubmit.mock.calls[0][0].meta).toMatchObject({
      extensionTarget: "claude-desktop",
      selectedAccountId: "account-one",
      claudeDesktopMode: "proxy",
    });
  });

  it.each(["huawei_codeagent_2", "huawei_codeagent_3"])(
    "%s shows Huawei login and preserves its selected account and proxy metadata on save",
    async (providerType) => {
      const onSubmit = renderForm(providerType);
      await screen.findByPlaceholderText("用户名");
      expect(screen.queryByTestId("xai-auth")).not.toBeInTheDocument();
      expect(screen.queryByLabelText("API Key")).not.toBeInTheDocument();
      await screen.findAllByText("test-huawei-user");

      fireEvent.click(screen.getByRole("button", { name: "保存" }));
      await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
      const saved = onSubmit.mock.calls[0][0];
      expect(saved.meta).toMatchObject({
        providerType,
        selectedAccountId: "account-one",
        appTarget: "claude_desktop",
        extraAuthHeaders: authHeaders,
        headerMode: "whitelist",
        claudeDesktopMode: "proxy",
      });
      expect(saved.meta.authBinding?.authProvider).not.toBe("xai_oauth");
      expect(JSON.parse(saved.settingsConfig).env).not.toHaveProperty(
        "ANTHROPIC_AUTH_TOKEN",
      );
    },
  );

  it("new CodeAgent presets use Huawei login and model discovery", async () => {
    const user = userEvent.setup();
    renderForm();
    await user.click(
      screen.getByRole("button", { name: /Huawei CodeAgent 3.0/ }),
    );
    await screen.findByPlaceholderText("用户名");
    expect(screen.queryByTestId("xai-auth")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /获取模型/ }));
    await waitFor(() => expect(api.fetchModels).toHaveBeenCalledTimes(1));
  });

  it("submits login credentials to Huawei and can save the default-account selection", async () => {
    const user = userEvent.setup();
    const onSubmit = renderForm("huawei_codeagent_2");
    await user.type(
      await screen.findByPlaceholderText("用户名"),
      "new-test-user",
    );
    await user.type(screen.getByPlaceholderText("密码"), "test-password");
    await user.click(
      await screen.findByRole("button", { name: "添加账号或重新登录" }),
    );
    await waitFor(() =>
      expect(api.startLogin).toHaveBeenCalledWith(
        "new-test-user",
        "test-password",
      ),
    );

    await user.click(
      within(screen.getByText("选择账号").parentElement!).getByRole("combobox"),
    );
    await user.click(screen.getByRole("option", { name: "使用默认账号" }));
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit.mock.calls[0][0].meta.selectedAccountId).toBeUndefined();
  });
});
