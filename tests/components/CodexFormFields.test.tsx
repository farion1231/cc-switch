import { fireEvent, render, screen } from "@testing-library/react";
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

const renderForm = (overrides: Partial<CodexFormFieldsProps> = {}) => {
  const props: CodexFormFieldsProps = {
    appId: "codex",
    codexApiKey: "test-key",
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
    codexModel: "gpt-5.6",
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
    codexLiveEnabled: true,
    onCodexLiveEnabledChange: vi.fn(),
    codexLiveCreateEndpoint: "live",
    onCodexLiveCreateEndpointChange: vi.fn(),
    codexLiveSidebandEndpoint: "live/{call_id}",
    onCodexLiveSidebandEndpointChange: vi.fn(),
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
    ...render(
      <FormShell>
        <CodexFormFields {...props} />
      </FormShell>,
    ),
  };
};

describe("CodexFormFields Live capability", () => {
  it("shows explicit Live opt-in and relative endpoint defaults", () => {
    renderForm();

    expect(
      screen.getByTestId("codex-live-provider-config"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("switch", { name: "支持 Codex Live 语音" }),
    ).toBeChecked();
    expect(screen.getByLabelText("Live 创建端点")).toHaveValue("live");
    expect(screen.getByLabelText("Sideband 端点模板")).toHaveValue(
      "live/{call_id}",
    );
  });

  it("reports endpoint edits without changing provider routing", () => {
    const onSidebandChange = vi.fn();
    renderForm({ onCodexLiveSidebandEndpointChange: onSidebandChange });

    fireEvent.change(screen.getByLabelText("Sideband 端点模板"), {
      target: { value: "voice/{call_id}" },
    });

    expect(onSidebandChange).toHaveBeenCalledWith("voice/{call_id}");
  });

  it("disables Live when the provider uses a full request URL", () => {
    renderForm({ isFullUrl: true, codexLiveEnabled: true });

    const liveSwitch = screen.getByRole("switch", {
      name: "支持 Codex Live 语音",
    });
    expect(liveSwitch).toBeDisabled();
    expect(liveSwitch).not.toBeChecked();
    expect(
      screen.getByText(
        "Codex Live requires a base URL. Disable Full URL mode before enabling Live.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("Live 创建端点")).not.toBeInTheDocument();
  });
});
