import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentProps, PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { DeepSeekHarnessFormFields } from "@/components/providers/forms/DeepSeekHarnessFormFields";
import { Form } from "@/components/ui/form";
import { fetchModelsForConfig } from "@/lib/api/model-fetch";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), info: vi.fn(), error: vi.fn(), warning: vi.fn() },
}));

vi.mock("@/lib/api/model-fetch", () => ({
  fetchModelsForConfig: vi.fn(),
  showFetchModelsError: vi.fn(),
}));

type DshFieldsProps = ComponentProps<typeof DeepSeekHarnessFormFields>;

const FormShell = ({ children }: PropsWithChildren) => {
  const form = useForm();
  return <Form {...form}>{children}</Form>;
};

const renderFields = (overrides: Partial<DshFieldsProps> = {}) => {
  const props: DshFieldsProps = {
    isOfficial: false,
    apiKey: "sk-test",
    onApiKeyChange: vi.fn(),
    apiKeyEnv: "CUSTOM_DSH_API_KEY",
    onApiKeyEnvChange: vi.fn(),
    baseUrl: "https://api.example.com/v1",
    onBaseUrlChange: vi.fn(),
    api: "openai-completions",
    onApiChange: vi.fn(),
    models: [{ id: "deepseek-v4-pro", name: "DeepSeek V4 Pro" }],
    onModelsChange: vi.fn(),
    defaultModel: "deepseek-v4-pro",
    onDefaultModelChange: vi.fn(),
    category: "custom",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    ...overrides,
  };

  return {
    props,
    ...render(
      <FormShell>
        <DeepSeekHarnessFormFields {...props} />
      </FormShell>,
    ),
  };
};

describe("DeepSeekHarnessFormFields", () => {
  it("shows credential ref and API format for custom providers", () => {
    renderFields();

    expect(screen.getByLabelText("Credential Ref")).toHaveValue(
      "CUSTOM_DSH_API_KEY",
    );
    expect(screen.getByText("API Format")).toBeInTheDocument();
    expect(screen.getByText("Model Catalog")).toBeInTheDocument();
    expect(screen.getByLabelText("Default Model")).toHaveValue(
      "deepseek-v4-pro",
    );
  });

  it("hides credential ref and API format for official providers", () => {
    renderFields({
      isOfficial: true,
      category: "official",
      apiKeyEnv: "DEEPSEEK_API_KEY",
    });

    expect(screen.queryByLabelText("Credential Ref")).not.toBeInTheDocument();
    expect(screen.queryByText("API Format")).not.toBeInTheDocument();
    expect(screen.getByText("Model Catalog")).toBeInTheDocument();
    expect(screen.getByLabelText("API Key")).toBeDisabled();
  });

  it("updates a model name", () => {
    const onModelsChange = vi.fn();
    renderFields({ onModelsChange });

    fireEvent.change(screen.getByDisplayValue("DeepSeek V4 Pro"), {
      target: { value: "V4 Pro" },
    });

    expect(onModelsChange).toHaveBeenCalledWith([
      { id: "deepseek-v4-pro", name: "V4 Pro" },
    ]);
  });

  it("updates the optional context window", () => {
    const onModelsChange = vi.fn();
    renderFields({ onModelsChange });

    fireEvent.change(screen.getByLabelText("Context Window"), {
      target: { value: "128000" },
    });

    expect(onModelsChange).toHaveBeenCalledWith([
      { id: "deepseek-v4-pro", name: "DeepSeek V4 Pro", contextWindow: 128000 },
    ]);
  });

  it("adds a model row", () => {
    const onModelsChange = vi.fn();
    renderFields({ models: [], onModelsChange });

    fireEvent.click(screen.getByRole("button", { name: "Add" }));

    expect(onModelsChange).toHaveBeenCalledWith([{ id: "", name: undefined }]);
  });

  it("removes a model row", () => {
    const onModelsChange = vi.fn();
    renderFields({ onModelsChange });

    fireEvent.click(screen.getByRole("button", { name: "Remove" }));

    expect(onModelsChange).toHaveBeenCalledWith([]);
  });

  it("uppercases the credential ref", () => {
    const onApiKeyEnvChange = vi.fn();
    renderFields({ onApiKeyEnvChange });

    fireEvent.change(screen.getByLabelText("Credential Ref"), {
      target: { value: "my_key" },
    });

    expect(onApiKeyEnvChange).toHaveBeenCalledWith("MY_KEY");
  });

  it("updates the default model", () => {
    const onDefaultModelChange = vi.fn();
    renderFields({ onDefaultModelChange });

    fireEvent.change(screen.getByLabelText("Default Model"), {
      target: { value: "glm-5" },
    });

    expect(onDefaultModelChange).toHaveBeenCalledWith("glm-5");
  });

  it("fetches models and appends a selected model", async () => {
    Element.prototype.scrollIntoView = vi.fn();
    vi.mocked(fetchModelsForConfig).mockResolvedValue([
      { id: "gpt-4o", ownedBy: "openai" },
    ]);
    const onModelsChange = vi.fn();
    renderFields({ models: [], onModelsChange });

    fireEvent.click(screen.getByRole("button", { name: "Fetch Models" }));

    await waitFor(() =>
      expect(fetchModelsForConfig).toHaveBeenCalledWith(
        "https://api.example.com/v1",
        "sk-test",
      ),
    );

    fireEvent.click(
      await screen.findByRole("button", { name: "Select model" }),
    );
    fireEvent.click(await screen.findByRole("option", { name: "gpt-4o" }));

    expect(onModelsChange).toHaveBeenCalledWith([
      { id: "gpt-4o", name: "gpt-4o" },
    ]);
  });
});
