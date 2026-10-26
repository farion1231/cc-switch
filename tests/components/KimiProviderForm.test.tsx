import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { KimiProviderForm } from "@/components/providers/forms/KimiProviderForm";
import { kimiProviderPresets } from "@/config/kimiProviderPresets";
import { MODELS_DEV_API_URL } from "@/lib/modelsDev";
import type { ProviderCategory } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";
import { server } from "../msw/server";

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

const config = {
  provider: {
    type: "kimi",
    base_url: "https://api.kimi.com/coding/v1",
    api_key: "",
    oauth: { storage: "file", key: "kimi-code" },
    custom_headers: { "X-Custom": "keep" },
  },
  models: {
    "kimi-code/k3": {
      provider: "managed:kimi-code",
      model: "k3",
      max_context_size: 1048576,
      capabilities: ["thinking", "tool_use"],
      overrides: { default_effort: "max" },
    },
  },
};

// 模型参数收在每行的展开区里，和 Pi、OpenCode 一样。
function expandModels() {
  for (const toggle of screen.getAllByRole("button", {
    name: "opencode.toggleModelDetails",
  }))
    fireEvent.click(toggle);
}

function setup(
  onSubmit = vi.fn(),
  category?: ProviderCategory,
  settingsConfig: Record<string, unknown> = config,
) {
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <KimiProviderForm
        appId="kimi"
        providerId="managed:kimi-code"
        initialData={{ name: "Kimi Code", settingsConfig, category }}
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />
    </QueryClientProvider>,
  );
  return onSubmit;
}

describe("KimiProviderForm", () => {
  it.each(["import", "json"] as const)(
    "preserves native capabilities from %s without exposing audio or video controls",
    async (source) => {
      const nativeConfig = structuredClone(config);
      const model = nativeConfig.models["kimi-code/k3"];
      model.capabilities = [
        "image_in",
        "video_in",
        "audio_in",
        "image_out",
        "audio_out",
        "future_capability",
        "thinking",
        "tool_use",
      ];
      const native = {
        ...nativeConfig,
        models: {
          "kimi-code/k3": {
            ...model,
            overrides: {
              ...model.overrides,
              capabilities: [
                "video_in",
                "image_in",
                "audio_in",
                "future_override",
                "always_thinking",
              ],
            },
          },
        },
      };
      const submit = setup(
        vi.fn(),
        undefined,
        source === "import" ? native : config,
      );
      if (source === "json") {
        fireEvent.change(screen.getByLabelText("provider.configJson"), {
          target: { value: JSON.stringify(native) },
        });
      }
      expandModels();
      expect(
        screen.getByRole("switch", { name: "kimi.capabilities.image_in" }),
      ).toBeChecked();
      expect(
        screen.queryByRole("switch", { name: "kimi.capabilities.audio_in" }),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByRole("switch", { name: "kimi.capabilities.video_in" }),
      ).not.toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "Save" }));
      await waitFor(() => expect(submit).toHaveBeenCalledOnce());
      const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
      expect(saved.models["kimi-code/k3"]).toMatchObject({
        capabilities: native.models["kimi-code/k3"].capabilities,
        overrides: native.models["kimi-code/k3"].overrides,
      });
      expect(saved.provider).toEqual(config.provider);
    },
  );

  it("keeps image input configurable on models", async () => {
    const native = structuredClone(config);
    native.models["kimi-code/k3"].capabilities.push(
      "audio_in",
      "future_capability",
    );
    const submit = setup(vi.fn(), undefined, native);
    expandModels();
    fireEvent.click(
      screen.getByRole("switch", { name: "kimi.capabilities.image_in" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    expect(
      JSON.parse(submit.mock.calls[0][0].settingsConfig).models["kimi-code/k3"]
        .capabilities,
    ).toEqual([
      "thinking",
      "tool_use",
      "audio_in",
      "future_capability",
      "image_in",
    ]);
  });

  it.each([
    { group: "vendor", name: "Kimi" },
    { group: "thirdparty", name: "88API" },
    { group: "cloud", name: "Microsoft Foundry" },
    { group: "local", name: "Ollama" },
    { group: "local", name: "LM Studio" },
    { group: "local", name: "vLLM" },
  ])(
    "adds $name from the $group catalog using its native configuration",
    async ({ group, name }) => {
      const user = userEvent.setup();
      const submit = vi.fn();
      const selected = kimiProviderPresets.find(
        (preset) => preset.name === name,
      )!;
      render(
        <QueryClientProvider client={createTestQueryClient()}>
          <KimiProviderForm
            appId="kimi"
            submitLabel="Save"
            onSubmit={submit}
            onCancel={() => {}}
          />
        </QueryClientProvider>,
      );
      await user.click(
        screen.getByRole("button", {
          name: new RegExp(`^providerPreset\\.group\\.${group} `),
        }),
      );
      await user.click(screen.getByRole("button", { name }));
      expect(screen.getByLabelText("opencode.providerKey")).toHaveValue(
        selected.providerKey,
      );
      expect(screen.getByLabelText("opencode.baseUrl")).toHaveValue(
        selected.settingsConfig.provider.base_url,
      );
      expect(
        screen.queryByLabelText("kimi.providerCategory"),
      ).not.toBeInTheDocument();
      if (selected.category !== "local") {
        fireEvent.change(screen.getByLabelText("API Key"), {
          target: { value: "test-key" },
        });
      }
      await user.click(screen.getByRole("button", { name: "Save" }));
      await waitFor(() => expect(submit).toHaveBeenCalledOnce());
      const saved = submit.mock.calls[0][0];
      expect(saved).toMatchObject({
        presetCategory: selected.category,
        providerKey: selected.providerKey,
      });
      const settings = JSON.parse(saved.settingsConfig);
      expect(settings.provider).toMatchObject({
        ...selected.settingsConfig.provider,
        api_key: selected.category === "local" ? "local" : "test-key",
      });
      for (const [key, model] of Object.entries(
        selected.settingsConfig.models,
      )) {
        expect(settings.models[key]).toEqual({
          ...model,
          provider: selected.providerKey,
        });
      }
    },
  );

  it.each([
    "custom",
    "cn_official",
    "third_party",
    "aggregator",
    "cloud_provider",
    "local",
  ] as const)(
    "preserves the category when editing an existing %s provider",
    async (category) => {
      const user = userEvent.setup();
      const submit = setup(vi.fn(), category);
      await user.click(screen.getByRole("button", { name: "Save" }));
      await waitFor(() => expect(submit).toHaveBeenCalledOnce());
      expect(submit.mock.calls[0][0].presetCategory).toBe(category);
      expect(JSON.parse(submit.mock.calls[0][0].settingsConfig)).toEqual(
        config,
      );
    },
  );

  it("edits only the models of an official account, keeping its login", async () => {
    const submit = setup();
    expect(
      screen.getByText("kimi.oauth.connectionManaged"),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("opencode.baseUrl")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("API Key")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
    expect(saved.provider).toEqual(config.provider);
    expect(saved.models).toEqual(config.models);
    expect(submit.mock.calls[0][0].providerKey).toBe("managed:kimi-code");
  });

  it("adds models from the official account's own catalog", async () => {
    const user = userEvent.setup();
    let requested: unknown;
    server.use(
      http.post("http://tauri.local/kimi_oauth_models", async ({ request }) => {
        requested = await request.json();
        return HttpResponse.json({
          "kimi-code/k3": {
            provider: "managed:kimi-code",
            model: "k3",
            max_context_size: 1048576,
          },
          "kimi-code/k3.1-pro": {
            provider: "managed:kimi-code",
            model: "k3.1-pro",
            display_name: "K3.1 Pro",
            max_context_size: 1048576,
            capabilities: ["tool_use", "thinking"],
            support_efforts: ["high", "max"],
            default_effort: "max",
          },
        });
      }),
    );
    const submit = setup();
    await user.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await user.click(await screen.findByRole("checkbox", { name: "k3.1-pro" }));
    await user.click(
      screen.getByRole("button", { name: "providerForm.modelPicker.apply" }),
    );
    expect(requested).toEqual({ accountId: "managed:kimi-code" });
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
    expect(saved.models["kimi-code/k3.1-pro"]).toEqual({
      provider: "managed:kimi-code",
      model: "k3.1-pro",
      display_name: "K3.1 Pro",
      max_context_size: 1048576,
      capabilities: ["tool_use", "thinking"],
      support_efforts: ["high", "max"],
      default_effort: "max",
    });
    expect(saved.models["kimi-code/k3"]).toEqual(config.models["kimi-code/k3"]);
  });

  it("shows request headers inline like Pi", () => {
    const { oauth: _oauth, ...apiKeyProvider } = config.provider;
    setup(vi.fn(), undefined, {
      ...config,
      provider: { ...apiKeyProvider, type: "openai", api_key: "sk-test" },
    });
    expect(screen.getByDisplayValue("X-Custom")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", {
        name: "providerForm.advancedOptionsToggle",
      }),
    ).not.toBeInTheDocument();
  });

  it("flags a preset whose provider key collides with an existing card", async () => {
    const user = userEvent.setup();
    const selected = kimiProviderPresets.find(
      (preset) => preset.name === "Kimi",
    )!;
    server.use(
      http.post("http://tauri.local/get_providers", () =>
        HttpResponse.json({
          [selected.providerKey]: {
            id: selected.providerKey,
            name: "Kimi",
            settingsConfig: {},
          },
        }),
      ),
    );
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <KimiProviderForm
          appId="kimi"
          submitLabel="Save"
          onSubmit={vi.fn()}
          onCancel={() => {}}
        />
      </QueryClientProvider>,
    );
    await user.click(
      screen.getByRole("button", { name: /^providerPreset\.group\.vendor / }),
    );
    await user.click(screen.getByRole("button", { name: "Kimi" }));
    expect(
      await screen.findByText("opencode.providerKeyDuplicate"),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("opencode.providerKey")).toHaveValue(
      selected.providerKey,
    );
  });

  it("blocks invalid JSON until the editor is corrected", () => {
    const submit = setup();
    const editor = screen.getByLabelText("provider.configJson");
    fireEvent.change(editor, { target: { value: "{" } });
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    fireEvent.change(editor, { target: { value: JSON.stringify(config) } });
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
    expect(submit).not.toHaveBeenCalled();
  });

  it("requires at least one model with a positive context length", () => {
    setup();
    expandModels();
    fireEvent.change(screen.getByLabelText("kimi.contextSize"), {
      target: { value: "0" },
    });
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("kimi.contextSize"), {
      target: { value: "262144" },
    });
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "common.delete" }));
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("adds fetched models and fills their window and capabilities from models.dev", async () => {
    const user = userEvent.setup();
    let fetchRequest: Record<string, unknown> | undefined;
    server.use(
      http.post(
        "http://tauri.local/fetch_models_for_config",
        async ({ request }) => {
          fetchRequest = (await request.json()) as Record<string, unknown>;
          return HttpResponse.json([
            { id: "model-x", ownedBy: "example" },
            { id: "model-y", ownedBy: "example" },
          ]);
        },
      ),
      http.get(MODELS_DEV_API_URL, () =>
        HttpResponse.json({
          example: {
            api: "https://api.example.com/v1",
            models: {
              "model-x": {
                limit: { context: 123456 },
                reasoning: true,
                modalities: { input: ["text", "image"], output: ["text"] },
              },
            },
          },
        }),
      ),
    );
    const submit = vi.fn();
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <KimiProviderForm
          appId="kimi"
          submitLabel="Save"
          onSubmit={submit}
          onCancel={() => {}}
        />
      </QueryClientProvider>,
    );
    fireEvent.change(screen.getByLabelText("opencode.providerKey"), {
      target: { value: "acme" },
    });
    fireEvent.change(screen.getByLabelText("opencode.baseUrl"), {
      target: { value: "https://api.example.com/v1" },
    });
    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "sk-test" },
    });
    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "Acme" },
    });

    await user.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await user.click(await screen.findByRole("checkbox", { name: "model-x" }));
    await user.click(await screen.findByRole("checkbox", { name: "model-y" }));
    await user.click(
      screen.getByRole("button", { name: "providerForm.modelPicker.apply" }),
    );

    expect(fetchRequest).toMatchObject({
      baseUrl: "https://api.example.com/v1",
      apiKey: "sk-test",
    });
    await waitFor(() =>
      expect(
        JSON.parse(
          (screen.getByLabelText("provider.configJson") as HTMLTextAreaElement)
            .value,
        ).models["acme/model-x"].max_context_size,
      ).toBe(123456),
    );
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
    expect(saved.models).toEqual({
      "acme/model-x": {
        provider: "acme",
        model: "model-x",
        max_context_size: 123456,
        capabilities: ["tool_use", "image_in", "thinking"],
      },
      // models.dev 没有这款：保留占位窗口，用户自己核对。
      "acme/model-y": {
        provider: "acme",
        model: "model-y",
        max_context_size: 262144,
        capabilities: ["tool_use"],
      },
    });
  });

  it("uses the Anthropic auth header when fetching models for Anthropic providers", async () => {
    const user = userEvent.setup();
    let fetchRequest: Record<string, unknown> | undefined;
    server.use(
      http.post(
        "http://tauri.local/fetch_models_for_config",
        async ({ request }) => {
          fetchRequest = (await request.json()) as Record<string, unknown>;
          return HttpResponse.json([]);
        },
      ),
    );
    setup(vi.fn(), undefined, {
      ...config,
      provider: {
        type: "anthropic",
        base_url: "https://api.example.com",
        api_key: "sk-test",
      },
    });
    await user.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await waitFor(() =>
      expect(fetchRequest).toMatchObject({ apiFormat: "anthropic-messages" }),
    );
  });
});
