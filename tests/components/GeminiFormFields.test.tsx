import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { GeminiFormFields } from "@/components/providers/forms/GeminiFormFields";

const baseProps = {
  shouldShowApiKey: true,
  apiKey: "",
  onApiKeyChange: vi.fn(),
  category: "official" as const,
  shouldShowApiKeyLink: false,
  websiteUrl: "https://example.com",
  isPartner: false,
  shouldShowSpeedTest: false,
  baseUrl: "https://aiplatform.googleapis.com/v1",
  onBaseUrlChange: vi.fn(),
  isEndpointModalOpen: false,
  onEndpointModalToggle: vi.fn(),
  onCustomEndpointsChange: vi.fn(),
  autoSelect: true,
  onAutoSelectChange: vi.fn(),
  shouldShowModelField: false,
  model: "",
  onModelChange: vi.fn(),
  speedTestEndpoints: [],
};

describe("GeminiFormFields", () => {
  it("google-vertex-fast preset should allow API key input in official category", () => {
    const onApiKeyChange = vi.fn();
    render(
      <GeminiFormFields
        {...baseProps}
        onApiKeyChange={onApiKeyChange}
        partnerPromotionKey="google-vertex-fast"
      />,
    );

    const input = screen.getByLabelText("API Key");
    expect(input).not.toBeDisabled();

    fireEvent.change(input, { target: { value: "vertex-key" } });
    expect(onApiKeyChange).toHaveBeenCalledWith("vertex-key");
  });

  it("google-official preset should hide API key input", () => {
    render(
      <GeminiFormFields {...baseProps} partnerPromotionKey="google-official" />,
    );

    expect(screen.queryByLabelText("API Key")).toBeNull();
  });
});

