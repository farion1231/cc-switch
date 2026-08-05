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

  it("角色模型上下文长度输入框能正确写入后缀", () => {
    const onModelChange = vi.fn();
    renderCopilotForm({
      defaultSonnetModel: "claude-sonnet",
      defaultSonnetModelName: "Claude Sonnet",
      onModelChange,
    });

    // getAllByLabelText 返回所有 context window 输入框，顺序为：
    // [0] Sonnet, [1] Opus, [2] Fable, [3] Haiku, [4] Subagent, [5] 兜底模型
    const contextInputs = screen.getAllByLabelText("Context Window");
    const sonnetContextInput = contextInputs[0];

    fireEvent.change(sonnetContextInput, { target: { value: "1M" } });

    expect(onModelChange).toHaveBeenCalledWith(
      "ANTHROPIC_DEFAULT_SONNET_MODEL",
      "claude-sonnet[1m]",
    );
  });

  it("关闭自动同步开关时走独立回调，避免被整包 settingsConfig 更新覆盖", () => {
    const onAutoSyncContextWindowChange = vi.fn();
    const onSettingsConfigChange = vi.fn();
    renderCopilotForm({
      settingsConfig: JSON.stringify({ env: {}, autoSyncContextWindow: true }),
      onSettingsConfigChange,
      onAutoSyncContextWindowChange,
    });

    fireEvent.click(
      screen.getByRole("switch", {
        name: "自动同步模型上下文长度",
      }),
    );

    expect(onAutoSyncContextWindowChange).toHaveBeenCalledWith(false);
    expect(onSettingsConfigChange).not.toHaveBeenCalled();
  });

  it("没有独立回调时，关闭开关会清理 settingsConfig 中的 ACW/MAX", () => {
    const onSettingsConfigChange = vi.fn();
    renderCopilotForm({
      settingsConfig: JSON.stringify({
        env: {
          ANTHROPIC_MODEL: "model[1M]",
          CLAUDE_CODE_AUTO_COMPACT_WINDOW: "800000",
          CLAUDE_CODE_MAX_CONTEXT_TOKENS: "1000000",
        },
        autoSyncContextWindow: true,
      }),
      onSettingsConfigChange,
    });

    fireEvent.click(
      screen.getByRole("switch", {
        name: "自动同步模型上下文长度",
      }),
    );

    const updated = JSON.parse(onSettingsConfigChange.mock.calls[0][0]);
    expect(updated.autoSyncContextWindow).toBe(false);
    expect(updated.env.CLAUDE_CODE_AUTO_COMPACT_WINDOW).toBeUndefined();
    expect(updated.env.CLAUDE_CODE_MAX_CONTEXT_TOKENS).toBeUndefined();
    expect(updated.env.ANTHROPIC_MODEL).toBe("model[1M]");
  });

  it("开关打开和关闭时显示对应的上下文同步提示", () => {
    renderCopilotForm({
      settingsConfig: JSON.stringify({ env: {}, autoSyncContextWindow: true }),
    });

    expect(
      screen.getByText(
        "上下文长度和压缩阈值按切换的模型更新配置 json。切换后需重启 Claude Code（退出后用 claude --resume 恢复会话）才生效。多CC终端使用不同模型，以最后切换模型时的上下文长度作为全局变量。",
      ),
    ).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("switch", {
        name: "自动同步模型上下文长度",
      }),
    );

    expect(
      screen.getByText(
        "对于[1M]后缀的模型Claude Code原生支持1M上下文，其他输入形式或不输入默认为200k，切换模型后终端内生效。",
      ),
    ).toBeInTheDocument();
  });

  it("settingsConfig 缺少 autoSyncContextWindow 时开关默认关闭", () => {
    renderCopilotForm({
      settingsConfig: JSON.stringify({ env: {} }),
    });

    expect(
      screen.getByRole("switch", {
        name: "自动同步模型上下文长度",
      }),
    ).not.toBeChecked();
  });

  it("自动同步开启时显示已保存的压缩比例", () => {
    renderCopilotForm({
      settingsConfig: JSON.stringify({
        env: {},
        autoSyncContextWindow: true,
        autoSyncCompactRatio: 0.8,
      }),
    });

    expect(screen.getByLabelText("压缩比例")).toHaveValue(0.8);
  });

  it("修改压缩比例时走独立回调", () => {
    const onAutoSyncCompactRatioChange = vi.fn();
    renderCopilotForm({
      settingsConfig: JSON.stringify({ env: {}, autoSyncContextWindow: true }),
      onAutoSyncCompactRatioChange,
    });

    fireEvent.change(screen.getByLabelText("压缩比例"), {
      target: { value: "0.5" },
    });

    expect(onAutoSyncCompactRatioChange).toHaveBeenCalledWith(0.5);
  });

  it("清空压缩比例时删除字段", () => {
    const onAutoSyncCompactRatioChange = vi.fn();
    renderCopilotForm({
      settingsConfig: JSON.stringify({
        env: {},
        autoSyncContextWindow: true,
        autoSyncCompactRatio: 0.8,
      }),
      onAutoSyncCompactRatioChange,
    });

    fireEvent.change(screen.getByLabelText("压缩比例"), {
      target: { value: "" },
    });

    expect(onAutoSyncCompactRatioChange).toHaveBeenCalledWith(null);
  });

  it("压缩比例超出范围时显示错误且不写入", () => {
    const onAutoSyncCompactRatioChange = vi.fn();
    renderCopilotForm({
      settingsConfig: JSON.stringify({ env: {}, autoSyncContextWindow: true }),
      onAutoSyncCompactRatioChange,
    });

    fireEvent.change(screen.getByLabelText("压缩比例"), {
      target: { value: "1.5" },
    });

    expect(onAutoSyncCompactRatioChange).not.toHaveBeenCalled();
    expect(
      screen.getByText("压缩比例必须是 0.2~1 之间的数字"),
    ).toBeInTheDocument();
  });

  it("自动同步关闭时压缩比例输入框禁用", () => {
    renderCopilotForm({
      settingsConfig: JSON.stringify({
        env: {},
        autoSyncContextWindow: false,
        autoSyncCompactRatio: 0.8,
      }),
    });

    expect(screen.getByLabelText("压缩比例")).toBeDisabled();
  });
});
