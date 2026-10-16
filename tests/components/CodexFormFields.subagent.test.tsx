import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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

function codexFormProps(
  overrides: Partial<CodexFormFieldsProps> = {},
): CodexFormFieldsProps {
  return {
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
}

function renderCodexForm(overrides: Partial<CodexFormFieldsProps> = {}) {
  const props = codexFormProps(overrides);
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

    for (const name of ["codexConfig.apiUrlLabel", "默认模型", "子代理模型"]) {
      expect(screen.getByRole("button", { name })).toBeInTheDocument();
    }
    // Anthropic 格式的高级区自动展开：后果提示仍在页面上，说明在字段名后的「?」里。
    for (const name of ["上游格式", "认证字段", "最大输出 tokens"]) {
      expect(screen.getByRole("button", { name })).toBeInTheDocument();
    }
    expect(
      screen.getByText("不要超过模型或网关的真实输出上限，否则会返回 400。"),
    ).toBeVisible();
    expect(
      screen.getByText("以下两项只在路由、聚合模式下生效。"),
    ).toBeVisible();
  });
});

const autoReviewSelect = () =>
  document.getElementById("codexAutoReviewModel") as HTMLButtonElement | null;

describe("CodexFormFields Auto Review model", () => {
  it("offers the card's own models and writes the pick through the handler", async () => {
    Element.prototype.scrollIntoView = vi.fn();
    const user = userEvent.setup();
    const onAutoReviewModelChange = vi.fn();
    renderCodexForm({
      catalogModels: [
        { model: "deepseek-v4-pro" },
        { model: "deepseek-v4-flash", displayName: "DeepSeek V4 Flash" },
      ],
      onCatalogModelsChange: vi.fn(),
      onAutoReviewModelChange,
    });
    const trigger = autoReviewSelect()!;
    // 列表里没有 codex-auto-review：默认由主模型自己审批
    expect(trigger.textContent).toBe("主模型自己审批（默认）");
    expect(trigger).not.toBeDisabled();

    await user.click(trigger);
    const options = await screen.findAllByRole("option");
    expect(options.map((option) => option.textContent)).toEqual([
      "主模型自己审批（默认）",
      "deepseek-v4-pro",
      "DeepSeek V4 Flash",
    ]);
    await user.click(screen.getByRole("option", { name: "DeepSeek V4 Flash" }));
    expect(onAutoReviewModelChange).toHaveBeenCalledWith("deepseek-v4-flash");
  });

  it("keeps a saved pick when the list and the value arrive together inside a form", () => {
    // 编辑页：表单先空着渲染，读到卡之后列表和已存的值在同一次渲染里到达。Radix 在 <form>
    // 里带一个隐藏的原生 select，值先到、选项后注册时它会回报一次空串，不能当成「改回默认」。
    const onAutoReviewModelChange = vi.fn();
    const props = codexFormProps({
      catalogModels: [],
      onCatalogModelsChange: vi.fn(),
      onAutoReviewModelChange,
    });
    const inForm = (overrides: Partial<CodexFormFieldsProps>) => (
      <FormShell>
        <form>
          <CodexFormFields {...props} {...overrides} />
        </form>
      </FormShell>
    );
    const { rerender } = render(inForm({}));
    rerender(
      inForm({
        catalogModels: [
          { model: "deepseek-v4-pro" },
          { model: "deepseek-flash" },
        ],
        autoReviewModel: "deepseek-flash",
      }),
    );
    expect(autoReviewSelect()!.textContent).toBe("deepseek-flash");
    expect(onAutoReviewModelChange).not.toHaveBeenCalled();
  });

  it("shows a pick that left the list as the default", () => {
    renderCodexForm({
      catalogModels: [{ model: "deepseek-v4-pro" }],
      onCatalogModelsChange: vi.fn(),
      autoReviewModel: "deepseek-v4-flash",
      onAutoReviewModelChange: vi.fn(),
    });
    expect(autoReviewSelect()!.textContent).toBe("主模型自己审批（默认）");
  });

  it("is disabled with Codex's default and an explanation when the card has no list", () => {
    renderCodexForm({
      catalogModels: [],
      onCatalogModelsChange: vi.fn(),
      onAutoReviewModelChange: vi.fn(),
    });
    const trigger = autoReviewSelect()!;
    expect(trigger).toBeDisabled();
    expect(trigger.textContent).toBe("Codex 默认（codex-auto-review）");
    expect(
      screen.getByText(
        /这张卡没有模型列表：Codex 会用官方的 codex-auto-review 审批/,
      ),
    ).toBeTruthy();
  });

  it("is hidden for the official card", () => {
    renderCodexForm({
      category: "official",
      onAutoReviewModelChange: vi.fn(),
    });
    expect(autoReviewSelect()).toBeNull();
  });
});
