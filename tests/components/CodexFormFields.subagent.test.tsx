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

const NOT_IN_CATALOG = /该模型不在模型映射中：Codex 拿不到它的上下文窗口/;

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

  it("offers to add a model outside the mapping, with the default model when the mapping is empty", () => {
    const onCatalogModelsChange = vi.fn();
    renderCodexForm({
      apiFormat: "openai_chat",
      subagentModel: "k3-mini",
      catalogModels: [],
      onCatalogModelsChange,
    });
    expect(screen.getByText(NOT_IN_CATALOG)).toBeTruthy();

    onCatalogModelsChange.mockClear();
    fireEvent.click(screen.getByRole("button", { name: "加入映射" }));
    const rows = onCatalogModelsChange.mock.calls.at(-1)?.[0] ?? [];
    expect(rows.map((row: { model: string }) => row.model)).toEqual([
      "gpt-a",
      "k3-mini",
    ]);
  });

  it("stays quiet for a native Responses provider without a mapping", () => {
    renderCodexForm({ subagentModel: "k3-mini", catalogModels: [] });
    expect(screen.queryByText(NOT_IN_CATALOG)).toBeNull();
  });
});
