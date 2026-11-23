import { fireEvent, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PiProviderForm } from "@/components/providers/forms/PiProviderForm";
import { renderWithQueryClient as render } from "../utils/testQueryClient";
import {
  completeModel,
  openAdvancedOptions,
} from "./piProviderFormTestHelpers";

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

describe("PiProviderForm JSON sync and native round-trip", () => {
  beforeEach(() => {
    Element.prototype.scrollIntoView = vi.fn();
  });

  it("keeps config JSON synchronized with structured fields", async () => {
    const user = userEvent.setup();
    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save synchronized preview"
        onSubmit={() => {}}
        onCancel={() => {}}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "Live preview" },
    });
    fireEvent.change(
      screen.getByPlaceholderText("https://api.example.com/v1"),
      {
        target: { value: "https://preview.example/v1" },
      },
    );
    await user.click(screen.getByRole("button", { name: "pi.form.addModel" }));
    fireEvent.change(screen.getByPlaceholderText("model-id"), {
      target: { value: "preview-model" },
    });

    const preview = screen.getByLabelText(
      "provider.configJson",
    ) as HTMLTextAreaElement;
    expect(JSON.parse(preview.value)).toEqual({
      name: "Live preview",
      baseUrl: "https://preview.example/v1",
      api: "openai-completions",
      models: [
        {
          id: "preview-model",
          name: "preview-model",
          reasoning: false,
          input: ["text"],
        },
      ],
    });
  });

  it("keeps an invalid JSON draft authoritative until it is repaired", async () => {
    const user = userEvent.setup();
    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save repaired configuration"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    const configEditor = screen.getByLabelText(
      "provider.configJson",
    ) as HTMLTextAreaElement;
    const validDraft = JSON.stringify(
      {
        name: "",
        api: "openai-completions",
        models: [],
        nativeField: { keep: true },
      },
      null,
      2,
    );
    fireEvent.change(configEditor, { target: { value: validDraft } });
    const invalidDraft = validDraft.slice(0, -1);
    fireEvent.change(configEditor, {
      target: { value: invalidDraft },
    });

    const endpoint = screen.getByPlaceholderText("https://api.example.com/v1");
    expect(configEditor).toHaveValue(invalidDraft);
    expect(endpoint).toBeDisabled();
    expect(endpoint).toHaveValue("");
    expect(screen.getByText("pi.form.fixJsonFirst")).toBeInTheDocument();

    fireEvent.change(configEditor, { target: { value: validDraft } });
    expect(endpoint).toBeEnabled();
    fireEvent.change(endpoint, {
      target: { value: "https://repaired.example/v1" },
    });
    expect(JSON.parse(configEditor.value)).toMatchObject({
      baseUrl: "https://repaired.example/v1",
      nativeField: { keep: true },
    });
  });

  it("does not infer model capabilities when an id-only JSON row is inserted", async () => {
    render(
      <PiProviderForm
        appId="pi"
        providerId="existing-provider"
        submitLabel="Save inserted model"
        onSubmit={vi.fn()}
        onCancel={() => {}}
        initialData={{
          name: "Existing provider",
          settingsConfig: {
            name: "Existing provider",
            baseUrl: "https://api.example.com/v1",
            api: "openai-completions",
            models: [
              {
                id: "existing-model",
                name: "Existing Model",
                reasoning: true,
                input: ["text", "image"],
                contextWindow: 256_000,
                maxTokens: 32_000,
              },
            ],
          },
        }}
      />,
    );

    const configEditor = screen.getByLabelText(
      "provider.configJson",
    ) as HTMLTextAreaElement;
    const config = JSON.parse(configEditor.value);
    config.models.unshift({ id: "brand-new" });
    fireEvent.change(configEditor, {
      target: { value: JSON.stringify(config, null, 2) },
    });

    expect(JSON.parse(configEditor.value).models).toEqual([
      { id: "brand-new" },
      {
        id: "existing-model",
        name: "Existing Model",
        reasoning: true,
        input: ["text", "image"],
        contextWindow: 256_000,
        maxTokens: 32_000,
      },
    ]);
  });

  it("updates structured fields from config JSON and preserves native fields", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save edited JSON"
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
      target: { value: "CC Switch label" },
    });

    const configEditor = screen.getByLabelText("provider.configJson");
    fireEvent.change(configEditor, {
      target: {
        value: JSON.stringify(
          {
            name: "Pi native label",
            api: "anthropic-messages",
            apiKey: "json-key",
            baseUrl: "https://json.example",
            headers: { "X-Title": "JSON provider" },
            compat: {
              supportsDeveloperRole: false,
              chatTemplateKwargs: {
                thinking: { $var: "thinking.enabled" },
              },
            },
            models: [
              {
                id: "json-model",
                name: "JSON Model",
                reasoning: true,
                input: ["text", "image"],
                contextWindow: 128000,
                maxTokens: 16384,
              },
            ],
            nativeField: { keep: true },
          },
          null,
          2,
        ),
      },
    });

    openAdvancedOptions();
    await waitFor(() => {
      expect(screen.getByLabelText("pi.form.credential")).toHaveValue(
        "json-key",
      );
      expect(screen.getByLabelText("Base URL")).toHaveValue(
        "https://json.example",
      );
      expect(screen.getByLabelText("Header")).toHaveValue("X-Title");
      expect(screen.getByLabelText("Value")).toHaveValue("JSON provider");
      expect(
        screen
          .getAllByLabelText("pi.form.optionKey")
          .map((input) => (input as HTMLInputElement).value),
      ).toEqual(
        expect.arrayContaining(["supportsDeveloperRole", "chatTemplateKwargs"]),
      );
      expect(
        screen
          .getAllByLabelText("pi.form.optionValue")
          .map((input) => (input as HTMLInputElement).value),
      ).toEqual(
        expect.arrayContaining([
          "false",
          '{"thinking":{"$var":"thinking.enabled"}}',
        ]),
      );
      expect(screen.getByLabelText("pi.form.modelId")).toHaveValue(
        "json-model",
      );
      expect(screen.getByLabelText("pi.form.modelName")).toHaveValue(
        "JSON Model",
      );
    });

    fireEvent.change(screen.getByLabelText("Base URL"), {
      target: { value: "https://structured.example/v1" },
    });
    const synchronizedConfig = JSON.parse(
      (screen.getByLabelText("provider.configJson") as HTMLTextAreaElement)
        .value,
    );
    expect(synchronizedConfig).toMatchObject({
      name: "Pi native label",
      baseUrl: "https://structured.example/v1",
      nativeField: { keep: true },
    });

    await user.click(screen.getByRole("button", { name: "Save edited JSON" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toEqual(
      synchronizedConfig,
    );
  });

  it("preserves preset fields across a temporarily invalid JSON draft", async () => {
    const user = userEvent.setup();
    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save repaired JSON"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );

    await user.click(screen.getByText("Kimi", { selector: "span" }));
    const configEditor = screen.getByLabelText(
      "provider.configJson",
    ) as HTMLTextAreaElement;
    const originalConfig = JSON.parse(configEditor.value);
    fireEvent.change(configEditor, {
      target: { value: configEditor.value.slice(0, -1) },
    });
    originalConfig.nativeField = { keep: true };
    fireEvent.change(configEditor, {
      target: { value: JSON.stringify(originalConfig, null, 2) },
    });

    await user.click(
      screen.getAllByRole("button", {
        name: "展开或收起模型详情",
      })[0],
    );
    expect(
      screen.queryByRole("button", {
        name: "pi.form.restoreModelAutofill",
      }),
    ).not.toBeInTheDocument();
    expect(screen.getByLabelText("pi.form.reasoning")).toBeChecked();
  });

  it("edits an incomplete model without inventing optional fields", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const input = {
      name: "Existing local model",
      baseUrl: "https://api.example.com/v1",
      api: "openai-responses",
      apiKey: "old-secret",
      models: [
        {
          id: "gpt-5.6-sol",
          maxTokens: 64_000,
          nativeModelField: { keep: true },
        },
      ],
    };

    render(
      <PiProviderForm
        appId="pi"
        providerId="existing-local-provider"
        submitLabel="Save existing local provider"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{ name: input.name, settingsConfig: input }}
      />,
    );

    expect(
      JSON.parse(
        (screen.getByLabelText("provider.configJson") as HTMLTextAreaElement)
          .value,
      ).models,
    ).toEqual(input.models);

    fireEvent.change(screen.getByLabelText("pi.form.credential"), {
      target: { value: "new-secret" },
    });

    fireEvent.click(
      screen.getByRole("button", { name: "Save existing local provider" }),
    );
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toEqual({
      ...input,
      apiKey: "new-secret",
    });
  });

  it("round-trips a minimal explicit built-in provider node without inventing fields", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);

    render(
      <PiProviderForm
        appId="pi"
        providerId="anthropic"
        submitLabel="Save explicit Anthropic"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{ name: "anthropic", settingsConfig: {} }}
      />,
    );

    expect(screen.getByLabelText("provider.configJson")).toHaveValue("{}");
    fireEvent.click(
      screen.getByRole("button", { name: "Save explicit Anthropic" }),
    );

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toEqual({});
  });

  it("edits a partial built-in provider override without requiring inherited transport fields", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const input = {
      apiKey: "secret",
      models: [
        {
          id: "deepseek-custom",
          name: "DeepSeek Custom",
          reasoning: true,
          input: ["text"],
          contextWindow: 128_000,
          maxTokens: 32_768,
          futureModelField: { keep: true },
        },
      ],
      futureProviderField: { keep: true },
    };

    render(
      <PiProviderForm
        appId="pi"
        providerId="deepseek"
        submitLabel="Save explicit DeepSeek"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{ name: "deepseek", settingsConfig: input }}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "Save explicit DeepSeek" }),
    );

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toEqual(input);
  });

  it("does not infer preset credential requirements from an existing provider ID", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const input = {
      oauth: "example",
      futureField: { keep: true },
    };

    render(
      <PiProviderForm
        appId="pi"
        providerId="cc-switch-kimi"
        submitLabel="Save external Kimi node"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{ name: "External Kimi node", settingsConfig: input }}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "Save external Kimi node" }),
    );

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toEqual(input);
  });

  it("keeps models absent when an edit removes the native models field in JSON", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);

    render(
      <PiProviderForm
        appId="pi"
        providerId="openai"
        submitLabel="Save explicit OpenAI"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{
          name: "openai",
          settingsConfig: {
            models: [{ id: "gpt-5.6-sol" }],
            futureField: { keep: true },
          },
        }}
      />,
    );

    const configEditor = screen.getByLabelText(
      "provider.configJson",
    ) as HTMLTextAreaElement;
    fireEvent.change(configEditor, {
      target: {
        value: JSON.stringify({ futureField: { keep: true } }, null, 2),
      },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Save explicit OpenAI" }),
    );

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toEqual({
      futureField: { keep: true },
    });
  });

  it("round-trips Pi fields that are not exposed by the form", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const input = {
      name: "Provider with native options",
      baseUrl: "https://api.example.com/v1",
      apiKey: "test-key",
      api: "openai-responses",
      headers: { "x-provider-field": "provider-value" },
      models: [
        {
          id: "model",
          name: "Model",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 128_000,
          maxTokens: 16_384,
          samplingParams: {
            temperature: 0.7,
            top_p: 0.9,
          },
        },
      ],
      nativeOptionNotExposedByCcSwitch: {
        enabled: true,
        value: 3,
      },
    };

    render(
      <PiProviderForm
        appId="pi"
        providerId="native-options"
        submitLabel="Save native options"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{
          name: String(input.name),
          settingsConfig: input,
        }}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "Save native options" }),
    );

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toEqual(input);
  });

  it("preserves exact model IDs instead of trimming them", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const input = {
      name: "Exact IDs",
      api: "openai-responses",
      baseUrl: "https://api.example.com/v1",
      models: [completeModel(" "), completeModel(" model ")],
    };

    render(
      <PiProviderForm
        appId="pi"
        providerId="exact-ids"
        submitLabel="Save exact IDs"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{ name: input.name, settingsConfig: input }}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Save exact IDs" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toEqual(input);
  });

  it("preserves explicitly false native fields instead of erasing them", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const input = {
      name: "Explicit false",
      api: "openai-responses",
      baseUrl: "https://api.example.com/v1",
      authHeader: false,
      models: [completeModel("model", "Model")],
    };

    render(
      <PiProviderForm
        appId="pi"
        providerId="explicit-false"
        submitLabel="Save explicit false"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{ name: input.name, settingsConfig: input }}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "Save explicit false" }),
    );
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toEqual(input);
  });

  it("preserves an absent provider-level API until the user changes it", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const input = {
      name: "Inherited API",
      baseUrl: "https://api.example.com/v1",
      models: [completeModel("model", "Model")],
    };

    render(
      <PiProviderForm
        appId="pi"
        providerId="inherited-api"
        submitLabel="Save inherited API"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{ name: input.name, settingsConfig: input }}
      />,
    );

    expect(document.querySelector("#pi-provider-api-select")).toHaveTextContent(
      "OpenAI Chat Completions",
    );
    const preview = JSON.parse(
      (screen.getByLabelText("provider.configJson") as HTMLTextAreaElement)
        .value,
    );
    expect(preview).not.toHaveProperty("api");

    fireEvent.click(screen.getByRole("button", { name: "Save inherited API" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toEqual(input);
  });

  it("preserves an absent optional native provider name on a no-op edit", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const input = {
      baseUrl: "https://api.example.com/v1",
      models: [completeModel("model", "Model")],
    };

    render(
      <PiProviderForm
        appId="pi"
        providerId="unnamed-provider"
        submitLabel="Save unnamed provider"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{ name: "CC Switch label", settingsConfig: input }}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "Save unnamed provider" }),
    );
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toEqual(input);
  });

  it("preserves an independent native provider name on a no-op edit", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const input = {
      name: "Pi native label",
      baseUrl: "https://api.example.com/v1",
      models: [completeModel("model", "Model")],
    };

    render(
      <PiProviderForm
        appId="pi"
        providerId="independent-name-provider"
        submitLabel="Save independent name"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{ name: "CC Switch label", settingsConfig: input }}
      />,
    );

    expect(screen.getByLabelText("provider.name")).toHaveValue(
      "CC Switch label",
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Save independent name" }),
    );
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toEqual(input);
  });

  it("edits a native provider without adding snapshot ownership state", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const initialConfig = {
      name: "Managed",
      api: "openai-completions",
      baseUrl: "https://api.example.com/v1",
      models: [
        completeModel("model-a", "Model A"),
        completeModel("model-b", "Model B"),
      ],
      futureField: { preserve: true },
    };

    render(
      <PiProviderForm
        appId="pi"
        providerId="managed"
        submitLabel="Save managed provider"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{ name: "Managed", settingsConfig: initialConfig }}
      />,
    );

    for (const button of screen.getAllByRole("button", {
      name: "pi.form.removeModel",
    })) {
      expect(button).toBeEnabled();
    }
    fireEvent.change(screen.getAllByLabelText("pi.form.modelId")[0], {
      target: { value: "model-a-renamed" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Save managed provider" }),
    );

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit.mock.calls[0][0]).not.toHaveProperty(
      "expectedSettingsConfig",
    );
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toMatchObject({
      futureField: { preserve: true },
      models: [{ id: "model-a-renamed" }, { id: "model-b" }],
    });
  });

  it("does not expose failover endpoint controls", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);

    render(
      <PiProviderForm
        appId="pi"
        providerId="managed"
        submitLabel="Save managed provider"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{
          name: "Managed",
          settingsConfig: {
            name: "Managed",
            api: "openai-responses",
            baseUrl: "https://api.example.com/v1",
            models: [completeModel("model", "Model")],
          },
          meta: {
            endpointAutoSelect: true,
            custom_endpoints: {
              "https://failover.example/v1": {
                url: "https://failover.example/v1",
                addedAt: 1,
              },
            },
          },
        }}
      />,
    );

    expect(
      screen.queryByRole("button", { name: "pi.form.manageEndpoints" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("https://failover.example/v1"),
    ).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Save managed provider" }),
    );

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toMatchObject({
      baseUrl: "https://api.example.com/v1",
      models: [{ id: "model" }],
    });
  });
});
