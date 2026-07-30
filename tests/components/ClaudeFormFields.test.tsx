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

const getCopilotFormProps = (
  overrides: Partial<ClaudeFormFieldsProps> = {},
): ClaudeFormFieldsProps => ({
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
});

const renderCopilotForm = (overrides: Partial<ClaudeFormFieldsProps> = {}) => {
  const props = getCopilotFormProps(overrides);

  return render(
    <FormShell>
      <ClaudeFormFields {...props} />
    </FormShell>,
  );
};

const getOrdinaryClaudeFormProps = (
  overrides: Partial<ClaudeFormFieldsProps> = {},
): ClaudeFormFieldsProps =>
  getCopilotFormProps({
    category: "custom",
    isCopilotPreset: false,
    usesOAuth: false,
    isCopilotAuthenticated: false,
    selectedGitHubAccountId: null,
    defaultSonnetModel: "",
    defaultSonnetModelName: "",
    onClaudeSubscriptionPassthroughChange: vi.fn(),
    ...overrides,
  });

const renderOrdinaryClaudeForm = (
  overrides: Partial<ClaudeFormFieldsProps> = {},
) => {
  const props = getOrdinaryClaudeFormProps(overrides);

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

  it("在普通供应商模型映射提示后、映射行前渲染订阅透传开关", () => {
    const onToggle = vi.fn();
    renderOrdinaryClaudeForm({
      claudeSubscriptionPassthrough: true,
      onClaudeSubscriptionPassthroughChange: onToggle,
    });

    const hint = screen.getByText("providerForm.modelMappingHint");
    const toggle = screen.getByRole("switch", {
      name: "Claude 订阅透传",
    });
    const firstRole = screen.getByText("Sonnet");

    expect(toggle).toHaveAttribute("data-state", "checked");
    expect(
      hint.compareDocumentPosition(toggle) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      toggle.compareDocumentPosition(firstRole) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();

    fireEvent.click(toggle);
    expect(onToggle).toHaveBeenCalledWith(false);
  });

  it("启用订阅透传时自动展开高级选项", () => {
    renderOrdinaryClaudeForm({
      claudeModel: "",
      defaultSonnetModel: "",
      defaultSonnetModelName: "",
      claudeSubscriptionPassthrough: true,
      onClaudeSubscriptionPassthroughChange: vi.fn(),
    });

    expect(
      screen.getByRole("switch", {
        name: "Claude 订阅透传",
      }),
    ).toHaveAttribute("data-state", "checked");
  });

  it("透传从关闭变为启用时自动展开高级选项", () => {
    const { rerender } = render(
      <FormShell>
        <ClaudeFormFields
          {...({
            ...getOrdinaryClaudeFormProps(),
            claudeSubscriptionPassthrough: false,
          } satisfies ClaudeFormFieldsProps)}
        />
      </FormShell>,
    );

    expect(
      screen.queryByRole("switch", { name: "Claude 订阅透传" }),
    ).toBeNull();

    rerender(
      <FormShell>
        <ClaudeFormFields
          {...({
            ...getOrdinaryClaudeFormProps(),
            claudeSubscriptionPassthrough: true,
          } satisfies ClaudeFormFieldsProps)}
        />
      </FormShell>,
    );

    expect(
      screen.getByRole("switch", { name: "Claude 订阅透传" }),
    ).toHaveAttribute("data-state", "checked");
  });

  it("未接入订阅透传回调时不渲染开关", () => {
    renderOrdinaryClaudeForm({
      customUserAgent: "test-agent",
      onClaudeSubscriptionPassthroughChange: undefined,
    });

    expect(
      screen.queryByRole("switch", {
        name: "Claude 订阅透传",
      }),
    ).toBeNull();
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
});
