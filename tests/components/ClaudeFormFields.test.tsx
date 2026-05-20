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
    onModelChange: vi.fn(),
    speedTestEndpoints: [],
    apiFormat: "anthropic",
    onApiFormatChange: vi.fn(),
    apiKeyField: "ANTHROPIC_AUTH_TOKEN",
    onApiKeyFieldChange: vi.fn(),
    isFullUrl: false,
    onFullUrlChange: vi.fn(),
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
});
