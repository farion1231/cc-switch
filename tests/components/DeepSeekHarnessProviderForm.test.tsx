import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { ProviderForm, type ProviderFormValues } from "@/components/providers/forms/ProviderForm";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn() } }));
vi.mock("@/lib/api/model-fetch", () => ({
  fetchModelsForConfig: vi.fn().mockResolvedValue([]),
  showFetchModelsError: vi.fn(),
}));

function renderForm(
  onSubmit: (values: ProviderFormValues) => void,
  initialData?: Parameters<typeof ProviderForm>[0]["initialData"],
) {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ProviderForm
        appId="deepseek-harness"
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        initialData={initialData}
      />
    </QueryClientProvider>,
  );
}

describe("DeepSeek Harness provider form", () => {
  it("creates a custom native provider with Codex-style structured fields", async () => {
    const onSubmit = vi.fn();
    renderForm(onSubmit);

    expect(screen.getByLabelText("Provider ID")).toHaveValue("custom-dsh");
    expect(screen.getByLabelText("API Key")).toBeInTheDocument();
    expect(screen.getByLabelText("Base URL")).toBeInTheDocument();
    expect(screen.getByText("API Format")).toBeInTheDocument();
    expect(screen.getByText("Model Catalog")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("API Key"), { target: { value: "secret" } });
    fireEvent.change(screen.getByLabelText("Base URL"), {
      target: { value: "https://gateway.example/v1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.add" }));
    fireEvent.change(screen.getAllByPlaceholderText("model-id")[0], {
      target: { value: "glm-5.3" },
    });
    fireEvent.change(screen.getByLabelText("Default Model"), {
      target: { value: "glm-5.3" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await vi.waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const values = onSubmit.mock.calls[0][0] as ProviderFormValues;
    expect(values.providerKey).toBe("custom-dsh");
    expect(values.meta?.providerType).toBe("dsh_pi_ai");
    expect(values.meta?.dshCurrentModel).toBe("glm-5.3");
    expect(JSON.parse(values.settingsConfig)).toMatchObject({
      baseURL: "https://gateway.example/v1",
      apiKeyEnv: "CUSTOM_DSH_API_KEY",
      models: [{ id: "glm-5.3" }],
    });
  });

  it("loads an imported native provider for editing", () => {
    renderForm(vi.fn(), {
      name: "Company Gateway",
      category: "custom",
      settingsConfig: {
        displayName: "Company Gateway",
        api: "openai-completions",
        baseURL: "https://company.example/v1",
        apiKeyEnv: "COMPANY_API_KEY",
        apiKey: "secret",
        models: [{ id: "deepseek-v4-pro", name: "DeepSeek V4 Pro" }],
      },
      meta: { providerType: "dsh_pi_ai", dshCurrentModel: "deepseek-v4-pro" },
    });

    expect(screen.getByLabelText("Base URL")).toHaveValue("https://company.example/v1");
    expect(screen.getByLabelText("Default Model")).toHaveValue("deepseek-v4-pro");
    expect(screen.getByPlaceholderText("model-id")).toHaveValue(
      "deepseek-v4-pro",
    );
  });
});
