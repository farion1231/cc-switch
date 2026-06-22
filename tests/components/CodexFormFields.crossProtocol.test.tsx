import { render, screen } from "@testing-library/react";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { Form } from "@/components/ui/form";
import { CodexFormFields } from "@/components/providers/forms/CodexFormFields";

const noop = vi.fn();

function renderCodexForm(apiFormat: "anthropic" | "gemini_native") {
  const Wrapper = () => {
    const form = useForm();

    return (
      <Form {...form}>
        <CodexFormFields
          codexApiKey=""
          onApiKeyChange={noop}
          category="third_party"
          shouldShowApiKeyLink
          websiteUrl="https://example.com"
          shouldShowSpeedTest
          codexBaseUrl="https://generativelanguage.googleapis.com"
          onBaseUrlChange={noop}
          isFullUrl={false}
          onFullUrlChange={noop}
          isEndpointModalOpen={false}
          onEndpointModalToggle={noop}
          autoSelect={false}
          onAutoSelectChange={noop}
          apiFormat={apiFormat}
          onApiFormatChange={noop}
          speedTestEndpoints={[]}
          customUserAgent=""
          onCustomUserAgentChange={noop}
        />
      </Form>
    );
  };

  return render(<Wrapper />);
}

describe("CodexFormFields cross-protocol UX", () => {
  it("shows the Gemini bridge warning for Codex via Gemini Native presets", () => {
    renderCodexForm("gemini_native");

    expect(
      screen.getByText("Codex → Gemini Native bridge"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/AI Studio API key.*oauth_creds\.json/i),
    ).toBeInTheDocument();
  });

  it("shows the Anthropic bridge warning for Codex via Claude providers", () => {
    renderCodexForm("anthropic");

    expect(screen.getByText("Codex → Anthropic bridge")).toBeInTheDocument();
    expect(
      screen.getByText(/converted to Anthropic messages/i),
    ).toBeInTheDocument();
  });
});
