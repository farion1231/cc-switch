import { render, screen, waitFor } from "@testing-library/react";
import type { PropsWithChildren } from "react";
import { FormProvider, useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import type { CodexApiFormat } from "@/types";
import { CodexFormFields } from "./CodexFormFields";

function TestForm({ children }: PropsWithChildren) {
  const form = useForm();
  return <FormProvider {...form}>{children}</FormProvider>;
}

function renderFields(apiFormat: CodexApiFormat) {
  const onCatalogModelsChange = vi.fn();
  render(
    <TestForm>
      <CodexFormFields
        appId="codex"
        codexApiKey=""
        onApiKeyChange={vi.fn()}
        category="third_party"
        shouldShowApiKeyLink={false}
        websiteUrl=""
        shouldShowSpeedTest={false}
        codexBaseUrl=""
        onBaseUrlChange={vi.fn()}
        isFullUrl={false}
        onFullUrlChange={vi.fn()}
        isEndpointModalOpen={false}
        onEndpointModalToggle={vi.fn()}
        autoSelect={false}
        onAutoSelectChange={vi.fn()}
        apiFormat={apiFormat}
        onApiFormatChange={vi.fn()}
        anthropicAuthField="ANTHROPIC_AUTH_TOKEN"
        onAnthropicAuthFieldChange={vi.fn()}
        impersonateClaudeCode={false}
        onImpersonateClaudeCodeChange={vi.fn()}
        maxOutputTokens=""
        onMaxOutputTokensChange={vi.fn()}
        promptCacheRouting="auto"
        onPromptCacheRoutingChange={vi.fn()}
        catalogModels={[{ model: "gpt-5.6-sol", useResponsesLite: true }]}
        onCatalogModelsChange={onCatalogModelsChange}
        speedTestEndpoints={[]}
        customUserAgent=""
        onCustomUserAgentChange={vi.fn()}
        localProxyHeadersOverride=""
        onLocalProxyHeadersOverrideChange={vi.fn()}
        localProxyBodyOverride=""
        onLocalProxyBodyOverrideChange={vi.fn()}
      />
    </TestForm>,
  );
  return onCatalogModelsChange;
}

describe("CodexFormFields Responses Lite control", () => {
  it.each(["openai_chat", "anthropic"] as const)(
    "disables and clears the override for %s providers",
    async (apiFormat) => {
      const onCatalogModelsChange = renderFields(apiFormat);

      const control = screen.getByRole("combobox", {
        name: "Responses Lite",
      });
      expect(control).toBeDisabled();
      expect(control).toHaveTextContent("关闭");
      expect(control).toHaveAttribute(
        "title",
        "Responses Lite 仅支持原生 Responses 上游；Chat 与 Anthropic 转换格式固定关闭。",
      );

      await waitFor(() => {
        expect(onCatalogModelsChange).toHaveBeenCalledWith([
          expect.objectContaining({
            model: "gpt-5.6-sol",
            useResponsesLite: false,
          }),
        ]);
      });
    },
  );

  it("keeps the control and enabled override for native Responses", () => {
    const onCatalogModelsChange = renderFields("openai_responses");

    const control = screen.getByRole("combobox", {
      name: "Responses Lite",
    });
    expect(control).not.toBeDisabled();
    expect(control).toHaveTextContent("开启");
    expect(onCatalogModelsChange).not.toHaveBeenCalled();
  });
});
