import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentProps, PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ClaudeFormFields } from "@/components/providers/forms/ClaudeFormFields";
import { Form } from "@/components/ui/form";

const copilotApiMock = vi.hoisted(() => ({
  copilotGetModels: vi.fn(),
  copilotGetModelsForAccount: vi.fn(),
}));

const modelFetchApiMock = vi.hoisted(() => ({
  fetchCodexOauthModels: vi.fn(),
  fetchModelsForConfig: vi.fn(),
  showFetchModelsError: vi.fn(),
}));

vi.mock("@/lib/api/copilot", () => ({
  copilotGetModels: copilotApiMock.copilotGetModels,
  copilotGetModelsForAccount: copilotApiMock.copilotGetModelsForAccount,
}));

vi.mock("@/lib/api/model-fetch", () => ({
  fetchCodexOauthModels: modelFetchApiMock.fetchCodexOauthModels,
  fetchModelsForConfig: modelFetchApiMock.fetchModelsForConfig,
  showFetchModelsError: modelFetchApiMock.showFetchModelsError,
}));

vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: () => <div data-testid="copilot-auth-section" />,
}));

vi.mock("@/components/providers/forms/CodexOAuthSection", () => ({
  CodexOAuthSection: () => <div data-testid="codex-oauth-section" />,
}));

type ClaudeFormFieldsProps = ComponentProps<typeof ClaudeFormFields>;

const FormShell = ({ children }: PropsWithChildren) => {
  const form = useForm();

  return <Form {...form}>{children}</Form>;
};

const renderCopilotForm = (overrides: Partial<ClaudeFormFieldsProps> = {}) => {
  const props: ClaudeFormFieldsProps = {
    shouldShowApiKey: false,
    apiKey: "",
    onApiKeyChange: vi.fn(),
    category: "official",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    isCopilotPreset: true,
    usesOAuth: true,
    isCopilotAuthenticated: true,
    selectedGitHubAccountId: "gh-1",
    onGitHubAccountSelect: vi.fn(),
    isCodexOauthPreset: false,
    isCodexOauthAuthenticated: false,
    selectedCodexAccountId: null,
    onCodexAccountSelect: vi.fn(),
    codexFastMode: false,
    onCodexFastModeChange: vi.fn(),
    templateValueEntries: [],
    templateValues: {},
    templatePresetName: "",
    onTemplateValueChange: vi.fn(),
    shouldShowSpeedTest: false,
    baseUrl: "",
    onBaseUrlChange: vi.fn(),
    isEndpointModalOpen: false,
    onEndpointModalToggle: vi.fn(),
    onCustomEndpointsChange: vi.fn(),
    autoSelect: false,
    onAutoSelectChange: vi.fn(),
    showEndpointTools: true,
    shouldShowModelSelector: true,
    claudeModel: "",
    defaultHaikuModel: "",
    defaultHaikuModelName: "",
    defaultSonnetModel: "claude-sonnet",
    defaultSonnetModelName: "Claude Sonnet",
    defaultOpusModel: "",
    defaultOpusModelName: "",
    defaultFableModel: "",
    defaultFableModelName: "",
    subagentModel: "",
    onModelChange: vi.fn(),
    speedTestEndpoints: [],
    apiFormat: "anthropic",
    onApiFormatChange: vi.fn(),
    apiKeyField: "ANTHROPIC_AUTH_TOKEN",
    onApiKeyFieldChange: vi.fn(),
    isFullUrl: false,
    onFullUrlChange: vi.fn(),
    customUserAgent: "",
    onCustomUserAgentChange: vi.fn(),
    localProxyHeadersOverride: "",
    onLocalProxyHeadersOverrideChange: vi.fn(),
    localProxyBodyOverride: "",
    onLocalProxyBodyOverrideChange: vi.fn(),
    ...overrides,
  };

  return render(
    <FormShell>
      <ClaudeFormFields {...props} />
    </FormShell>,
  );
};

const renderCodexOauthForm = (overrides: Partial<ClaudeFormFieldsProps> = {}) =>
  renderCopilotForm({
    isCopilotPreset: false,
    isCopilotAuthenticated: false,
    selectedGitHubAccountId: null,
    isCodexOauthPreset: true,
    isCodexOauthAuthenticated: true,
    selectedCodexAccountId: "chatgpt-1",
    ...overrides,
  });

describe("ClaudeFormFields", () => {
  beforeEach(() => {
    copilotApiMock.copilotGetModels.mockResolvedValue([]);
    copilotApiMock.copilotGetModelsForAccount.mockResolvedValue([]);
    modelFetchApiMock.fetchCodexOauthModels.mockResolvedValue([]);
    modelFetchApiMock.fetchModelsForConfig.mockResolvedValue([]);
  });

  it("不会在 Copilot 表单打开时自动获取模型列表", () => {
    renderCopilotForm();

    expect(copilotApiMock.copilotGetModels).not.toHaveBeenCalled();
    expect(copilotApiMock.copilotGetModelsForAccount).not.toHaveBeenCalled();
  });

  it("点击获取模型列表后才请求当前 Copilot 账号的模型", async () => {
    renderCopilotForm();

    fireEvent.click(
      screen.getByRole("button", {
        name: "providerForm.fetchModels",
      }),
    );

    await waitFor(() => {
      expect(copilotApiMock.copilotGetModelsForAccount).toHaveBeenCalledWith(
        "gh-1",
      );
    });
    expect(copilotApiMock.copilotGetModels).not.toHaveBeenCalled();
  });

  it("不会在 Codex OAuth 表单打开时自动获取模型列表", () => {
    renderCodexOauthForm();

    expect(modelFetchApiMock.fetchCodexOauthModels).not.toHaveBeenCalled();
  });

  it("点击获取模型列表后才请求当前 Codex OAuth 账号的模型", async () => {
    renderCodexOauthForm();

    fireEvent.click(
      screen.getByRole("button", {
        name: "providerForm.fetchModels",
      }),
    );

    await waitFor(() => {
      expect(modelFetchApiMock.fetchCodexOauthModels).toHaveBeenCalledWith(
        "chatgpt-1",
      );
    });
  });

  it("一键设置会同时写入 Subagent 模型", () => {
    const onModelChange = vi.fn();
    renderCopilotForm({
      claudeModel: "shared-model[1M]",
      defaultSonnetModel: "",
      defaultSonnetModelName: "",
      onModelChange,
    });

    fireEvent.click(
      screen.getByRole("button", {
        name: "一键设置",
      }),
    );

    expect(onModelChange).toHaveBeenCalledWith(
      "CLAUDE_CODE_SUBAGENT_MODEL",
      "shared-model[1M]",
    );
  });

  it("Subagent Provider 默认显示当前供应商，并支持选择其他供应商", () => {
    const onSubagentRouteProviderIdChange = vi.fn();
    renderCopilotForm({
      subagentModel: "local-sub[1M]",
      subagentRouteCandidates: [
        { id: "provider-b", name: "Provider B" },
        { id: "provider-c", name: "Provider C" },
      ],
      subagentRouteProviderId: "",
      onSubagentRouteProviderIdChange,
      resolvedSubagentRouteModel: "",
    });

    expect(
      screen.getByText("By default, subagent requests use this provider", {
        exact: false,
      }),
    ).toBeInTheDocument();

    // Open selector and pick a foreign provider
    fireEvent.click(
      screen.getByRole("combobox", { name: /Subagent Provider/i }),
    );
    fireEvent.click(screen.getByRole("option", { name: "Provider B" }));
    expect(onSubagentRouteProviderIdChange).toHaveBeenCalledWith("provider-b");
  });

  it("选择外部供应商时展示目标 Subagent 模型与接管说明", () => {
    renderCopilotForm({
      subagentModel: "local-sub",
      subagentRouteCandidates: [{ id: "provider-b", name: "Provider B" }],
      subagentRouteProviderId: "provider-b",
      onSubagentRouteProviderIdChange: vi.fn(),
      resolvedSubagentRouteModel: "target-subagent[1M]",
    });

    expect(
      screen.getByText(/Target subagent model: target-subagent\[1M\]/i),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Claude local proxy takeover/i),
    ).toBeInTheDocument();
    // Must not leak secrets into route UI labels
    expect(screen.queryByText(/sk-/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/api[_-]?key/i)).not.toBeInTheDocument();
  });

  it("目标供应商缺失时显示校验错误", () => {
    renderCopilotForm({
      subagentRouteCandidates: [],
      subagentRouteProviderId: "deleted-provider",
      onSubagentRouteProviderIdChange: vi.fn(),
      resolvedSubagentRouteModel: "",
      subagentRouteError:
        "Selected subagent provider no longer exists. Choose another provider or Current provider.",
    });

    expect(screen.getByTestId("subagent-route-error")).toHaveTextContent(
      /no longer exists/i,
    );
  });

  it("目标供应商未配置 Subagent 模型时显示校验错误", () => {
    renderCopilotForm({
      subagentRouteCandidates: [{ id: "provider-b", name: "Provider B" }],
      subagentRouteProviderId: "provider-b",
      onSubagentRouteProviderIdChange: vi.fn(),
      resolvedSubagentRouteModel: "",
      subagentRouteError:
        "The selected subagent provider has no Subagent model configured.",
    });

    expect(screen.getByTestId("subagent-route-error")).toHaveTextContent(
      /no Subagent model configured/i,
    );
  });
});
