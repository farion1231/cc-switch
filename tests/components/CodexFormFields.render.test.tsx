import { render, screen } from "@testing-library/react";
import type { ComponentProps, PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodexFormFields } from "@/components/providers/forms/CodexFormFields";
import { Form } from "@/components/ui/form";

// 这个测试文件专门覆盖 CodexFormFields.modelFilter.test.ts 覆盖不到的一类
// bug：纯函数逻辑全对，但渲染条件/JSX 门控写错，导致 UI 实际没按预期展示
// 控件。上一轮就是这类问题——1M 相关纯函数测试全部通过，但 checkbox 的渲染
// 门控条件把大多数行都挡回了数字输入框，用户看到的还是输入框。

const copilotApiMock = vi.hoisted(() => ({
  copilotGetModels: vi.fn(),
  copilotGetModelsForAccount: vi.fn(),
}));

const modelFetchApiMock = vi.hoisted(() => ({
  fetchModelsForConfig: vi.fn(),
  showFetchModelsError: vi.fn(),
}));

vi.mock("@/lib/api/copilot", () => ({
  copilotGetModels: copilotApiMock.copilotGetModels,
  copilotGetModelsForAccount: copilotApiMock.copilotGetModelsForAccount,
}));

vi.mock("@/lib/api/model-fetch", () => ({
  fetchModelsForConfig: modelFetchApiMock.fetchModelsForConfig,
  showFetchModelsError: modelFetchApiMock.showFetchModelsError,
}));

vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: () => <div data-testid="copilot-auth-section" />,
}));

type CodexFormFieldsProps = ComponentProps<typeof CodexFormFields>;

const FormShell = ({ children }: PropsWithChildren) => {
  const form = useForm();
  return <Form {...form}>{children}</Form>;
};

const renderCodexForm = (overrides: Partial<CodexFormFieldsProps> = {}) => {
  const props: CodexFormFieldsProps = {
    codexApiKey: "",
    onApiKeyChange: vi.fn(),
    category: "third_party",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    shouldShowSpeedTest: false,
    codexBaseUrl: "",
    onBaseUrlChange: vi.fn(),
    isFullUrl: false,
    onFullUrlChange: vi.fn(),
    isEndpointModalOpen: false,
    onEndpointModalToggle: vi.fn(),
    onCustomEndpointsChange: vi.fn(),
    autoSelect: false,
    onAutoSelectChange: vi.fn(),
    codexModel: "",
    onModelChange: vi.fn(),
    apiFormat: "openai_responses",
    onApiFormatChange: vi.fn(),
    anthropicAuthField: "ANTHROPIC_AUTH_TOKEN",
    onAnthropicAuthFieldChange: vi.fn(),
    impersonateClaudeCode: false,
    onImpersonateClaudeCodeChange: vi.fn(),
    maxOutputTokens: "",
    onMaxOutputTokensChange: vi.fn(),
    promptCacheRouting: "auto",
    onPromptCacheRoutingChange: vi.fn(),
    catalogModels: [{ model: "claude-opus-4-5" }],
    onCatalogModelsChange: vi.fn(),
    speedTestEndpoints: [],
    customUserAgent: "",
    onCustomUserAgentChange: vi.fn(),
    localProxyHeadersOverride: "",
    onLocalProxyHeadersOverrideChange: vi.fn(),
    localProxyBodyOverride: "",
    onLocalProxyBodyOverrideChange: vi.fn(),
    isCopilotPreset: true,
    usesOAuth: true,
    selectedGitHubAccountId: "gh-1",
    onGitHubAccountSelect: vi.fn(),
    ...overrides,
  };

  return render(
    <FormShell>
      <CodexFormFields {...props} />
    </FormShell>,
  );
};

describe("CodexFormFields 模型映射行的 1M 控件", () => {
  beforeEach(() => {
    copilotApiMock.copilotGetModels.mockResolvedValue([]);
    copilotApiMock.copilotGetModelsForAccount.mockResolvedValue([]);
    modelFetchApiMock.fetchModelsForConfig.mockResolvedValue([]);
  });

  it("GitHub Copilot 供应商：已有 Claude 模型的行显示 1M 勾选框，而不是数字输入框", () => {
    renderCodexForm({ catalogModels: [{ model: "claude-opus-4-5" }] });

    expect(screen.getByRole("checkbox", { name: "1M" })).toBeInTheDocument();
    expect(
      screen.queryByLabelText("codexConfig.catalogColumnContext"),
    ).not.toBeInTheDocument();
  });

  it("GitHub Copilot 供应商：新增的空白行也显示 1M 勾选框（不随模型文本切换控件类型）", () => {
    renderCodexForm({ catalogModels: [{ model: "" }] });

    expect(screen.getByRole("checkbox", { name: "1M" })).toBeInTheDocument();
  });

  it("GitHub Copilot 供应商：GPT 模型的行同样显示 1M 勾选框", () => {
    renderCodexForm({ catalogModels: [{ model: "gpt-5.3-codex" }] });

    expect(screen.getByRole("checkbox", { name: "1M" })).toBeInTheDocument();
  });

  it("非 Copilot 的 Codex 供应商：模型映射行仍显示数字输入框，不受影响", () => {
    renderCodexForm({
      isCopilotPreset: false,
      usesOAuth: false,
      selectedGitHubAccountId: null,
      catalogModels: [{ model: "deepseek-v4-flash" }],
    });

    expect(
      screen.queryByRole("checkbox", { name: "1M" }),
    ).not.toBeInTheDocument();
    expect(screen.getByLabelText("上下文窗口")).toBeInTheDocument();
  });

  it("GitHub Copilot 供应商：上游协议由模型能力自动选择，不显示手动格式下拉", () => {
    renderCodexForm({ shouldShowSpeedTest: true });

    expect(screen.getByText("按模型自动选择")).toBeInTheDocument();
    expect(screen.queryByLabelText("上游格式")).not.toBeInTheDocument();
  });
});
