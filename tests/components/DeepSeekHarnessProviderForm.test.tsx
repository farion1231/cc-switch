import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  ProviderForm,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import { createTestQueryClient } from "../utils/testQueryClient";

const dshState = vi.hoisted(() => ({
  providerIds: [] as string[],
  credentialRefs: [] as string[],
  isLoading: false,
}));

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

vi.mock("@/lib/api/model-fetch", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api/model-fetch")>();
  return {
    ...actual,
    fetchModelsForConfig: vi.fn().mockResolvedValue([]),
    showFetchModelsError: vi.fn(),
  };
});

vi.mock("@/lib/query/dsh", () => ({
  useDshCurrentState: () => ({
    data: {
      providerIds: dshState.providerIds,
      currentProviderId: null,
      currentModel: null,
      credentialRefs: dshState.credentialRefs,
    },
    isLoading: dshState.isLoading,
  }),
  invalidateDshProviderCaches: vi.fn(),
  dshKeys: { all: ["dsh"], currentState: ["dsh", "currentState"] },
}));

// CodeMirror is heavyweight in jsdom; expose it as a plain textarea instead.
vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
    placeholder,
  }: {
    value: string;
    onChange: (value: string) => void;
    placeholder?: string;
  }) => (
    <textarea
      aria-label="dsh-settings-config"
      value={value}
      placeholder={placeholder}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

function renderForm(
  onSubmit: (values: ProviderFormValues) => void,
  initialData?: Parameters<typeof ProviderForm>[0]["initialData"],
  providerId?: string,
) {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ProviderForm
        appId="deepseek-harness"
        providerId={providerId}
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        initialData={initialData}
      />
    </QueryClientProvider>,
  );
}

describe("DeepSeek Harness provider form (shared shell)", () => {
  beforeEach(() => {
    dshState.providerIds = [];
    dshState.credentialRefs = [];
    dshState.isLoading = false;
  });

  it("isolates repeated presets and synchronizes late native refs before saving", async () => {
    const onSubmit = vi.fn();
    dshState.isLoading = true;
    const { rerender } = renderForm(onSubmit);
    fireEvent.click(screen.getByRole("button", { name: /OpenAI Compatible/ }));
    // Programmatic input simulates a filled draft while native refs are loading.
    fireEvent.change(screen.getByLabelText(/Provider Key/), {
      target: { value: "second-route" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    fireEvent.change(screen.getAllByPlaceholderText("Model ID")[0], {
      target: { value: "model" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(onSubmit).not.toHaveBeenCalled());

    dshState.isLoading = false;
    dshState.credentialRefs = ["OPENAI_API_KEY", "SECOND_ROUTE_API_KEY"];
    rerender(
      <QueryClientProvider client={createTestQueryClient()}>
        <ProviderForm
          appId="deepseek-harness"
          submitLabel="Save"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
        />
      </QueryClientProvider>,
    );
    const editor = screen.getByLabelText(
      "dsh-settings-config",
    ) as HTMLTextAreaElement;
    await waitFor(() =>
      expect(JSON.parse(editor.value).apiKeyEnv).toBe("SECOND_ROUTE_API_KEY_2"),
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig).apiKeyEnv).toBe(
      "SECOND_ROUTE_API_KEY_2",
    );

    fireEvent.click(screen.getByRole("button", { name: /OpenAI Compatible/ }));
    fireEvent.change(screen.getByLabelText(/Provider Key/), {
      target: { value: "third-route" },
    });
    expect(JSON.parse(editor.value).apiKeyEnv).toBe("THIRD_ROUTE_API_KEY");
  });

  it("reuses the shared shell and creates a custom pi-ai route", async () => {
    const onSubmit = vi.fn();
    const { container } = renderForm(onSubmit);

    // Shared provider shell fields.
    expect(container.querySelector('input[name="name"]')).toBeInTheDocument();
    expect(screen.getByLabelText("provider.name")).toBeInTheDocument();
    expect(screen.getByLabelText("provider.websiteUrl")).toBeInTheDocument();
    expect(screen.getByLabelText("provider.notes")).toBeInTheDocument();
    // Shared JSON editor (mocked as a textarea) instead of the old bespoke one.
    const jsonEditor = screen.getByLabelText(
      "dsh-settings-config",
    ) as HTMLTextAreaElement;
    expect(jsonEditor).toBeInTheDocument();
    expect(jsonEditor.value).toContain("Custom DSH");

    // DSH structured fields are rendered alongside the shared shell.
    expect(screen.getByLabelText(/Provider Key/)).toBeInTheDocument();
    expect(screen.getByLabelText("API Key")).toBeInTheDocument();
    expect(screen.getByLabelText("Base URL")).toBeInTheDocument();
    expect(screen.getByText("Model Catalog")).toBeInTheDocument();
    expect(screen.getByLabelText("Default Model")).toBeInTheDocument();

    fireEvent.change(container.querySelector('input[name="name"]')!, {
      target: { value: "Company Gateway" },
    });
    fireEvent.change(screen.getByLabelText(/Provider Key/), {
      target: { value: "company-gateway" },
    });
    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "secret" },
    });
    fireEvent.change(screen.getByLabelText("Base URL"), {
      target: { value: "https://gateway.example/v1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    fireEvent.change(screen.getAllByPlaceholderText("Model ID")[0], {
      target: { value: "glm-5.3" },
    });
    fireEvent.change(screen.getByLabelText("Default Model"), {
      target: { value: "glm-5.3" },
    });

    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const values = onSubmit.mock.calls[0][0] as ProviderFormValues;
    expect(values.providerKey).toBe("company-gateway");
    expect(values.meta?.providerType).toBe("dsh_pi_ai");
    expect(values.meta?.dshCurrentModel).toBe("glm-5.3");
    expect(JSON.parse(values.settingsConfig)).toMatchObject({
      // The name field follows into the native display name, and the
      // credential ref is derived from the provider key.
      displayName: "Company Gateway",
      api: "openai-completions",
      baseURL: "https://gateway.example/v1",
      apiKeyEnv: "COMPANY_GATEWAY_API_KEY",
      apiKey: "secret",
      models: [{ id: "glm-5.3" }],
    });
  });

  it("defaults to the custom preset without highlighting a preset in add mode", () => {
    renderForm(vi.fn());

    const customButton = screen.getByRole("button", {
      name: "providerPreset.custom",
    });
    expect(customButton.className).toContain("border-border-strong");

    const openAiButton = screen.getByRole("button", {
      name: /OpenAI Compatible/,
    });
    expect(openAiButton.className).not.toContain("border-border-strong");
  });

  it("locks the official route key and submits the dsh_deepseek shape", async () => {
    const onSubmit = vi.fn();
    renderForm(onSubmit);

    fireEvent.click(screen.getByRole("button", { name: /DeepSeek/ }));

    const providerKey = screen.getByLabelText(/Provider Key/);
    expect(providerKey).toHaveValue("deepseek-official");
    expect(providerKey).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const values = onSubmit.mock.calls[0][0] as ProviderFormValues;
    expect(values.providerKey).toBe("deepseek-official");
    expect(values.meta?.providerType).toBe("dsh_deepseek");
    const settings = JSON.parse(values.settingsConfig);
    // The desktop profile is the fixed write target; it must not leak into
    // the provider settings (the llm-deepseek schema has no profile field).
    expect(settings.profile).toBeUndefined();
    expect(settings.models.length).toBeGreaterThan(0);
  });

  it("keeps an existing native provider key locked when it is already routed", async () => {
    dshState.providerIds = ["company-gateway"];
    const { container } = renderForm(
      vi.fn(),
      {
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
      },
      "company-gateway",
    );

    expect(
      container.querySelector<HTMLInputElement>('input[name="name"]')?.value,
    ).toBe("Company Gateway");
    expect(screen.getByLabelText(/Provider Key/)).toHaveValue(
      "company-gateway",
    );
    expect(screen.getByLabelText(/Provider Key/)).toBeDisabled();
    expect(screen.getByLabelText("Base URL")).toHaveValue(
      "https://company.example/v1",
    );
    expect(screen.getByLabelText("Default Model")).toHaveValue(
      "deepseek-v4-pro",
    );
    expect(screen.getAllByPlaceholderText("Model ID")[0]).toHaveValue(
      "deepseek-v4-pro",
    );
  });

  it("locks the key for an existing DSH provider not yet in the live config", () => {
    dshState.providerIds = [];
    renderForm(
      vi.fn(),
      {
        name: "Legacy Route",
        category: "custom",
        settingsConfig: {
          displayName: "Legacy Route",
          api: "openai-completions",
          baseURL: "https://legacy.example/v1",
          apiKeyEnv: "LEGACY_API_KEY",
          apiKey: "secret",
          models: [{ id: "deepseek-v4-pro", name: "DeepSeek V4 Pro" }],
        },
        meta: { providerType: "dsh_pi_ai" },
      },
      "legacy-route",
    );

    expect(screen.getByLabelText(/Provider Key/)).toHaveValue("legacy-route");
    expect(screen.getByLabelText(/Provider Key/)).toBeDisabled();
  });
});
