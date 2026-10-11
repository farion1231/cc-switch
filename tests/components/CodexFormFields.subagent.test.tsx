import { fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps, PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { CodexFormFields } from "@/components/providers/forms/CodexFormFields";
import { Form } from "@/components/ui/form";

vi.mock("@/hooks/useModelMetadataFill", () => ({
  useModelMetadataFill: () => () => {},
}));

type CodexFormFieldsProps = ComponentProps<typeof CodexFormFields>;

const FormShell = ({ children }: PropsWithChildren) => {
  const form = useForm();
  return <Form {...form}>{children}</Form>;
};

const NOT_IN_CATALOG = /该模型不在模型列表中：Codex 拿不到它的上下文窗口/;

function renderCodexForm(overrides: Partial<CodexFormFieldsProps> = {}) {
  const props: CodexFormFieldsProps = {
    codexApiKey: "sk-test",
    onApiKeyChange: vi.fn(),
    category: "third_party",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    codexBaseUrl: "https://api.example.com",
    onBaseUrlChange: vi.fn(),
    isFullUrl: false,
    onFullUrlChange: vi.fn(),
    isEndpointModalOpen: false,
    onEndpointModalToggle: vi.fn(),
    autoSelect: false,
    onAutoSelectChange: vi.fn(),
    codexModel: "gpt-a",
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
    speedTestEndpoints: [],
    customUserAgent: "",
    onCustomUserAgentChange: vi.fn(),
    localProxyHeadersOverride: "",
    onLocalProxyHeadersOverrideChange: vi.fn(),
    localProxyBodyOverride: "",
    onLocalProxyBodyOverrideChange: vi.fn(),
    shouldShowSpeedTest: false,
    subagentModel: "",
    onSubagentModelChange: vi.fn(),
    subagentReasoningEffort: "",
    onSubagentReasoningEffortChange: vi.fn(),
    ...overrides,
  };
  render(
    <FormShell>
      <CodexFormFields {...props} />
    </FormShell>,
  );
  return props;
}

const subagentInput = () =>
  document.getElementById("codexSubagentModel") as HTMLInputElement | null;

describe("CodexFormFields sub-agent defaults", () => {
  it("writes the typed model through the change handler", () => {
    const props = renderCodexForm();
    fireEvent.change(subagentInput()!, { target: { value: "gpt-a-mini" } });
    expect(props.onSubagentModelChange).toHaveBeenCalledWith("gpt-a-mini");
  });

  it("is hidden when the caller gives no handler (Grok Build)", () => {
    renderCodexForm({ onSubagentModelChange: undefined });
    expect(subagentInput()).toBeNull();
  });

  it("offers to add a model outside the list, with the default model when the list is empty", () => {
    const onCatalogModelsChange = vi.fn();
    renderCodexForm({
      apiFormat: "openai_chat",
      subagentModel: "k3-mini",
      catalogModels: [],
      onCatalogModelsChange,
    });
    expect(screen.getByText(NOT_IN_CATALOG)).toBeTruthy();

    onCatalogModelsChange.mockClear();
    fireEvent.click(screen.getByRole("button", { name: "加入列表" }));
    const rows = onCatalogModelsChange.mock.calls.at(-1)?.[0] ?? [];
    expect(rows.map((row: { model: string }) => row.model)).toEqual([
      "gpt-a",
      "k3-mini",
    ]);
  });

  it("stays quiet for a native Responses provider without a list", () => {
    renderCodexForm({ subagentModel: "k3-mini", catalogModels: [] });
    expect(screen.queryByText(NOT_IN_CATALOG)).toBeNull();
  });
});

const rowModels = () =>
  screen
    .getAllByPlaceholderText("例如: deepseek-v4-flash")
    .map((input) => (input as HTMLInputElement).value);

describe("CodexFormFields direct layout", () => {
  it("lets the list own the default model: no separate field, list outside the advanced section", () => {
    renderCodexForm({
      catalogModels: [{ model: "gpt-a" }],
      onCatalogModelsChange: vi.fn(),
    });

    expect(screen.getByText("模型列表")).toBeVisible();
    // 「第一行是默认模型」写在「模型列表」后面的「?」里。
    expect(screen.getByRole("button", { name: "模型列表" })).toBeVisible();
    expect(screen.queryByText("上游格式")).toBeNull();
    expect(document.getElementById("codexDefaultModel")).toBeNull();
    expect(
      screen.getAllByRole("button", { name: /providerForm\.fetchModels/ }),
    ).toHaveLength(1);
  });

  it("shows the default model field below an empty list, and seeds the first row with it", () => {
    const onCatalogModelsChange = vi.fn();
    renderCodexForm({ catalogModels: [], onCatalogModelsChange });

    const listTitle = screen.getByText("模型列表");
    const defaultModel = document.getElementById("codexDefaultModel")!;
    expect(
      listTitle.compareDocumentPosition(defaultModel) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "手动添加" }));
    expect(rowModels()).toEqual(["gpt-a", ""]);
    expect(document.getElementById("codexDefaultModel")).toBeNull();
    const rows = onCatalogModelsChange.mock.calls.at(-1)?.[0] ?? [];
    expect(rows.map((row: { model: string }) => row.model)).toEqual([
      "gpt-a",
      "",
    ]);
  });

  it("shows an older card's default row first without reporting a change", () => {
    const onCatalogModelsChange = vi.fn();
    renderCodexForm({
      codexModel: "gpt-b",
      catalogModels: [{ model: "gpt-a" }, { model: "gpt-b" }],
      onCatalogModelsChange,
    });

    expect(rowModels()).toEqual(["gpt-b", "gpt-a"]);
    expect(onCatalogModelsChange).not.toHaveBeenCalled();
  });

  it("keeps the default model field and its fetch button without a list (Grok Build)", () => {
    renderCodexForm();
    expect(screen.queryByText("模型列表")).toBeNull();
    expect(document.getElementById("codexDefaultModel")).not.toBeNull();
    expect(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    ).toBeInTheDocument();
  });
});

describe("CodexFormFields help tips", () => {
  it("puts field explanations behind「?」and keeps consequences on the page", () => {
    renderCodexForm({
      apiFormat: "anthropic",
      // 有地址栏的卡才显示上游格式。
      shouldShowSpeedTest: true,
      catalogModels: [],
      onCatalogModelsChange: vi.fn(),
    });

    for (const name of [
      "codexConfig.apiUrlLabel",
      "默认模型",
      "子代理模型",
    ]) {
      expect(screen.getByRole("button", { name })).toBeInTheDocument();
    }
    // Anthropic 格式的高级区自动展开：后果提示仍在页面上，说明在字段名后的「?」里。
    for (const name of ["上游格式", "认证字段", "最大输出 tokens"]) {
      expect(screen.getByRole("button", { name })).toBeInTheDocument();
    }
    expect(
      screen.getByText("不要超过模型或网关的真实输出上限，否则会返回 400。"),
    ).toBeVisible();
    expect(screen.getByText("以下两项只在路由、聚合模式下生效。")).toBeVisible();
  });
});
