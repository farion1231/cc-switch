import { render, screen, waitFor } from "@testing-library/react";
import type { PropsWithChildren } from "react";
import { FormProvider, useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { CodexFormFields } from "./CodexFormFields";

function TestForm({ children }: PropsWithChildren) {
  const form = useForm();
  return <FormProvider {...form}>{children}</FormProvider>;
}

describe("CodexFormFields Responses Lite control", () => {
  it("disables and clears the override for Anthropic providers", async () => {
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
          apiFormat="anthropic"
          onApiFormatChange={vi.fn()}
          anthropicAuthField="ANTHROPIC_AUTH_TOKEN"
          onAnthropicAuthFieldChange={vi.fn()}
          impersonateClaudeCode={false}
          onImpersonateClaudeCodeChange={vi.fn()}
          maxOutputTokens=""
          onMaxOutputTokensChange={vi.fn()}
          promptCacheRouting="auto"
          onPromptCacheRoutingChange={vi.fn()}
          catalogModels={[{ model: "claude-sonnet", useResponsesLite: true }]}
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

    const control = screen.getByRole("combobox", {
      name: "Responses Lite",
    });
    expect(control).toBeDisabled();
    expect(control).toHaveTextContent("关闭");
    expect(control).toHaveAttribute(
      "title",
      "Anthropic Messages 转换不支持 Responses Lite，因此固定关闭。",
    );

    await waitFor(() => {
      expect(onCatalogModelsChange).toHaveBeenCalledWith([
        expect.objectContaining({
          model: "claude-sonnet",
          useResponsesLite: false,
        }),
      ]);
    });
  });
});
