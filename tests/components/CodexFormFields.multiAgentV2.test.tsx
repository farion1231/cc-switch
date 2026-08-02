import { fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { CodexFormFields } from "@/components/providers/forms/CodexFormFields";
import { Form } from "@/components/ui/form";

type Props = ComponentProps<typeof CodexFormFields>;

function renderFields(overrides: Partial<Props> = {}) {
  const props: Props = {
    appId: "codex",
    codexApiKey: "",
    onApiKeyChange: vi.fn(),
    category: "third_party",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    shouldShowSpeedTest: true,
    codexBaseUrl: "https://example.com/v1",
    onBaseUrlChange: vi.fn(),
    isFullUrl: false,
    onFullUrlChange: vi.fn(),
    isEndpointModalOpen: false,
    onEndpointModalToggle: vi.fn(),
    autoSelect: false,
    onAutoSelectChange: vi.fn(),
    apiFormat: "openai_chat",
    onApiFormatChange: vi.fn(),
    multiAgentV2Enabled: false,
    onMultiAgentV2EnabledChange: vi.fn(),
    multiAgentV2Available: true,
    anthropicAuthField: "ANTHROPIC_AUTH_TOKEN",
    onAnthropicAuthFieldChange: vi.fn(),
    impersonateClaudeCode: false,
    onImpersonateClaudeCodeChange: vi.fn(),
    maxOutputTokens: "",
    onMaxOutputTokensChange: vi.fn(),
    promptCacheRouting: "auto",
    onPromptCacheRoutingChange: vi.fn(),
    catalogModels: [{ model: "LongCat-2.0" }],
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

  function FormHarness() {
    const form = useForm();
    return (
      <Form {...form}>
        <CodexFormFields {...props} />
      </Form>
    );
  }

  render(<FormHarness />);
  return props;
}

function openAdvancedOptions() {
  const button = screen.getByRole("button", {
    name: /高级选项|advanced options/i,
  });
  if (button.getAttribute("aria-expanded") !== "true") {
    fireEvent.click(button);
  }
}

describe("CodexFormFields multi-agent V2 capability", () => {
  it("shows the V2 switch for a third-party Chat provider with model mappings", () => {
    renderFields();
    openAdvancedOptions();

    const toggle = screen.getByRole("switch", {
      name: /V2 子智能体|V2 subagents/i,
    });
    expect(toggle).toBeEnabled();
  });

  it("does not show the V2 switch for Official providers", () => {
    renderFields({ category: "official" });

    expect(
      screen.queryByRole("switch", { name: /V2 子智能体|V2 subagents/i }),
    ).toBeNull();
  });

  it.each(["openai_responses", "anthropic"] as const)(
    "does not show the V2 switch for %s upstream format",
    (apiFormat) => {
      renderFields({ apiFormat });
      openAdvancedOptions();

      expect(
        screen.queryByRole("switch", {
          name: /V2 子智能体|V2 subagents/i,
        }),
      ).toBeNull();
    },
  );

  it("disables the switch when there are no model mappings", () => {
    renderFields({ catalogModels: [], multiAgentV2Available: false });
    openAdvancedOptions();

    expect(
      screen.getByRole("switch", { name: /V2 子智能体|V2 subagents/i }),
    ).toBeDisabled();
    expect(screen.getByText(/请先添加模型映射/)).toBeInTheDocument();
  });
});
