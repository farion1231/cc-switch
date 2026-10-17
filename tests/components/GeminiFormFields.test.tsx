import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useForm } from "react-hook-form";
import { Form } from "@/components/ui/form";
import { GeminiFormFields } from "@/components/providers/forms/GeminiFormFields";
import type { ComponentProps } from "react";

const api = vi.hoisted(() => ({
  fetchModelsForConfig: vi.fn(),
  getAgyModels: vi.fn(),
  showFetchModelsError: vi.fn(),
}));
vi.mock("@/lib/api/model-fetch", () => api);

function Harness(props: Partial<ComponentProps<typeof GeminiFormFields>>) {
  const form = useForm();
  return (
    <Form {...form}>
      <GeminiFormFields
        appId="agy"
        shouldShowApiKey={false}
        apiKey="fixture-key"
        onApiKeyChange={vi.fn()}
        shouldShowApiKeyLink={false}
        websiteUrl=""
        shouldShowSpeedTest={false}
        baseUrl=""
        onBaseUrlChange={vi.fn()}
        isEndpointModalOpen={false}
        onEndpointModalToggle={vi.fn()}
        onCustomEndpointsChange={vi.fn()}
        autoSelect={false}
        onAutoSelectChange={vi.fn()}
        shouldShowModelField
        model=""
        onModelChange={vi.fn()}
        speedTestEndpoints={[]}
        {...props}
      />
    </Form>
  );
}

describe("Antigravity model discovery", () => {
  beforeEach(() => {
    // cmdk scrolls its selected option; jsdom does not implement this browser API.
    Element.prototype.scrollIntoView ??= vi.fn();
    vi.clearAllMocks();
    api.fetchModelsForConfig.mockResolvedValue([{ id: "gemini-3.8-flash" }]);
    api.getAgyModels.mockResolvedValue([{ id: "Gemini 3.8 Flash (Low)" }]);
  });

  it("uses Google's default API when a key has no endpoint override", async () => {
    render(<Harness />);
    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await waitFor(() =>
      expect(api.fetchModelsForConfig).toHaveBeenCalledWith(
        "https://generativelanguage.googleapis.com",
        "fixture-key",
        false,
        undefined,
        undefined,
        { apiFormat: "google-generative-ai" },
      ),
    );
    expect(api.getAgyModels).not.toHaveBeenCalled();
    expect(api.showFetchModelsError).not.toHaveBeenCalled();
  });

  it("keeps a custom Gemini endpoint", async () => {
    render(<Harness baseUrl="https://relay.example/gemini" />);
    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await waitFor(() =>
      expect(api.fetchModelsForConfig).toHaveBeenCalledWith(
        "https://relay.example/gemini",
        "fixture-key",
        false,
        undefined,
        undefined,
        { apiFormat: "google-generative-ai" },
      ),
    );
  });

  it("uses native discovery for official login without a key", async () => {
    render(<Harness apiKey="" />);
    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await waitFor(() => expect(api.getAgyModels).toHaveBeenCalledOnce());
    expect(api.fetchModelsForConfig).not.toHaveBeenCalled();
  });
  it("selects the native preference name rather than a CLI alias", async () => {
    const onModelChange = vi.fn();
    render(<Harness apiKey="" onModelChange={onModelChange} />);
    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Select model",
      }),
    );
    fireEvent.click(
      await screen.findByRole("option", { name: "Gemini 3.8 Flash (Low)" }),
    );
    expect(onModelChange).toHaveBeenCalledWith("Gemini 3.8 Flash (Low)");
  });
});
