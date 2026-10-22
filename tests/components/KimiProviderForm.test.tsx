import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { KimiProviderForm } from "@/components/providers/forms/KimiProviderForm";
import { kimiProviderPresets } from "@/config/kimiProviderPresets";
import type { ProviderCategory } from "@/types";
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
      expect(
        screen.getByRole("checkbox", { name: "kimi.capabilities.image_in" }),
      ).toBeChecked();
      expect(
        screen.queryByRole("checkbox", { name: "kimi.capabilities.audio_in" }),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByRole("checkbox", { name: "kimi.capabilities.video_in" }),
      ).not.toBeInTheDocument();
      fireEvent.change(screen.getByLabelText(/kimi\.baseUrl/), {
        target: { value: "https://updated.example.com/v1" },
      });
      fireEvent.click(screen.getByRole("button", { name: "Save" }));
      await waitFor(() => expect(submit).toHaveBeenCalledOnce());
      const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
      expect(saved.models["kimi-code/k3"]).toMatchObject({
        capabilities: native.models["kimi-code/k3"].capabilities,
        overrides: native.models["kimi-code/k3"].overrides,
      });
      expect(saved.provider).toEqual({
        ...config.provider,
        base_url: "https://updated.example.com/v1",
      });
    },
  );

  it("keeps image input configurable on models", async () => {
    const native = structuredClone(config);
    native.models["kimi-code/k3"].capabilities.push(
      "audio_in",
      "future_capability",
    );
    const submit = setup(vi.fn(), undefined, native);
    fireEvent.click(
      screen.getByRole("checkbox", { name: "kimi.capabilities.image_in" }),
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
      expect(screen.getByLabelText("opencode.providerKey *")).toHaveValue(
        selected.providerKey,
      );
      expect(screen.getByLabelText("kimi.baseUrl *")).toHaveValue(
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

  it("edits native endpoints while preserving OAuth and model overrides", async () => {
    const submit = setup();
    fireEvent.change(screen.getByLabelText("kimi.baseUrl *"), {
      target: { value: "https://example.com/v1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
    expect(saved.provider).toEqual({
      ...config.provider,
      base_url: "https://example.com/v1",
    });
    expect(saved.models).toEqual(config.models);
    expect(submit.mock.calls[0][0].providerKey).toBe("managed:kimi-code");
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
    fireEvent.change(screen.getByLabelText("kimi.contextSize *"), {
      target: { value: "0" },
    });
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("kimi.contextSize *"), {
      target: { value: "262144" },
    });
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "kimi.removeModel" }));
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });
});
