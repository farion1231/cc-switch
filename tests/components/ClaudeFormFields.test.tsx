import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState, type ComponentProps, type PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ClaudeFormFields } from "@/components/providers/forms/ClaudeFormFields";
import { useModelState } from "@/components/providers/forms/hooks/useModelState";
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

const buildCopilotFormProps = (
  overrides: Partial<ClaudeFormFieldsProps> = {},
): ClaudeFormFieldsProps => {
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
  return props;
};

const renderCopilotForm = (overrides: Partial<ClaudeFormFieldsProps> = {}) =>
  render(
    <FormShell>
      <ClaudeFormFields {...buildCopilotFormProps(overrides)} />
    </FormShell>,
  );

const renderStatefulCopilotForm = (
  initialConfig: string,
  overrides: Partial<ClaudeFormFieldsProps> = {},
) => {
  const StatefulForm = () => {
    const [settingsConfig, setSettingsConfig] = useState(initialConfig);
    const modelState = useModelState({
      settingsConfig,
      onConfigChange: setSettingsConfig,
    });
    const props = buildCopilotFormProps({
      ...overrides,
      claudeModel: modelState.claudeModel,
      defaultHaikuModel: modelState.defaultHaikuModel,
      defaultHaikuModelName: modelState.defaultHaikuModelName,
      defaultSonnetModel: modelState.defaultSonnetModel,
      defaultSonnetModelName: modelState.defaultSonnetModelName,
      defaultOpusModel: modelState.defaultOpusModel,
      defaultOpusModelName: modelState.defaultOpusModelName,
      defaultFableModel: modelState.defaultFableModel,
      defaultFableModelName: modelState.defaultFableModelName,
      subagentModel: modelState.subagentModel,
      onModelChange: modelState.handleModelChange,
      settingsConfig,
      onSettingsConfigChange: setSettingsConfig,
    });

    return (
      <FormShell>
        <ClaudeFormFields {...props} />
        <div data-testid="live-config" data-config={settingsConfig} />
      </FormShell>
    );
  };

  render(<StatefulForm />);
  return {
    getConfig: () =>
      JSON.parse(
        screen.getByTestId("live-config").dataset.config ?? "{}",
      ) as Record<string, any>,
  };
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
      "shared-model",
    );
  });

  it("角色模型名输入保留新模型后缀原样，不前端剥离", () => {
    const onModelChange = vi.fn();
    renderCopilotForm({
      defaultSonnetModel: "claude-sonnet[200k]",
      defaultSonnetModelName: "Claude Sonnet",
      onModelChange,
    });

    const modelInput = screen.getByDisplayValue("claude-sonnet[200k]");
    fireEvent.change(modelInput, { target: { value: "glm-5.2[200k]" } });

    expect(onModelChange).toHaveBeenCalledWith(
      "ANTHROPIC_DEFAULT_SONNET_MODEL",
      "glm-5.2[200k]",
    );
  });

  it("legacy 后缀模型改名后 contextWindows 保留窗口", () => {
    const { getConfig } = renderStatefulCopilotForm(
      JSON.stringify({
        env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5.2[200k]" },
      }),
    );

    const modelInput = document.getElementById(
      "claudeDefaultSonnetModel",
    ) as HTMLInputElement;
    fireEvent.change(modelInput, { target: { value: "glm-5.3" } });

    const config = getConfig();
    expect(config.env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("glm-5.3");
    expect(config.contextWindows.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe(200000);
  });

  it("非法窗口输入不清空已有 contextWindows", () => {
    const onSettingsConfigChange = vi.fn();
    renderCopilotForm({
      defaultSonnetModel: "glm-5.2",
      settingsConfig: JSON.stringify({
        env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5.2" },
        contextWindows: { ANTHROPIC_DEFAULT_SONNET_MODEL: 200000 },
      }),
      onSettingsConfigChange,
    });

    const contextInputs = screen.getAllByLabelText("Context Window");
    fireEvent.change(contextInputs[0], { target: { value: "1.5M" } });
    fireEvent.blur(contextInputs[0]);

    expect(onSettingsConfigChange).not.toHaveBeenCalled();
  });

  it("角色窗口输入框优先显示 contextWindows 中的值", () => {
    renderCopilotForm({
      defaultSonnetModel: "claude-sonnet",
      settingsConfig: JSON.stringify({
        env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet" },
        contextWindows: { ANTHROPIC_DEFAULT_SONNET_MODEL: 200000 },
      }),
    });

    const contextInputs = screen.getAllByLabelText("Context Window");
    expect(contextInputs[0]).toHaveValue("200000");
  });

  it("角色模型上下文长度输入框写入 contextWindows 且模型名保持干净", () => {
    const onModelChange = vi.fn();
    const onSettingsConfigChange = vi.fn();
    renderCopilotForm({
      defaultSonnetModel: "claude-sonnet",
      defaultSonnetModelName: "Claude Sonnet",
      settingsConfig: JSON.stringify({
        env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet" },
      }),
      onModelChange,
      onSettingsConfigChange,
    });

    // getAllByLabelText 返回所有 context window 输入框，顺序为：
    // [0] Sonnet, [1] Opus, [2] Fable, [3] Haiku, [4] Subagent, [5] 兜底模型
    const contextInputs = screen.getAllByLabelText("Context Window");
    fireEvent.change(contextInputs[0], { target: { value: "1M" } });
    fireEvent.blur(contextInputs[0]);

    expect(onModelChange).not.toHaveBeenCalled();
    const updated = JSON.parse(onSettingsConfigChange.mock.calls[0][0]);
    expect(updated.env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("claude-sonnet");
    expect(updated.contextWindows.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe(1000000);
  });

  it("从旧后缀模型输入窗口 200K 后 env 模型名干净且 contextWindows 写入", () => {
    const onSettingsConfigChange = vi.fn();
    renderCopilotForm({
      defaultSonnetModel: "model[1M]",
      defaultSonnetModelName: "Claude Sonnet",
      settingsConfig: JSON.stringify({
        env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "model[1M]" },
      }),
      onSettingsConfigChange,
    });

    const contextInputs = screen.getAllByLabelText("Context Window");
    fireEvent.change(contextInputs[0], { target: { value: "200K" } });
    fireEvent.blur(contextInputs[0]);

    const updated = JSON.parse(onSettingsConfigChange.mock.calls[0][0]);
    expect(updated.env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("model");
    expect(updated.contextWindows.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe(200000);
  });

  it("清空角色窗口时剥离 env 旧后缀并删除 contextWindows key", () => {
    const onSettingsConfigChange = vi.fn();
    renderCopilotForm({
      defaultSonnetModel: "model[1M]",
      defaultSonnetModelName: "Claude Sonnet",
      settingsConfig: JSON.stringify({
        env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "model[1M]" },
        contextWindows: { ANTHROPIC_DEFAULT_SONNET_MODEL: 1000000 },
      }),
      onSettingsConfigChange,
    });

    const contextInputs = screen.getAllByLabelText("Context Window");
    fireEvent.change(contextInputs[0], { target: { value: "" } });
    fireEvent.blur(contextInputs[0]);

    const updated = JSON.parse(onSettingsConfigChange.mock.calls[0][0]);
    expect(updated.env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("model");
    expect(
      updated.contextWindows.ANTHROPIC_DEFAULT_SONNET_MODEL,
    ).toBeUndefined();
  });

  it("兜底模型窗口输入框读取并写入 ANTHROPIC_MODEL 的 contextWindows", () => {
    const onModelChange = vi.fn();
    const onSettingsConfigChange = vi.fn();
    renderCopilotForm({
      claudeModel: "fallback-model",
      settingsConfig: JSON.stringify({
        env: { ANTHROPIC_MODEL: "fallback-model" },
        contextWindows: { ANTHROPIC_MODEL: 800000 },
      }),
      onModelChange,
      onSettingsConfigChange,
    });

    const contextInputs = screen.getAllByLabelText("Context Window");
    expect(contextInputs[5]).toHaveValue("800000");

    fireEvent.change(contextInputs[5], { target: { value: "200K" } });
    fireEvent.blur(contextInputs[5]);

    expect(onModelChange).not.toHaveBeenCalled();
    const updated = JSON.parse(onSettingsConfigChange.mock.calls[0][0]);
    expect(updated.env.ANTHROPIC_MODEL).toBe("fallback-model");
    expect(updated.contextWindows.ANTHROPIC_MODEL).toBe(200000);
  });

  it("兜底模型名输入保留新模型后缀原样", () => {
    const onModelChange = vi.fn();
    renderCopilotForm({
      claudeModel: "fallback-model[1M]",
      onModelChange,
    });

    const modelInput = document.getElementById(
      "claudeModel",
    ) as HTMLInputElement;
    fireEvent.change(modelInput, { target: { value: "new-model[200K]" } });

    expect(onModelChange).toHaveBeenCalledWith(
      "ANTHROPIC_MODEL",
      "new-model[200K]",
    );
  });

  it("关闭自动同步开关时走独立回调，避免被整包 settingsConfig 更新覆盖", () => {
    const onAutoSyncContextWindowChange = vi.fn();
    const onSettingsConfigChange = vi.fn();
    renderCopilotForm({
      settingsConfig: JSON.stringify({ env: {}, autoSyncContextWindow: true }),
      isProxyTakeover: true,
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
      isProxyTakeover: true,
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
      isProxyTakeover: true,
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
      screen.getByText("关闭后，切换模型不再自动更新上下文长度与压缩阈值。"),
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

  it("压缩比例输入 max=0.95 step=0.05", () => {
    renderCopilotForm({
      settingsConfig: JSON.stringify({ env: {}, autoSyncContextWindow: true }),
    });

    const input = screen.getByLabelText("压缩比例");
    expect(input).toHaveAttribute("max", "0.95");
    expect(input).toHaveAttribute("step", "0.05");
  });

  it("修改压缩比例时走独立回调", () => {
    const onAutoSyncCompactRatioChange = vi.fn();
    renderCopilotForm({
      settingsConfig: JSON.stringify({ env: {}, autoSyncContextWindow: true }),
      isProxyTakeover: true,
      onAutoSyncCompactRatioChange,
    });

    fireEvent.change(screen.getByLabelText("压缩比例"), {
      target: { value: "0.5" },
    });

    expect(onAutoSyncCompactRatioChange).toHaveBeenCalledWith(0.5);
  });

  it("压缩比例 0.95 在范围内时写入", () => {
    const onAutoSyncCompactRatioChange = vi.fn();
    renderCopilotForm({
      settingsConfig: JSON.stringify({ env: {}, autoSyncContextWindow: true }),
      isProxyTakeover: true,
      onAutoSyncCompactRatioChange,
    });

    fireEvent.change(screen.getByLabelText("压缩比例"), {
      target: { value: "0.95" },
    });

    expect(onAutoSyncCompactRatioChange).toHaveBeenCalledWith(0.95);
  });

  it("清空压缩比例时删除字段", () => {
    const onAutoSyncCompactRatioChange = vi.fn();
    renderCopilotForm({
      settingsConfig: JSON.stringify({
        env: {},
        autoSyncContextWindow: true,
        autoSyncCompactRatio: 0.8,
      }),
      isProxyTakeover: true,
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
      isProxyTakeover: true,
      onAutoSyncCompactRatioChange,
    });

    fireEvent.change(screen.getByLabelText("压缩比例"), {
      target: { value: "0.96" },
    });

    expect(onAutoSyncCompactRatioChange).not.toHaveBeenCalled();
    expect(
      screen.getByText("压缩比例必须是 0.2~0.95 之间的数字"),
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

  it("上下文长度框输入 200k 原样显示，失焦后写入 contextWindows 与 token", () => {
    const { getConfig } = renderStatefulCopilotForm(
      JSON.stringify({
        env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet" },
      }),
    );

    const contextInputs = screen.getAllByLabelText("Context Window");
    fireEvent.change(contextInputs[0], { target: { value: "200k" } });
    expect(contextInputs[0]).toHaveValue("200k");

    fireEvent.blur(contextInputs[0]);
    const config = getConfig();
    expect(config.contextWindows.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe(200000);
    // 原样串作为 display-only token 与解析数字同时持久化。
    expect(config.contextWindowTokens.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe(
      "200k",
    );
    // 合法 K/M 输入失焦后仍原样显示（回退到刚写入的 token），不再归一化成数字。
    expect(contextInputs[0]).toHaveValue("200k");
  });

  it("上下文长度框输入 2m 后按回车只结束编辑状态并写入解析值与 token", () => {
    const { getConfig } = renderStatefulCopilotForm(
      JSON.stringify({
        env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet" },
      }),
    );

    const contextInputs = screen.getAllByLabelText("Context Window");
    fireEvent.focus(contextInputs[0]);
    fireEvent.change(contextInputs[0], { target: { value: "2m" } });
    fireEvent.keyDown(contextInputs[0], { key: "Enter" });

    // 回车即提交解析（等价于失焦），不依赖表单保存。
    const config = getConfig();
    expect(config.contextWindows.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe(2000000);
    expect(config.contextWindowTokens.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe(
      "2m",
    );
    expect(contextInputs[0]).toHaveValue("2m");
  });

  it("重开编辑页时优先回显 contextWindowTokens 原样串", () => {
    renderCopilotForm({
      defaultSonnetModel: "claude-sonnet",
      settingsConfig: JSON.stringify({
        env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet" },
        contextWindows: { ANTHROPIC_DEFAULT_SONNET_MODEL: 200000 },
        contextWindowTokens: { ANTHROPIC_DEFAULT_SONNET_MODEL: "200k" },
      }),
    });

    const contextInputs = screen.getAllByLabelText("Context Window");
    expect(contextInputs[0]).toHaveValue("200k");
  });

  it("上下文长度框非法输入失焦后回退到已存值且不写入", () => {
    const onSettingsConfigChange = vi.fn();
    renderCopilotForm({
      defaultSonnetModel: "claude-sonnet",
      settingsConfig: JSON.stringify({
        env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet" },
        contextWindows: { ANTHROPIC_DEFAULT_SONNET_MODEL: 200000 },
      }),
      onSettingsConfigChange,
    });

    const contextInputs = screen.getAllByLabelText("Context Window");
    fireEvent.change(contextInputs[0], { target: { value: "200M3" } });
    fireEvent.blur(contextInputs[0]);

    expect(onSettingsConfigChange).not.toHaveBeenCalled();
    expect(contextInputs[0]).toHaveValue("200000");
  });

  it("模型名输入 glm-5.2[200k] 原样显示，不前端剥离", () => {
    renderStatefulCopilotForm(
      JSON.stringify({
        env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5.2" },
      }),
    );

    const modelInput = document.getElementById(
      "claudeDefaultSonnetModel",
    ) as HTMLInputElement;
    fireEvent.change(modelInput, { target: { value: "glm-5.2[200k]" } });

    // 模型名输入框原样显示带后缀值（显示名输入可能同值，故按 id 断言）。
    expect(
      (document.getElementById("claudeDefaultSonnetModel") as HTMLInputElement)
        .value,
    ).toBe("glm-5.2[200k]");
  });

  it("模型名带 [200k] 后缀时 display-name 同步干净名、模型字段保留后缀", () => {
    const { getConfig } = renderStatefulCopilotForm(
      JSON.stringify({
        env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5.2" },
      }),
    );

    const modelInput = document.getElementById(
      "claudeDefaultSonnetModel",
    ) as HTMLInputElement;
    fireEvent.change(modelInput, { target: { value: "glm-5.2[200k]" } });

    const config = getConfig();
    expect(config.env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("glm-5.2[200k]");
    expect(config.env.ANTHROPIC_DEFAULT_SONNET_MODEL_NAME).toBe("glm-5.2");
  });

  it("路由模式下自动同步开关可切换", () => {
    const onSettingsConfigChange = vi.fn();
    renderCopilotForm({
      settingsConfig: JSON.stringify({ env: {}, autoSyncContextWindow: false }),
      isProxyTakeover: true,
      onSettingsConfigChange,
    });

    const toggle = screen.getByRole("switch", {
      name: "自动同步模型上下文长度",
    });
    expect(toggle).not.toBeDisabled();

    fireEvent.click(toggle);
    expect(onSettingsConfigChange).toHaveBeenCalled();
  });

  it("直连模式下自动同步开关可用且跟随配置状态，不再提示仅路由可用", () => {
    const onSettingsConfigChange = vi.fn();
    renderCopilotForm({
      settingsConfig: JSON.stringify({ env: {}, autoSyncContextWindow: true }),
      isProxyTakeover: false,
      onSettingsConfigChange,
    });

    const toggle = screen.getByRole("switch", {
      name: "自动同步模型上下文长度",
    });
    expect(toggle).not.toBeDisabled();
    expect(toggle).toHaveAttribute("data-state", "checked");
    expect(
      screen.queryByText("自动同步仅在路由（代理接管）模式下可用"),
    ).not.toBeInTheDocument();

    fireEvent.click(toggle);
    expect(onSettingsConfigChange).toHaveBeenCalled();
  });
});
