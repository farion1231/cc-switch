import { fireEvent, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PiProviderForm } from "@/components/providers/forms/PiProviderForm";
import { renderWithQueryClient as render } from "../utils/testQueryClient";
import { completeModel } from "./piProviderFormTestHelpers";

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    id,
    value,
    onChange,
    readOnly,
  }: {
    id?: string;
    value: string;
    onChange: (value: string) => void;
    readOnly?: boolean;
  }) => (
    <textarea
      id={id}
      value={value}
      readOnly={readOnly}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

vi.mock("@/components/IconPicker", () => ({
  IconPicker: () => <div data-testid="icon-picker" />,
}));

vi.mock("@/components/ProviderIconBox", () => ({
  ProviderIconBox: () => (
    <span aria-hidden="true" data-testid="provider-icon-box" />
  ),
}));

describe("PiProviderForm thinking maps and model capabilities", () => {
  beforeEach(() => {
    Element.prototype.scrollIntoView = vi.fn();
  });

  it("uses structured Pi-native capabilities and limits in collapsed model details", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save model limits"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    fireEvent.change(screen.getByPlaceholderText("my-provider"), {
      target: { value: "limited-provider" },
    });
    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "Limited provider" },
    });
    fireEvent.change(
      screen.getByPlaceholderText("https://api.example.com/v1"),
      {
        target: { value: "https://api.example.com/v1" },
      },
    );
    await user.click(screen.getByRole("button", { name: "pi.form.addModel" }));
    fireEvent.change(screen.getByPlaceholderText("model-id"), {
      target: { value: "limited-model" },
    });

    expect(
      screen.queryByLabelText("pi.form.contextWindow"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByLabelText("pi.form.maxTokens"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByLabelText("pi.form.reasoning"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByLabelText("pi.form.imageInput"),
    ).not.toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "展开或收起模型详情" }),
    );
    expect(
      screen.queryByText("pi.form.thinkingLevelsLabel"),
    ).not.toBeInTheDocument();
    await user.click(screen.getByLabelText("pi.form.reasoning"));
    expect(
      screen
        .getByLabelText("pi.form.maxTokens")
        .compareDocumentPosition(
          screen.getByText("pi.form.thinkingLevelsLabel"),
        ) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    await user.click(screen.getByLabelText("pi.form.imageInput"));
    fireEvent.change(screen.getByLabelText("pi.form.contextWindow"), {
      target: { value: "128000.5" },
    });
    fireEvent.change(screen.getByLabelText("pi.form.maxTokens"), {
      target: { value: "16384.25" },
    });
    await user.click(screen.getByRole("button", { name: "Save model limits" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig).models).toEqual(
      [
        {
          id: "limited-model",
          name: "limited-model",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 128000.5,
          maxTokens: 16384.25,
        },
      ],
    );
  });

  it("reopens model details and focuses an invalid Pi-native limit", async () => {
    const user = userEvent.setup();
    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save invalid limits"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    fireEvent.change(screen.getByPlaceholderText("my-provider"), {
      target: { value: "invalid-limits" },
    });
    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "Invalid limits" },
    });
    fireEvent.change(
      screen.getByPlaceholderText("https://api.example.com/v1"),
      {
        target: { value: "https://api.example.com/v1" },
      },
    );
    await user.click(screen.getByRole("button", { name: "pi.form.addModel" }));
    fireEvent.change(screen.getByPlaceholderText("model-id"), {
      target: { value: "invalid-model" },
    });

    const toggle = screen.getByRole("button", {
      name: "展开或收起模型详情",
    });
    await user.click(toggle);
    fireEvent.change(screen.getByLabelText("pi.form.contextWindow"), {
      target: { value: "-1" },
    });
    await user.click(toggle);
    await user.click(
      screen.getByRole("button", { name: "Save invalid limits" }),
    );

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "pi.form.positiveNumberRequired",
    );
    await waitFor(() =>
      expect(screen.getByLabelText("pi.form.contextWindow")).toHaveFocus(),
    );
  });

  it("requires custom model limits instead of inferring Pi metadata", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockResolvedValue(undefined);

    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save Pi provider"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    fireEvent.change(screen.getByPlaceholderText("my-provider"), {
      target: { value: "verified-provider" },
    });
    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "Verified provider" },
    });
    fireEvent.change(
      screen.getByPlaceholderText("https://api.example.com/v1"),
      {
        target: { value: "https://api.example.com/v1" },
      },
    );
    fireEvent.click(screen.getByRole("button", { name: "pi.form.addModel" }));
    fireEvent.change(screen.getByPlaceholderText("model-id"), {
      target: { value: "opaque-model" },
    });

    expect(
      screen.queryByLabelText("pi.form.contextWindow"),
    ).not.toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "展开或收起模型详情" }),
    );
    expect(screen.getByLabelText("pi.form.reasoning")).not.toBeChecked();
    expect(screen.getByLabelText("pi.form.imageInput")).not.toBeChecked();
    expect(screen.getByLabelText("pi.form.contextWindow")).toHaveValue(null);
    expect(screen.getByLabelText("pi.form.maxTokens")).toHaveValue(null);

    await user.click(screen.getByRole("button", { name: "Save Pi provider" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "pi.form.positiveNumberRequired",
    );
    expect(onSubmit).not.toHaveBeenCalled();

    fireEvent.change(screen.getByLabelText("pi.form.contextWindow"), {
      target: { value: "128000" },
    });
    fireEvent.change(screen.getByLabelText("pi.form.maxTokens"), {
      target: { value: "16384" },
    });
    await user.click(screen.getByRole("button", { name: "Save Pi provider" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const submitted = onSubmit.mock.calls[0][0];
    expect(submitted.providerKey).toBe("verified-provider");
    expect(JSON.parse(submitted.settingsConfig)).toEqual({
      name: "Verified provider",
      api: "openai-completions",
      baseUrl: "https://api.example.com/v1",
      models: [
        {
          id: "opaque-model",
          name: "opaque-model",
          reasoning: false,
          input: ["text"],
          contextWindow: 128_000,
          maxTokens: 16_384,
        },
      ],
    });
  });

  it("keeps a preset thinking map when the user changes the API", async () => {
    const user = userEvent.setup();
    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save exact thinking map"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );

    await user.click(screen.getByText("Kimi", { selector: "span" }));
    const configEditor = screen.getByLabelText(
      "provider.configJson",
    ) as HTMLTextAreaElement;
    expect(JSON.parse(configEditor.value).models[0].thinkingLevelMap).toEqual({
      off: null,
      minimal: null,
      low: "low",
      medium: null,
      high: "high",
      xhigh: null,
      max: "max",
    });

    await user.click(document.querySelector("#pi-provider-api-select")!);
    await user.click(
      await screen.findByRole("option", { name: "OpenAI Responses" }),
    );
    expect(JSON.parse(configEditor.value).models[0].thinkingLevelMap).toEqual({
      off: null,
      minimal: null,
      low: "low",
      medium: null,
      high: "high",
      xhigh: null,
      max: "max",
    });

    await user.click(document.querySelector("#pi-provider-api-select")!);
    await user.click(
      await screen.findByRole("option", {
        name: "OpenAI Chat Completions",
      }),
    );
    expect(JSON.parse(configEditor.value).models[0].thinkingLevelMap).toEqual({
      off: null,
      minimal: null,
      low: "low",
      medium: null,
      high: "high",
      xhigh: null,
      max: "max",
    });
  });

  it("edits Pi thinking-map missing, null, and string states from the collapsed capability area", async () => {
    const user = userEvent.setup();
    // Start with a model so this test exercises thinking-map interactions without
    // repeating the separately covered preset selection and model creation flow.
    render(
      <PiProviderForm
        appId="pi"
        providerId="custom-provider"
        submitLabel="Save custom thinking map"
        onSubmit={vi.fn()}
        onCancel={() => {}}
        initialData={{
          name: "Custom reasoning provider",
          settingsConfig: {
            api: "openai-completions",
            models: [completeModel("custom-reasoning-model")],
          },
        }}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "展开或收起模型详情" }),
    );
    await user.click(screen.getByLabelText("pi.form.reasoning"));
    await user.click(
      screen.getByRole("button", {
        name: "pi.form.customizeThinkingLevels",
      }),
    );

    const configEditor = screen.getByLabelText(
      "provider.configJson",
    ) as HTMLTextAreaElement;
    expect(JSON.parse(configEditor.value).models[0]).not.toHaveProperty(
      "thinkingLevelMap",
    );

    await user.click(
      screen
        .getByText("pi.form.thinkingLevels.high")
        .closest("button") as HTMLButtonElement,
    );
    await user.click(
      screen.getByLabelText("pi.form.thinkingLevelMarkUnavailable"),
    );
    expect(JSON.parse(configEditor.value).models[0].thinkingLevelMap).toEqual({
      high: null,
    });

    await user.keyboard("{Escape}");
    await user.click(
      screen
        .getByText("pi.form.thinkingLevels.xhigh")
        .closest("button") as HTMLButtonElement,
    );
    await user.click(screen.getByLabelText("pi.form.thinkingLevelMapTo"));
    fireEvent.change(screen.getByLabelText("pi.form.thinkingLevelValue"), {
      target: { value: "xhigh-vendor-value" },
    });
    expect(JSON.parse(configEditor.value).models[0].thinkingLevelMap).toEqual({
      high: null,
      xhigh: "xhigh-vendor-value",
    });

    await user.click(screen.getByLabelText("pi.form.reasoning"));
    expect(
      screen.queryByText("pi.form.thinkingLevelsLabel"),
    ).not.toBeInTheDocument();
    expect(JSON.parse(configEditor.value).models[0].thinkingLevelMap).toEqual({
      high: null,
      xhigh: "xhigh-vendor-value",
    });

    await user.click(screen.getByLabelText("pi.form.reasoning"));
    expect(screen.getByText("pi.form.thinkingLevelsLabel")).toBeVisible();
  });

  it("lets the user edit a preset thinking map without automatic recovery", async () => {
    const user = userEvent.setup();
    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save thinking map"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );

    await user.click(screen.getByText("DeepSeek", { selector: "span" }));
    await user.click(
      screen.getAllByRole("button", {
        name: "展开或收起模型详情",
      })[0],
    );

    const configEditor = screen.getByLabelText(
      "provider.configJson",
    ) as HTMLTextAreaElement;
    const automaticMap = {
      minimal: null,
      low: null,
      medium: null,
      high: "high",
      max: "max",
    };
    expect(JSON.parse(configEditor.value).models[0].thinkingLevelMap).toEqual(
      automaticMap,
    );

    await user.click(
      screen.getByRole("button", {
        name: "pi.form.customizeThinkingLevels",
      }),
    );
    await user.click(
      screen
        .getByText("pi.form.thinkingLevels.high")
        .closest("button") as HTMLButtonElement,
    );
    await user.click(
      screen.getByLabelText("pi.form.thinkingLevelMarkUnavailable"),
    );

    expect(
      screen.queryByRole("button", {
        name: "pi.form.restoreModelAutofill",
      }),
    ).not.toBeInTheDocument();
    expect(JSON.parse(configEditor.value).models[0].thinkingLevelMap).toEqual({
      ...automaticMap,
      high: null,
    });
  });

  it.each([
    [
      "an explicit empty thinking map",
      { off: null, high: "provider-high" },
      {},
    ],
    ["a sparse thinking map", {}, { off: null, high: "provider-high" }],
  ])(
    "round-trips %s from configuration JSON without expanding it",
    async (_label, initialMap, editedMap) => {
      const onSubmit = vi.fn().mockResolvedValue(undefined);
      const input = {
        name: "Manual thinking map",
        baseUrl: "https://api.example.com/v1",
        api: "openai-completions",
        models: [
          {
            id: "custom-reasoning-model",
            name: "Custom Reasoning Model",
            reasoning: true,
            input: ["text"],
            contextWindow: 128_000,
            maxTokens: 16_384,
            thinkingLevelMap: initialMap,
            futureModelField: { keep: true },
          },
        ],
      };

      render(
        <PiProviderForm
          appId="pi"
          providerId="custom-provider"
          submitLabel="Save JSON thinking map"
          onSubmit={onSubmit}
          onCancel={() => {}}
          initialData={{ name: input.name, settingsConfig: input }}
        />,
      );

      const configEditor = screen.getByLabelText(
        "provider.configJson",
      ) as HTMLTextAreaElement;
      const editedConfig = JSON.parse(configEditor.value);
      editedConfig.models[0].thinkingLevelMap = editedMap;
      fireEvent.change(configEditor, {
        target: { value: JSON.stringify(editedConfig, null, 2) },
      });
      fireEvent.click(
        screen.getByRole("button", { name: "Save JSON thinking map" }),
      );

      await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
      expect(
        JSON.parse(onSubmit.mock.calls[0][0].settingsConfig).models[0],
      ).toEqual({
        ...input.models[0],
        thinkingLevelMap: editedMap,
      });
    },
  );

  it("rejects non-native thinking map keys before saving", async () => {
    const onSubmit = vi.fn();
    render(
      <PiProviderForm
        appId="pi"
        providerId="custom-provider"
        submitLabel="Save invalid thinking map"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{
          name: "Invalid thinking map",
          settingsConfig: {
            name: "Invalid thinking map",
            api: "openai-completions",
            baseUrl: "https://api.example.com/v1",
            models: [
              {
                id: "model",
                name: "Model",
                reasoning: true,
                input: ["text"],
                contextWindow: 128_000,
                maxTokens: 16_384,
                thinkingLevelMap: { turbo: "turbo" },
              },
            ],
          },
        }}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "Save invalid thinking map" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "pi.form.thinkingLevelMapInvalid",
    );
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("keeps manual fields stable after an unrelated JSON edit", async () => {
    const user = userEvent.setup();
    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save automatic fields"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    await user.click(screen.getByRole("button", { name: "pi.form.addModel" }));
    fireEvent.change(screen.getByLabelText("pi.form.modelId"), {
      target: { value: "gpt-5.6-sol" },
    });
    expect(screen.getByLabelText("pi.form.modelName")).toHaveValue(
      "gpt-5.6-sol",
    );

    const configEditor = screen.getByLabelText(
      "provider.configJson",
    ) as HTMLTextAreaElement;
    const config = JSON.parse(configEditor.value);
    config.nativeField = { keep: true };
    fireEvent.change(configEditor, {
      target: { value: JSON.stringify(config, null, 2) },
    });
    fireEvent.change(screen.getByLabelText("pi.form.modelId"), {
      target: { value: "unknown-after-json-edit" },
    });

    expect(screen.getByLabelText("pi.form.modelName")).toHaveValue(
      "unknown-after-json-edit",
    );
    await user.click(
      screen.getByRole("button", { name: "展开或收起模型详情" }),
    );
    expect(screen.getByLabelText("pi.form.reasoning")).not.toBeChecked();
    expect(screen.getByLabelText("pi.form.imageInput")).not.toBeChecked();
    expect(screen.getByLabelText("pi.form.contextWindow")).toHaveValue(null);
    expect(screen.getByLabelText("pi.form.maxTokens")).toHaveValue(null);
    expect(JSON.parse(configEditor.value).nativeField).toEqual({ keep: true });
  });

  it("does not infer capabilities when JSON changes only the model ID", async () => {
    const user = userEvent.setup();
    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save JSON model ID"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    await user.click(screen.getByRole("button", { name: "pi.form.addModel" }));
    fireEvent.change(screen.getByLabelText("pi.form.modelId"), {
      target: { value: "gpt-5.6-sol" },
    });

    const configEditor = screen.getByLabelText(
      "provider.configJson",
    ) as HTMLTextAreaElement;
    const config = JSON.parse(configEditor.value);
    config.models[0].id = "claude-haiku-4-5";
    fireEvent.change(configEditor, {
      target: { value: JSON.stringify(config, null, 2) },
    });

    expect(screen.getByLabelText("pi.form.modelName")).toHaveValue(
      "gpt-5.6-sol",
    );
    await user.click(
      screen.getByRole("button", { name: "展开或收起模型详情" }),
    );
    expect(screen.getByLabelText("pi.form.reasoning")).not.toBeChecked();
    expect(screen.getByLabelText("pi.form.imageInput")).not.toBeChecked();
    expect(screen.getByLabelText("pi.form.contextWindow")).toHaveValue(null);
    expect(screen.getByLabelText("pi.form.maxTokens")).toHaveValue(null);
    expect(
      screen.queryByRole("button", { name: "pi.form.restoreModelAutofill" }),
    ).not.toBeInTheDocument();
  });

  it("keeps a capability explicitly changed with a JSON model ID change", async () => {
    const user = userEvent.setup();
    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save JSON model override"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    await user.click(screen.getByRole("button", { name: "pi.form.addModel" }));
    fireEvent.change(screen.getByLabelText("pi.form.modelId"), {
      target: { value: "gpt-5.6-sol" },
    });

    const configEditor = screen.getByLabelText(
      "provider.configJson",
    ) as HTMLTextAreaElement;
    const config = JSON.parse(configEditor.value);
    config.models[0].id = "claude-haiku-4-5";
    config.models[0].maxTokens = 777;
    fireEvent.change(configEditor, {
      target: { value: JSON.stringify(config, null, 2) },
    });

    expect(screen.getByLabelText("pi.form.modelName")).toHaveValue(
      "gpt-5.6-sol",
    );
    await user.click(
      screen.getByRole("button", { name: "展开或收起模型详情" }),
    );
    expect(screen.getByLabelText("pi.form.contextWindow")).toHaveValue(null);
    expect(screen.getByLabelText("pi.form.maxTokens")).toHaveValue(777);
    expect(
      screen.queryByRole("button", { name: "pi.form.restoreModelAutofill" }),
    ).not.toBeInTheDocument();
  });

  it("keeps an id-only JSON draft but requires complete new models", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save JSON model"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    fireEvent.change(screen.getByPlaceholderText("my-provider"), {
      target: { value: "json-provider" },
    });
    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "JSON provider" },
    });
    fireEvent.change(
      screen.getByPlaceholderText("https://api.example.com/v1"),
      { target: { value: "https://api.example.com/v1" } },
    );
    const configEditor = screen.getByLabelText(
      "provider.configJson",
    ) as HTMLTextAreaElement;
    const config = JSON.parse(configEditor.value);
    config.models = [{ id: "json-only-model" }];
    fireEvent.change(configEditor, {
      target: { value: JSON.stringify(config, null, 2) },
    });

    await waitFor(() =>
      expect(JSON.parse(configEditor.value).models).toEqual([
        { id: "json-only-model" },
      ]),
    );
    expect(screen.getByLabelText("pi.form.modelName")).toHaveValue("");

    await user.click(screen.getByRole("button", { name: "Save JSON model" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "pi.form.modelNameRequired",
    );
    expect(onSubmit).not.toHaveBeenCalled();

    fireEvent.change(screen.getByLabelText("pi.form.modelName"), {
      target: { value: "JSON model" },
    });
    await user.click(
      screen.getByRole("button", { name: "展开或收起模型详情" }),
    );
    fireEvent.change(screen.getByLabelText("pi.form.contextWindow"), {
      target: { value: "128000" },
    });
    fireEvent.change(screen.getByLabelText("pi.form.maxTokens"), {
      target: { value: "16384" },
    });
    await user.click(screen.getByRole("button", { name: "Save JSON model" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig).models).toEqual(
      [
        {
          id: "json-only-model",
          name: "JSON model",
          reasoning: false,
          input: ["text"],
          contextWindow: 128000,
          maxTokens: 16384,
        },
      ],
    );
  });
});
