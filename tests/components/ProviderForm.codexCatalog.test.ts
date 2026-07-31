import {
  createElement,
  type ComponentProps,
  type PropsWithChildren,
} from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { normalizeCodexCatalogModelsForSave } from "@/components/providers/forms/ProviderForm";
import { CodexFormFields } from "@/components/providers/forms/CodexFormFields";
import { Form } from "@/components/ui/form";

const FormShell = ({ children }: PropsWithChildren) => {
  const form = useForm();
  return createElement(Form, { ...form, children });
};

type CodexFormFieldsProps = ComponentProps<typeof CodexFormFields>;

const renderCodexForm = (overrides: Partial<CodexFormFieldsProps> = {}) => {
  const props: CodexFormFieldsProps = {
    codexApiKey: "",
    onApiKeyChange: vi.fn(),
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    shouldShowSpeedTest: false,
    codexBaseUrl: "https://api.deepseek.com",
    onBaseUrlChange: vi.fn(),
    isFullUrl: false,
    onFullUrlChange: vi.fn(),
    isEndpointModalOpen: false,
    onEndpointModalToggle: vi.fn(),
    autoSelect: false,
    onAutoSelectChange: vi.fn(),
    codexModel: "deepseek-v4-flash",
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
    catalogModels: [],
    onCatalogModelsChange: vi.fn(),
    speedTestEndpoints: [],
    customUserAgent: "",
    onCustomUserAgentChange: vi.fn(),
    localProxyHeadersOverride: "",
    onLocalProxyHeadersOverrideChange: vi.fn(),
    localProxyBodyOverride: "",
    onLocalProxyBodyOverrideChange: vi.fn(),
    ...overrides,
  };

  return {
    props,
    ...render(createElement(CodexFormFields, props), { wrapper: FormShell }),
  };
};

describe("ProviderForm Codex catalog helpers", () => {
  it("normalizes catalog rows and removes empty or duplicate models", () => {
    expect(
      normalizeCodexCatalogModelsForSave([
        { model: " deepseek-v4-flash ", displayName: " DeepSeek " },
        { model: "deepseek-v4-flash", displayName: "Duplicate" },
        { model: "", displayName: "Empty" },
        { model: "kimi-k2", contextWindow: "128000 tokens" },
      ]),
    ).toEqual([
      { model: "deepseek-v4-flash", displayName: "DeepSeek" },
      { model: "kimi-k2", contextWindow: 128000 },
    ]);
  });

  it("preserves native-profile overrides (parallel tool calls + input modalities + base instructions)", () => {
    expect(
      normalizeCodexCatalogModelsForSave([
        {
          model: "MiniMax-M3",
          displayName: "MiniMax-M3",
          contextWindow: 1000000,
          supportsParallelToolCalls: true,
          inputModalities: ["text", "image"],
          baseInstructions:
            "  You are Codex, a coding agent based on MiniMax-M3.  ",
        },
        // false must be preserved (not dropped as falsy); empty modalities dropped;
        // empty/whitespace baseInstructions dropped
        {
          model: "mimo-v2.5-pro",
          supportsParallelToolCalls: false,
          inputModalities: [],
          baseInstructions: "   ",
        },
      ]),
    ).toEqual([
      {
        model: "MiniMax-M3",
        displayName: "MiniMax-M3",
        contextWindow: 1000000,
        supportsParallelToolCalls: true,
        inputModalities: ["text", "image"],
        baseInstructions: "You are Codex, a coding agent based on MiniMax-M3.",
      },
      { model: "mimo-v2.5-pro", supportsParallelToolCalls: false },
    ]);
  });

  it("preserves valid native Responses catalog capabilities", () => {
    expect(
      normalizeCodexCatalogModelsForSave([
        {
          model: "deepseek-v4-flash",
          applyPatchToolType: "freeform",
          webSearchToolType: "text",
          supportsSearchTool: true,
          supportVerbosity: true,
          defaultVerbosity: "low",
          supportedReasoningLevels: [
            { effort: "low", description: "Light reasoning" },
            { effort: "high", description: "Deep reasoning" },
          ],
          defaultReasoningLevel: "high",
          truncationPolicy: { mode: "tokens", limit: 10000 },
          multiAgentVersion: "v2",
          minimalClientVersion: "0.144.0",
        },
      ]),
    ).toEqual([
      {
        model: "deepseek-v4-flash",
        applyPatchToolType: "freeform",
        webSearchToolType: "text",
        supportsSearchTool: true,
        supportVerbosity: true,
        defaultVerbosity: "low",
        supportedReasoningLevels: [
          { effort: "low", description: "Light reasoning" },
          { effort: "high", description: "Deep reasoning" },
        ],
        defaultReasoningLevel: "high",
        truncationPolicy: { mode: "tokens", limit: 10000 },
        multiAgentVersion: "v2",
        minimalClientVersion: "0.144.0",
      },
    ]);
  });

  it("normalizes capability strings and drops malformed hidden metadata", () => {
    expect(
      normalizeCodexCatalogModelsForSave([
        {
          model: "deepseek-v4-flash",
          applyPatchToolType: "function",
          webSearchToolType: "video",
          supportsSearchTool: false,
          supportedReasoningLevels: [
            { effort: " low ", description: " Light reasoning " },
            { effort: "", description: "Missing effort" },
            { effort: "high", description: "   " },
            null,
          ],
          defaultReasoningLevel: " high ",
          truncationPolicy: { mode: "tokens", limit: -1 },
          multiAgentVersion: " v2 ",
          minimalClientVersion: " 0.144.0 ",
        } as any,
      ]),
    ).toEqual([
      {
        model: "deepseek-v4-flash",
        supportsSearchTool: false,
        supportedReasoningLevels: [
          { effort: "low", description: "Light reasoning" },
        ],
        defaultReasoningLevel: "high",
        multiAgentVersion: "v2",
        minimalClientVersion: "0.144.0",
      },
    ]);
  });

  it("keeps hidden catalog capabilities when a visible row field is edited", async () => {
    const onCatalogModelsChange = vi.fn();
    const catalogModel = {
      model: "deepseek-v4-flash",
      displayName: "DeepSeek V4 Flash",
      contextWindow: 1048576,
      supportsParallelToolCalls: true,
      inputModalities: ["text"],
      applyPatchToolType: "freeform" as const,
      webSearchToolType: "text" as const,
      supportsSearchTool: true,
      supportVerbosity: true,
      defaultVerbosity: "low",
      supportedReasoningLevels: [
        { effort: "low", description: "Light reasoning" },
        { effort: "high", description: "Deep reasoning" },
      ],
      defaultReasoningLevel: "high",
      truncationPolicy: { mode: "tokens" as const, limit: 10000 },
      multiAgentVersion: "v2",
      minimalClientVersion: "0.144.0",
    };

    renderCodexForm({ catalogModels: [catalogModel], onCatalogModelsChange });

    fireEvent.change(screen.getByDisplayValue("DeepSeek V4 Flash"), {
      target: { value: "DeepSeek Flash" },
    });

    await waitFor(() => expect(onCatalogModelsChange).toHaveBeenCalled());
    expect(onCatalogModelsChange.mock.lastCall?.[0]).toEqual([
      { ...catalogModel, displayName: "DeepSeek Flash" },
    ]);
  });

  it("refreshes rows when structured hidden capabilities change", async () => {
    const onCatalogModelsChange = vi.fn();
    const initialModel = {
      model: "deepseek-v4-flash",
      displayName: "DeepSeek V4 Flash",
      supportedReasoningLevels: [
        { effort: "low", description: "Light reasoning" },
      ],
      truncationPolicy: { mode: "tokens" as const, limit: 10000 },
    };
    const updatedModel = {
      ...initialModel,
      supportedReasoningLevels: [
        { effort: "high", description: "Deep reasoning" },
        { effort: "max", description: "Maximum reasoning" },
      ],
      truncationPolicy: { mode: "tokens" as const, limit: 20000 },
    };
    const { props, rerender } = renderCodexForm({
      catalogModels: [initialModel],
      onCatalogModelsChange,
    });

    rerender(
      createElement(CodexFormFields, {
        ...props,
        catalogModels: [updatedModel],
      }),
    );
    fireEvent.change(screen.getByDisplayValue("DeepSeek V4 Flash"), {
      target: { value: "DeepSeek Flash" },
    });

    await waitFor(() => expect(onCatalogModelsChange).toHaveBeenCalled());
    expect(onCatalogModelsChange.mock.lastCall?.[0]).toEqual([
      {
        ...updatedModel,
        displayName: "DeepSeek Flash",
        contextWindow: "",
      },
    ]);
  });

  it("refreshes rows when scalar hidden capabilities change", async () => {
    const onCatalogModelsChange = vi.fn();
    const initialModel = {
      model: "deepseek-v4-flash",
      displayName: "DeepSeek V4 Flash",
    };
    const updatedModel = {
      ...initialModel,
      applyPatchToolType: "freeform" as const,
      webSearchToolType: "text" as const,
      supportsSearchTool: true,
      supportVerbosity: true,
      defaultVerbosity: "low",
      defaultReasoningLevel: "high",
      multiAgentVersion: "v2",
      minimalClientVersion: "0.144.0",
    };
    const { props, rerender } = renderCodexForm({
      catalogModels: [initialModel],
      onCatalogModelsChange,
    });

    rerender(
      createElement(CodexFormFields, {
        ...props,
        catalogModels: [updatedModel],
      }),
    );
    fireEvent.change(screen.getByDisplayValue("DeepSeek V4 Flash"), {
      target: { value: "DeepSeek Flash" },
    });

    await waitFor(() => expect(onCatalogModelsChange).toHaveBeenCalled());
    expect(onCatalogModelsChange.mock.lastCall?.[0]).toEqual([
      {
        ...updatedModel,
        displayName: "DeepSeek Flash",
        contextWindow: "",
      },
    ]);
  });
});
