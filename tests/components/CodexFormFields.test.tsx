import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentProps, PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { CodexFormFields } from "@/components/providers/forms/CodexFormFields";
import { Form } from "@/components/ui/form";

type CodexFormFieldsProps = ComponentProps<typeof CodexFormFields>;

const FormShell = ({ children }: PropsWithChildren) => {
  const form = useForm();

  return <Form {...form}>{children}</Form>;
};

const renderFields = (
  catalogModels: NonNullable<CodexFormFieldsProps["catalogModels"]>,
) => {
  const onCatalogModelsChange = vi.fn();
  const props: CodexFormFieldsProps = {
    codexApiKey: "",
    onApiKeyChange: vi.fn(),
    category: "custom",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    shouldShowSpeedTest: false,
    codexBaseUrl: "https://api.example.com/v1",
    onBaseUrlChange: vi.fn(),
    isFullUrl: false,
    onFullUrlChange: vi.fn(),
    isEndpointModalOpen: false,
    onEndpointModalToggle: vi.fn(),
    autoSelect: false,
    onAutoSelectChange: vi.fn(),
    codexModel: "gpt-5.3-codex",
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
    catalogModels,
    onCatalogModelsChange,
    speedTestEndpoints: [],
    customUserAgent: "",
    onCustomUserAgentChange: vi.fn(),
    localProxyHeadersOverride: "",
    onLocalProxyHeadersOverrideChange: vi.fn(),
    localProxyBodyOverride: "",
    onLocalProxyBodyOverrideChange: vi.fn(),
  };

  render(
    <FormShell>
      <CodexFormFields {...props} />
    </FormShell>,
  );
  return onCatalogModelsChange;
};

describe("CodexFormFields model mapping action", () => {
  it("replaces the only mapping after an explicit action", async () => {
    const onCatalogModelsChange = renderFields([
      {
        model: "gpt-5.6-sol",
        displayName: "gpt-5.6-sol",
        contextWindow: 372000,
      },
    ]);

    fireEvent.click(screen.getByRole("button", { name: "替换映射" }));

    await waitFor(() =>
      expect(onCatalogModelsChange).toHaveBeenLastCalledWith([
        {
          model: "gpt-5.3-codex",
          displayName: "gpt-5.3-codex",
          contextWindow: 372000,
        },
      ]),
    );
  });

  it("adds the default without replacing multiple mappings", async () => {
    const onCatalogModelsChange = renderFields([
      { model: "gpt-5.6-sol" },
      { model: "gpt-5.6-luna" },
    ]);

    fireEvent.click(screen.getByRole("button", { name: "加入映射" }));

    await waitFor(() => {
      const models = onCatalogModelsChange.mock.lastCall?.[0];
      expect(models?.map(({ model }: { model: string }) => model)).toEqual([
        "gpt-5.6-sol",
        "gpt-5.6-luna",
        "gpt-5.3-codex",
      ]);
    });
  });
});
