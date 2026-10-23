import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { QwenProviderForm } from "@/components/providers/forms/QwenProviderForm";
import { qwenProviderPresets } from "@/config/qwenProviderPresets";
import { http, HttpResponse } from "msw";
import { MODELS_DEV_API_URL } from "@/lib/modelsDev";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";

beforeAll(() => {
  Element.prototype.scrollIntoView ??= vi.fn();
});

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    id,
    value,
    onChange,
  }: {
    id: string;
    value: string;
    onChange: (text: string) => void;
  }) => (
    <textarea
      id={id}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

const imported = {
  authType: "openai",
  baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
  envKey: "OPENAI_API_KEY",
  models: [
    {
      id: "qwen3-coder-plus",
      name: "Qwen3 Coder Plus",
      description: "native description",
      capabilities: { vision: true },
      generationConfig: {
        contextWindowSize: 1000000,
        samplingParams: { temperature: 0.2 },
      },
    },
    {
      id: "claude-sonnet",
      authType: "anthropic",
      baseUrl: "https://other.example.com",
    },
  ],
};

function renderForm(
  props: Partial<React.ComponentProps<typeof QwenProviderForm>> = {},
) {
  const onSubmit = vi.fn();
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <QwenProviderForm
        appId="qwen"
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={() => {}}
        {...props}
      />
    </QueryClientProvider>,
  );
  return onSubmit;
}

const save = () => screen.getByRole("button", { name: "Save" });
// Context windows live in each model's collapsed details, as in the other forms.
const expandModels = () =>
  screen
    .getAllByRole("button", { name: "opencode.toggleModelDetails" })
    .forEach((toggle) => fireEvent.click(toggle));

describe("QwenProviderForm", () => {
  it.each([
    { group: "thirdparty", name: "OpenRouter" },
    { group: "local", name: "Ollama" },
  ])(
    "adds $name from the $group catalog using the native profile shape",
    async ({ group, name }) => {
      const user = userEvent.setup();
      const selected = qwenProviderPresets.find(
        (preset) => preset.name === name,
      )!;
      const submit = renderForm();
      await user.click(
        screen.getByRole("button", {
          name: new RegExp(`^providerPreset\\.group\\.${group} `),
        }),
      );
      await user.click(screen.getByRole("button", { name }));
      expect(screen.getByLabelText("opencode.providerKey")).toHaveValue(
        selected.providerKey,
      );
      if (selected.category !== "local") {
        fireEvent.change(screen.getByLabelText("API Key"), {
          target: { value: "test-key" },
        });
      }
      await user.click(save());
      await waitFor(() => expect(submit).toHaveBeenCalledOnce());
      const saved = submit.mock.calls[0][0];
      expect(saved).toMatchObject({
        presetCategory: selected.category,
        providerKey: selected.providerKey,
      });
      expect(JSON.parse(saved.settingsConfig)).toEqual({
        ...selected.settingsConfig,
        apiKey: selected.category === "local" ? "local" : "test-key",
      });
    },
  );

  it("requires a valid provider key and at least one named model", () => {
    renderForm();
    fireEvent.change(screen.getByLabelText(/provider\.name/), {
      target: { value: "My Provider" },
    });
    const key = screen.getByLabelText("opencode.providerKey");
    fireEvent.change(key, { target: { value: "My_Key" } });
    expect(key).toHaveValue("mykey");
    fireEvent.click(screen.getByRole("button", { name: /opencode\.addModel/ }));
    expect(save()).toBeDisabled();
    fireEvent.change(screen.getByLabelText("opencode.modelId"), {
      target: { value: "qwen3-coder-plus" },
    });
    expect(save()).toBeEnabled();
    fireEvent.change(key, { target: { value: "-bad" } });
    expect(save()).toBeDisabled();
    fireEvent.change(key, { target: { value: "my-key" } });
    expect(save()).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "qwen.removeModel" }));
    expect(save()).toBeDisabled();
  });

  it("fetches models from the endpoint and fills picked ones", async () => {
    const user = userEvent.setup();
    let request: Record<string, unknown> | undefined;
    server.use(
      http.post(
        "http://tauri.local/fetch_models_for_config",
        async ({ request: req }) => {
          request = (await req.json()) as Record<string, unknown>;
          return HttpResponse.json([
            { id: "qwen3.7-plus", ownedBy: null },
            { id: "glm-5", ownedBy: null },
          ]);
        },
      ),
      http.get(MODELS_DEV_API_URL, () =>
        HttpResponse.json({
          relay: {
            api: "https://relay.example.com/v1",
            models: { "qwen3.7-plus": { limit: { context: 1000000 } } },
          },
        }),
      ),
    );
    const submit = renderForm();
    fireEvent.change(screen.getByLabelText(/provider\.name/), {
      target: { value: "Relay" },
    });
    fireEvent.change(screen.getByLabelText("opencode.providerKey"), {
      target: { value: "relay" },
    });
    fireEvent.change(screen.getByLabelText("opencode.baseUrl"), {
      target: { value: "https://relay.example.com/v1" },
    });
    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "sk-relay" },
    });
    // A blank manual row is replaced by the picked models.
    fireEvent.click(screen.getByRole("button", { name: /opencode\.addModel/ }));

    await user.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await user.click(
      await screen.findByRole("checkbox", { name: "qwen3.7-plus" }),
    );
    await user.click(screen.getByRole("checkbox", { name: "glm-5" }));
    await user.click(
      screen.getByRole("button", { name: "providerForm.modelPicker.apply" }),
    );

    expect(request).toMatchObject({
      baseUrl: "https://relay.example.com/v1",
      apiKey: "sk-relay",
      apiFormat: "openai-completions",
    });
    await waitFor(() =>
      expect(
        screen
          .getAllByLabelText("opencode.modelId")
          .map((input) => (input as HTMLInputElement).value),
      ).toEqual(["qwen3.7-plus", "glm-5"]),
    );
    expandModels();
    await waitFor(() =>
      expect(screen.getAllByLabelText("qwen.contextWindowSize")[0]).toHaveValue(
        1000000,
      ),
    );
    await user.click(save());
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    expect(JSON.parse(submit.mock.calls[0][0].settingsConfig).models).toEqual([
      { id: "qwen3.7-plus", generationConfig: { contextWindowSize: 1000000 } },
      { id: "glm-5" },
    ]);
    // A full form flow, like the App integration tests.
  }, 10_000);

  it("requires a base URL for the Anthropic protocol", () => {
    renderForm({
      providerId: "anthropic-relay",
      initialData: {
        name: "Relay",
        settingsConfig: {
          authType: "anthropic",
          apiKey: "sk-test",
          models: [{ id: "claude-sonnet" }],
        },
      },
    });
    expect(save()).toBeDisabled();
    expect(screen.getByText("qwen.baseUrlRequired")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("opencode.baseUrl"), {
      target: { value: "https://relay.example.com" },
    });
    expect(save()).toBeEnabled();
  });

  it("rejects duplicate model IDs and non-positive context windows", () => {
    renderForm({
      providerId: "custom",
      initialData: {
        name: "Custom",
        settingsConfig: {
          authType: "openai",
          models: [{ id: "a" }, { id: "b" }],
        },
      },
    });
    expect(save()).toBeEnabled();
    const [, second] = screen.getAllByLabelText("opencode.modelId");
    fireEvent.change(second, { target: { value: "a" } });
    expect(save()).toBeDisabled();
    expect(screen.getAllByText("qwen.duplicateModelId")).toHaveLength(2);
    fireEvent.change(second, { target: { value: "b" } });
    expandModels();
    const [context] = screen.getAllByLabelText("qwen.contextWindowSize");
    fireEvent.change(context, { target: { value: "0" } });
    expect(save()).toBeDisabled();
    fireEvent.change(context, { target: { value: "131072" } });
    expect(save()).toBeEnabled();
  });

  it("preserves envKey and unknown native model fields on edit", async () => {
    const submit = renderForm({
      providerId: "openai-api-key",
      initialData: { name: "OpenAI", settingsConfig: imported },
    });
    expect(screen.getByLabelText("qwen.envKey")).toHaveValue("OPENAI_API_KEY");
    expandModels();
    // A model imported with its own protocol and endpoint shows them read-only.
    expect(
      screen.getByText("qwen.authTypes.anthropic · https://other.example.com"),
    ).toBeInTheDocument();
    const [context] = screen.getAllByLabelText("qwen.contextWindowSize");
    fireEvent.change(context, { target: { value: "262144" } });
    fireEvent.click(save());
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    expect(submit.mock.calls[0][0].providerKey).toBe("openai-api-key");
    const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
    expect(saved).toEqual({
      ...imported,
      models: [
        {
          ...imported.models[0],
          generationConfig: {
            ...imported.models[0].generationConfig,
            contextWindowSize: 262144,
          },
        },
        imported.models[1],
      ],
    });
  });

  it("blocks invalid JSON until the editor is corrected", () => {
    const submit = renderForm({
      providerId: "openai-api-key",
      initialData: { name: "OpenAI", settingsConfig: imported },
    });
    const editor = screen.getByLabelText("provider.configJson");
    fireEvent.change(editor, { target: { value: "{" } });
    expect(save()).toBeDisabled();
    fireEvent.change(editor, { target: { value: JSON.stringify(imported) } });
    expect(save()).toBeEnabled();
    expect(submit).not.toHaveBeenCalled();
  });
});
