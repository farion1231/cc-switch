import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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

const feedback = vi.hoisted(() => ({
  success: vi.fn(),
  info: vi.fn(),
  error: vi.fn(),
}));
vi.mock("@/lib/toast", () => ({ toast: feedback }));

const catalogButton = "provider.form.agy.readModelCatalog";
const keyHint = "provider.form.agy.modelCatalogKeyHint";
const emptyKeyHint = "provider.form.agy.modelCatalogEmptyKeyHint";
const loginHint = "provider.form.agy.modelCatalogLoginHint";

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

beforeEach(() => {
  // cmdk scrolls its selected option; jsdom does not implement this browser API.
  Element.prototype.scrollIntoView ??= vi.fn();
  vi.clearAllMocks();
  vi.spyOn(console, "warn").mockImplementation(() => {});
  api.fetchModelsForConfig.mockResolvedValue([{ id: "gemini-3.8-flash" }]);
  api.getAgyModels.mockResolvedValue([{ id: "Gemini 3.8 Flash (Low)" }]);
});

afterEach(() => vi.restoreAllMocks());

describe("Antigravity model catalog", () => {
  it.each(["", " https://relay.example/gemini "])(
    "identifies the catalog as unverified for a key with endpoint %j",
    async (baseUrl) => {
      render(<Harness baseUrl={baseUrl} />);
      expect(screen.getByText(keyHint)).toBeVisible();
      expect(screen.queryByText(loginHint)).not.toBeInTheDocument();
      expect(
        screen.queryByRole("button", { name: "providerForm.fetchModels" }),
      ).not.toBeInTheDocument();

      fireEvent.click(screen.getByRole("button", { name: catalogButton }));
      await waitFor(() =>
        expect(api.getAgyModels).toHaveBeenCalledWith({
          apiKey: "fixture-key",
          baseUrl: baseUrl.trim(),
        }),
      );
      await waitFor(() =>
        expect(feedback.success).toHaveBeenCalledWith(
          "provider.form.agy.modelCatalogSuccess",
        ),
      );
      expect(screen.getByText(keyHint)).toBeVisible();
      expect(api.fetchModelsForConfig).not.toHaveBeenCalled();
      expect(api.showFetchModelsError).not.toHaveBeenCalled();
      expect(feedback.success).toHaveBeenCalledTimes(1);
    },
  );

  it.each(["", "  \t "])(
    "describes native sign-in for an empty or whitespace-only key %j",
    async (apiKey) => {
      render(<Harness apiKey={apiKey} />);
      expect(screen.getByText(loginHint)).toBeVisible();
      expect(screen.queryByText(keyHint)).not.toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: catalogButton }));
      await waitFor(() => expect(api.getAgyModels).toHaveBeenCalledOnce());
      expect(api.fetchModelsForConfig).not.toHaveBeenCalled();
      expect(screen.getByText(loginHint)).toBeVisible();
    },
  );

  it.each(["fixture-key", ""])(
    "selects the native display name, not an API ID or CLI alias (key %j)",
    async (apiKey) => {
      const onModelChange = vi.fn();
      render(<Harness apiKey={apiKey} onModelChange={onModelChange} />);
      fireEvent.click(screen.getByRole("button", { name: catalogButton }));
      fireEvent.click(
        await screen.findByRole("button", { name: "Select model" }),
      );
      fireEvent.click(
        await screen.findByRole("option", { name: "Gemini 3.8 Flash (Low)" }),
      );
      expect(onModelChange).toHaveBeenCalledTimes(1);
      expect(onModelChange).toHaveBeenCalledWith("Gemini 3.8 Flash (Low)");
    },
  );

  it("does not replace an existing model when reading the catalog", async () => {
    const onModelChange = vi.fn();
    render(
      <Harness
        model="Gemini 4 Hydrogen (High)"
        onModelChange={onModelChange}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: catalogButton }));
    await screen.findByRole("button", { name: "Select model" });
    expect(screen.getByRole("textbox")).toHaveValue("Gemini 4 Hydrogen (High)");
    expect(onModelChange).not.toHaveBeenCalled();
    expect(screen.getByText(keyHint)).toBeVisible();
  });

  it("reports an empty runtime catalog without a provider-availability claim", async () => {
    api.getAgyModels.mockResolvedValueOnce([]);
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: catalogButton }));
    await waitFor(() =>
      expect(feedback.info).toHaveBeenCalledWith(
        "provider.form.agy.modelCatalogEmpty",
      ),
    );
    expect(feedback.success).not.toHaveBeenCalled();
    expect(feedback.error).not.toHaveBeenCalled();
    expect(screen.getByText(keyHint)).toBeVisible();
  });

  it.each(["HTTP 401: CLI failed", "Antigravity model discovery timed out"])(
    "does not interpret a catalog error as a provider validation result: %s",
    async (error) => {
      api.getAgyModels.mockRejectedValueOnce(error);
      render(<Harness />);
      fireEvent.click(screen.getByRole("button", { name: catalogButton }));
      await waitFor(() =>
        expect(feedback.error).toHaveBeenCalledWith(
          "provider.form.agy.modelCatalogError",
        ),
      );
      expect(api.showFetchModelsError).not.toHaveBeenCalled();
      expect(feedback.success).not.toHaveBeenCalled();
      expect(screen.getByRole("button", { name: catalogButton })).toBeEnabled();
      expect(screen.getByText(keyHint)).toBeVisible();
    },
  );

  it("updates the provenance hint when the authentication mode changes", () => {
    const { rerender } = render(<Harness />);
    expect(screen.getByText(keyHint)).toBeVisible();
    rerender(<Harness apiKey="   " />);
    expect(screen.getByText(loginHint)).toBeVisible();
    expect(screen.queryByText(keyHint)).not.toBeInTheDocument();
  });

  it("distinguishes an unfilled API key field from native sign-in mode", () => {
    const { rerender } = render(<Harness shouldShowApiKey apiKey="  " />);
    expect(screen.getByText(emptyKeyHint)).toBeVisible();
    expect(screen.queryByText(loginHint)).not.toBeInTheDocument();
    expect(screen.queryByText(keyHint)).not.toBeInTheDocument();
    rerender(<Harness shouldShowApiKey apiKey="fixture-key" />);
    expect(screen.getByText(keyHint)).toBeVisible();
    expect(screen.queryByText(emptyKeyHint)).not.toBeInTheDocument();
  });
});

describe("Gemini CLI model discovery", () => {
  // #8546：Gemini CLI 只说原生协议，取模型要走 v1beta + x-goog-api-key
  it("fetches models with the Gemini native format and keeps button and feedback unchanged", async () => {
    render(<Harness appId="gemini" baseUrl=" https://relay.example/gemini " />);
    expect(screen.queryByText(keyHint)).not.toBeInTheDocument();
    expect(screen.queryByText(emptyKeyHint)).not.toBeInTheDocument();
    expect(screen.queryByText(loginHint)).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: catalogButton }),
    ).not.toBeInTheDocument();
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
    await waitFor(() =>
      expect(feedback.success).toHaveBeenCalledWith(
        "providerForm.fetchModelsSuccess",
      ),
    );
    expect(api.getAgyModels).not.toHaveBeenCalled();
  });

  it.each([
    ["", ""],
    ["fixture-key", ""],
    ["", "https://relay.example/gemini"],
  ])("keeps preflight validation (key %j, endpoint %j)", (apiKey, baseUrl) => {
    render(<Harness appId="gemini" apiKey={apiKey} baseUrl={baseUrl} />);
    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    expect(api.showFetchModelsError).toHaveBeenCalledWith(
      null,
      expect.any(Function),
      { hasApiKey: !!apiKey, hasBaseUrl: !!baseUrl },
    );
    expect(api.fetchModelsForConfig).not.toHaveBeenCalled();
    expect(api.getAgyModels).not.toHaveBeenCalled();
  });

  it("keeps provider error handling unchanged", async () => {
    const error = "HTTP 401: unauthorized";
    api.fetchModelsForConfig.mockRejectedValueOnce(error);
    render(<Harness appId="gemini" baseUrl="https://relay.example/gemini" />);
    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await waitFor(() =>
      expect(api.showFetchModelsError).toHaveBeenCalledWith(
        error,
        expect.any(Function),
      ),
    );
    expect(feedback.error).not.toHaveBeenCalled();
    expect(feedback.success).not.toHaveBeenCalled();
  });

  it("keeps the provider empty-list feedback unchanged", async () => {
    api.fetchModelsForConfig.mockResolvedValueOnce([]);
    render(<Harness appId="gemini" baseUrl="https://relay.example/gemini" />);
    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await waitFor(() =>
      expect(feedback.info).toHaveBeenCalledWith(
        "providerForm.fetchModelsEmpty",
      ),
    );
    expect(feedback.success).not.toHaveBeenCalled();
  });
});
