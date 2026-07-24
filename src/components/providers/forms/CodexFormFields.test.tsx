import { render, screen } from "@testing-library/react";
import type { PropsWithChildren } from "react";
import { FormProvider, useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import type { CodexApiFormat } from "@/types";
import { CodexFormFields } from "./CodexFormFields";

function TestForm({ children }: PropsWithChildren) {
  const form = useForm();
  return <FormProvider {...form}>{children}</FormProvider>;
}

function renderFields(
  apiFormat: CodexApiFormat,
  useResponsesLite: boolean | null = true,
) {
  const onCatalogModelsChange = vi.fn();
  const catalogModels = [
    {
      model: "gpt-5.6-sol",
      ...(useResponsesLite === null ? {} : { useResponsesLite }),
    },
  ];
  const fields = (format: CodexApiFormat) => (
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
        apiFormat={format}
        onApiFormatChange={vi.fn()}
        anthropicAuthField="ANTHROPIC_AUTH_TOKEN"
        onAnthropicAuthFieldChange={vi.fn()}
        impersonateClaudeCode={false}
        onImpersonateClaudeCodeChange={vi.fn()}
        maxOutputTokens=""
        onMaxOutputTokensChange={vi.fn()}
        promptCacheRouting="auto"
        onPromptCacheRoutingChange={vi.fn()}
        catalogModels={catalogModels}
        onCatalogModelsChange={onCatalogModelsChange}
        speedTestEndpoints={[]}
        customUserAgent=""
        onCustomUserAgentChange={vi.fn()}
        localProxyHeadersOverride=""
        onLocalProxyHeadersOverrideChange={vi.fn()}
        localProxyBodyOverride=""
        onLocalProxyBodyOverrideChange={vi.fn()}
      />
    </TestForm>
  );
  const view = render(fields(apiFormat));
  return {
    onCatalogModelsChange,
    rerenderWithFormat: (format: CodexApiFormat) =>
      view.rerender(fields(format)),
  };
}

describe("CodexFormFields Responses Lite control", () => {
  it.each(["openai_chat", "anthropic"] as const)(
    "disables the control without clearing the override for %s providers",
    (apiFormat) => {
      const { onCatalogModelsChange } = renderFields(apiFormat);

      const control = screen.getByRole("combobox", {
        name: "Responses Lite",
      });
      expect(control).toBeDisabled();
      expect(control).toHaveTextContent("关闭");
      expect(control).toHaveAttribute(
        "title",
        "Responses Lite 仅支持原生 Responses 上游；Chat 与 Anthropic 转换格式固定关闭。",
      );

      expect(onCatalogModelsChange).not.toHaveBeenCalled();
    },
  );

  it("keeps the control and enabled override for native Responses", () => {
    const { onCatalogModelsChange } = renderFields("openai_responses");

    const control = screen.getByRole("combobox", {
      name: "Responses Lite",
    });
    expect(control).not.toBeDisabled();
    expect(control).toHaveTextContent("开启");
    expect(onCatalogModelsChange).not.toHaveBeenCalled();
  });

  it.each([
    ["enabled", true, "开启"],
    ["auto", null, "自动"],
  ] as const)(
    "restores the stored %s choice after switching back to native Responses",
    (_label, useResponsesLite, expectedLabel) => {
      const { onCatalogModelsChange, rerenderWithFormat } = renderFields(
        "openai_chat",
        useResponsesLite,
      );

      expect(
        screen.getByRole("combobox", { name: "Responses Lite" }),
      ).toHaveTextContent("关闭");

      rerenderWithFormat("openai_responses");

      const control = screen.getByRole("combobox", {
        name: "Responses Lite",
      });
      expect(control).not.toBeDisabled();
      expect(control).toHaveTextContent(expectedLabel);
      expect(onCatalogModelsChange).not.toHaveBeenCalled();
    },
  );
});
