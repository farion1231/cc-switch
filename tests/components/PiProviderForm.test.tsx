import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PiProviderForm } from "@/components/providers/forms/PiProviderForm";
import { http, HttpResponse } from "msw";
import { server } from "../msw/server";
import { renderWithQueryClient as render } from "../utils/testQueryClient";
import { MODELS_DEV_API_URL } from "@/lib/modelsDev";
import { piThinkingProfiles } from "@/config/piThinkingProfiles";
import { piProviderPresets } from "@/config/piProviderPresets";

const TAURI_ENDPOINT = "http://tauri.local";

type User = ReturnType<typeof userEvent.setup>;

// 拉到模型会自动打开勾选弹窗；关掉它，接着测每行的下拉。
async function fetchModelsAndClosePicker(user: User) {
  await user.click(
    screen.getByRole("button", { name: "providerForm.fetchModels" }),
  );
  await screen.findByRole("dialog");
  await user.keyboard("{Escape}");
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
}

function completeModel(id: string, name = id.trim() || "Model") {
  return {
    id,
    name,
    reasoning: false,
    input: ["text"],
    contextWindow: 128_000,
    maxTokens: 16_384,
  };
}

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

// 请求头、模型自动发现和接口兼容性收在默认折叠的「高级选项」里
function openAdvancedOptions() {
  fireEvent.click(
    screen.getByRole("button", { name: "providerForm.advancedOptionsToggle" }),
  );
}

describe("PiProviderForm", () => {
  beforeEach(() => {
    Element.prototype.scrollIntoView = vi.fn();
  });

  it("shows a preset's provider key and blocks saving when it is already taken", async () => {
    const user = userEvent.setup();
    const preset = piProviderPresets.find((entry) => entry.name === "Kimi")!;
    server.use(
      http.post("http://tauri.local/get_providers", () =>
        HttpResponse.json({
          [preset.providerKey]: {
            id: preset.providerKey,
            name: "Kimi",
            settingsConfig: {},
          },
        }),
      ),
    );
    const onSubmit = vi.fn();
    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save Pi provider"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );
    await user.click(
      screen.getByRole("button", { name: /^providerPreset\.group\.vendor / }),
    );
    await user.click(screen.getByRole("button", { name: "Kimi" }));
    expect(screen.getByLabelText("pi.form.providerKey")).toHaveValue(
      preset.providerKey,
    );
    expect(
      await screen.findByText("pi.form.providerKeyDuplicate"),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Save Pi provider" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "pi.form.providerKeyDuplicate",
    );
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("checks provider keys against the current app only", async () => {
    const user = userEvent.setup();
    server.use(
      http.post("http://tauri.local/get_providers", async ({ request }) => {
        const { app } = (await request.json()) as { app: string };
        const key = app === "pi" ? "pi-only" : "omp-only";
        return HttpResponse.json({
          [key]: { id: key, name: key, settingsConfig: {} },
        });
      }),
    );
    render(
      <PiProviderForm
        appId="omp"
        submitLabel="Save omp provider"
        onSubmit={() => {}}
        onCancel={() => {}}
      />,
    );
    const keyInput = screen.getByLabelText("pi.form.providerKey");

    // Pi 已有的标识在 omp 里可以用
    await user.clear(keyInput);
    await user.type(keyInput, "pi-only");
    await waitFor(() =>
      expect(
        screen.queryByText("pi.form.providerKeyDuplicate"),
      ).not.toBeInTheDocument(),
    );

    await user.clear(keyInput);
    await user.type(keyInput, "omp-only");
    expect(
      await screen.findByText("pi.form.providerKeyDuplicate"),
    ).toBeInTheDocument();
  });

  it("starts in the same editable custom state as OpenCode", async () => {
    const onSubmitReadyChange = vi.fn();
    const { container } = render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save preset"
        onSubmit={() => {}}
        onCancel={() => {}}
        onSubmitReadyChange={onSubmitReadyChange}
      />,
    );

    // 表单直接铺在添加页里，不再套一层卡片
    expect(container.querySelector("#provider-form")).not.toHaveClass("glass");
    expect(screen.getByLabelText("provider.name")).toBeInTheDocument();
    expect(screen.getByLabelText("provider.notes")).toBeInTheDocument();
    expect(screen.getByLabelText("provider.websiteUrl")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save preset" })).toBeEnabled();
    await waitFor(() =>
      expect(onSubmitReadyChange).toHaveBeenLastCalledWith(true),
    );
    expect(screen.queryByText("pi.form.stepPreset")).not.toBeInTheDocument();
    expect(screen.queryByText("pi.form.stepAuth")).not.toBeInTheDocument();
    expect(screen.queryByText("pi.form.stepModel")).not.toBeInTheDocument();
  });

  it("uses the OpenCode-style provider hierarchy without per-model endpoints", () => {
    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save provider hierarchy"
        onSubmit={() => {}}
        onCancel={() => {}}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    const apiFormat = document.querySelector("#pi-provider-api-select");
    const apiKey = screen.getByLabelText("pi.form.credential");
    const baseUrl = screen.getByLabelText("Base URL");
    const advanced = screen.getByRole("button", {
      name: "providerForm.advancedOptionsToggle",
    });
    const models = screen.getByText("模型配置");

    expect(apiFormat).toHaveAttribute("role", "combobox");
    expect(apiFormat?.compareDocumentPosition(apiKey)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(apiKey.compareDocumentPosition(baseUrl)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(baseUrl.compareDocumentPosition(models)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    // 高级选项和 Claude Code、Codex 一样排在模型配置后面
    expect(models.compareDocumentPosition(advanced)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    // 请求头和兼容性收在高级选项里，默认不展开
    expect(advanced).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("Headers")).not.toBeInTheDocument();
    expect(screen.queryByText("pi.form.compatibility")).not.toBeInTheDocument();

    openAdvancedOptions();
    const headers = screen.getByText("Headers");
    const compatibility = screen.getByText("pi.form.compatibility");
    expect(headers.compareDocumentPosition(compatibility)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(models.compareDocumentPosition(headers)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );

    fireEvent.click(screen.getByRole("button", { name: "pi.form.addModel" }));

    expect(screen.getByText("接口格式")).toBeInTheDocument();
    expect(screen.queryByText("自定义接口格式")).not.toBeInTheDocument();
    expect(screen.getByLabelText("pi.form.modelId")).toBeInTheDocument();
    expect(screen.getByLabelText("pi.form.modelName")).toBeInTheDocument();
    expect(screen.queryByText("pi.form.modelApi")).not.toBeInTheDocument();
    expect(screen.queryByText("pi.form.modelBaseUrl")).not.toBeInTheDocument();
    expect(models).not.toHaveClass("font-medium");
    expect(headers).toHaveClass("font-medium");
  });

  it("uses the shared request-header hierarchy while config JSON stays visible", async () => {
    const user = userEvent.setup();
    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save progressive fields"
        onSubmit={() => {}}
        onCancel={() => {}}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );

    openAdvancedOptions();
    expect(screen.getByText("Headers")).toBeInTheDocument();
    expect(
      screen.getByText("No custom headers configured"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Optional HTTP headers sent with provider requests, such as HTTP-Referer or X-Title.",
      ),
    ).toBeInTheDocument();
    expect(
      document.querySelector("#pi-header-identity"),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Add header" }),
    ).toBeInTheDocument();
    expect(screen.getByText("pi.form.compatibility")).toBeInTheDocument();
    expect(
      within(
        document.querySelector("#pi-provider-compat") as HTMLElement,
      ).getByRole("button", { name: "pi.form.compatibility" }),
    ).toHaveAttribute("aria-expanded", "false");
    expect(
      screen.getByText("pi.form.noCompatibilityOptions"),
    ).toBeInTheDocument();
    expect(
      screen.queryByLabelText(/pi\.form\.modelAdditionalConfig/),
    ).not.toBeInTheDocument();
    expect(screen.getByText("provider.configJson")).toBeInTheDocument();
    expect(screen.getByLabelText("provider.configJson")).not.toHaveAttribute(
      "readonly",
    );

    await user.click(screen.getByRole("button", { name: "Add header" }));
    expect(screen.getByText("Headers")).toBeInTheDocument();
    expect(screen.getByLabelText("Header")).toBeInTheDocument();
    expect(screen.getByLabelText("Value")).toBeInTheDocument();
  });

  it("edits provider compatibility options and keeps JSON synchronized", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(
      <PiProviderForm
        appId="pi"
        providerId="compat-provider"
        submitLabel="Save compatibility"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{
          name: "Compatibility provider",
          settingsConfig: { name: "Compatibility provider" },
        }}
      />,
    );

    openAdvancedOptions();
    const compatibilitySection = document.querySelector("#pi-provider-compat");
    expect(compatibilitySection).not.toBeNull();

    await user.click(
      within(compatibilitySection as HTMLElement).getByRole("button", {
        name: "pi.form.addCompatibilityOption",
      }),
    );
    const optionKey = screen.getByLabelText("pi.form.optionKey");
    await user.type(optionKey, "supportsDeveloperRole");
    await user.tab();
    const optionValue = screen.getByLabelText("pi.form.optionValue");
    expect(optionValue).toHaveFocus();
    await user.type(optionValue, "false");
    await user.tab();
    expect(
      within(compatibilitySection as HTMLElement).getByRole("button", {
        name: "pi.form.removeCompatibilityOption",
      }),
    ).toHaveFocus();
    await user.tab();

    const preview = screen.getByLabelText(
      "provider.configJson",
    ) as HTMLTextAreaElement;
    await waitFor(() =>
      expect(JSON.parse(preview.value)).toMatchObject({
        compat: {
          supportsDeveloperRole: false,
        },
      }),
    );

    fireEvent.click(screen.getByRole("button", { name: "Save compatibility" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toMatchObject({
      compat: {
        supportsDeveloperRole: false,
      },
    });
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

  it("stores Pi-native request headers without mixing them with API-key auth", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save identity"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    fireEvent.change(screen.getByPlaceholderText("my-provider"), {
      target: { value: "identity-provider" },
    });
    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "Identity provider" },
    });
    await user.click(document.querySelector("#pi-provider-api-select")!);
    await user.click(
      await screen.findByRole("option", { name: "Anthropic Messages" }),
    );
    fireEvent.change(
      screen.getByPlaceholderText("https://api.example.com/v1"),
      {
        target: { value: "https://api.example.com" },
      },
    );
    fireEvent.click(screen.getByRole("button", { name: "pi.form.addModel" }));
    fireEvent.change(screen.getByPlaceholderText("model-id"), {
      target: { value: "identity-model" },
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

    openAdvancedOptions();
    await user.click(screen.getByRole("button", { name: "Add header" }));
    const headerName = screen.getByLabelText("Header");
    fireEvent.change(headerName, {
      target: { value: "X-Client-Name" },
    });
    fireEvent.blur(headerName);
    fireEvent.change(screen.getByLabelText("Value"), {
      target: { value: "pi-ui" },
    });
    await user.click(screen.getByRole("button", { name: "Save identity" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const config = JSON.parse(onSubmit.mock.calls[0][0].settingsConfig);
    expect(config.headers).toEqual({ "X-Client-Name": "pi-ui" });
    expect(config).not.toHaveProperty("authHeader");
    expect(config.headers).not.toHaveProperty("authorization");
    expect(config.headers).not.toHaveProperty("x-api-key");
  });

  it("echoes existing Pi headers and preserves them when saving", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const input = {
      name: "Existing headers",
      api: "openai-completions",
      baseUrl: "https://api.example.com/v1",
      headers: {
        "HTTP-Referer": "https://cc-switch.example",
        "X-Title": "CC Switch",
      },
      models: [completeModel("model-a", "Model A")],
    };

    render(
      <PiProviderForm
        appId="pi"
        providerId="existing-headers"
        submitLabel="Save existing headers"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{ name: input.name, settingsConfig: input }}
      />,
    );

    openAdvancedOptions();
    expect(
      document.querySelector("#pi-header-identity"),
    ).not.toBeInTheDocument();
    expect(
      screen
        .getAllByLabelText("Header")
        .map((element) => element.getAttribute("value")),
    ).toEqual(["HTTP-Referer", "X-Title"]);
    expect(
      screen
        .getAllByLabelText("Value")
        .map((element) => element.getAttribute("value")),
    ).toEqual(["https://cc-switch.example", "CC Switch"]);

    fireEvent.click(
      screen.getByRole("button", { name: "Save existing headers" }),
    );

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toEqual(input);
  });

  it("applies a maintained preset without creating a Pi-owned provider key", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save preset"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );

    fireEvent.click(screen.getByText("Kimi", { selector: "span" }));
    fireEvent.change(screen.getByLabelText("pi.form.credential"), {
      target: { value: "literal-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save preset" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit.mock.calls[0][0]).toMatchObject({
      providerKey: "cc-switch-kimi",
      name: "Kimi",
      presetCategory: "cn_official",
    });
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toMatchObject({
      api: "openai-completions",
      baseUrl: "https://api.moonshot.cn/v1",
      apiKey: "literal-key",
    });
    expect(
      screen.queryByText("pi.form.nativeLoginAlternative"),
    ).not.toBeInTheDocument();
  });

  it("drops injectV1 when omp discovery leaves openai-models-list", async () => {
    const user = userEvent.setup();
    render(
      <PiProviderForm
        appId="omp"
        submitLabel="Save omp provider"
        onSubmit={() => {}}
        onCancel={() => {}}
        initialData={{
          name: "gateway",
          settingsConfig: {
            baseUrl: "https://gateway.example.com/v3/compat",
            api: "openai-completions",
            discovery: {
              type: "openai-models-list",
              injectV1: false,
              timeoutMs: 500,
            },
          },
        }}
      />,
    );
    const configEditor = screen.getByLabelText(
      "provider.configJson",
    ) as HTMLTextAreaElement;

    // omp 只允许 openai-models-list 带 injectV1，其余子键照旧保留
    openAdvancedOptions();
    await user.click(document.querySelector("#omp-discovery-type")!);
    await user.click(
      await screen.findByRole("option", {
        name: "omp.form.discoveryTypes.ollama",
      }),
    );
    expect(JSON.parse(configEditor.value).discovery).toEqual({
      type: "ollama",
      timeoutMs: 500,
    });
  });

  it("writes omp presets without Pi-only fields and edits discovery", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(
      <PiProviderForm
        appId="omp"
        submitLabel="Save omp preset"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );

    fireEvent.click(screen.getByText("Kimi", { selector: "span" }));
    const configEditor = screen.getByLabelText(
      "provider.configJson",
    ) as HTMLTextAreaElement;
    const preset = JSON.parse(configEditor.value);
    // omp 的 models.yml 没有供应商级 name，也没有 thinkingLevelMap
    expect(preset).not.toHaveProperty("name");
    expect(preset.models.length).toBeGreaterThan(0);
    for (const model of preset.models) {
      expect(model).not.toHaveProperty("thinkingLevelMap");
    }

    expect(document.querySelector("#omp-discovery-type")).toBeNull();
    openAdvancedOptions();
    await user.click(document.querySelector("#omp-discovery-type")!);
    await user.click(
      await screen.findByRole("option", {
        name: "omp.form.discoveryTypes.openaiModelsList",
      }),
    );
    expect(JSON.parse(configEditor.value).discovery).toEqual({
      type: "openai-models-list",
    });

    await user.click(document.querySelector("#omp-discovery-type")!);
    await user.click(
      await screen.findByRole("option", { name: "omp.form.discoveryOff" }),
    );
    expect(JSON.parse(configEditor.value)).not.toHaveProperty("discovery");

    fireEvent.change(screen.getByLabelText("pi.form.credential"), {
      target: { value: "literal-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save omp preset" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(
      JSON.parse(onSubmit.mock.calls[0][0].settingsConfig),
    ).not.toHaveProperty("name");
  });

  it("keeps preset model order without exposing a default-model field", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save preset"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );

    fireEvent.click(screen.getByText("Kimi", { selector: "span" }));
    fireEvent.change(screen.getByLabelText("pi.form.credential"), {
      target: { value: "literal-key" },
    });
    expect(
      document.querySelector("#pi-activation-model"),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save preset" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const submitted = onSubmit.mock.calls[0][0];
    const config = JSON.parse(submitted.settingsConfig);
    expect(config.models.map((model: { id: string }) => model.id)).toEqual([
      "kimi-k2.7-code",
      "kimi-k3",
      "kimi-k2.7-code-highspeed",
      "kimi-k2.6",
    ]);
    expect(
      config.models.map((model: { id: string; name?: string }) => ({
        id: model.id,
        name: model.name,
      })),
    ).toEqual([
      { id: "kimi-k2.7-code", name: "Kimi K2.7 Code" },
      { id: "kimi-k3", name: "Kimi K3" },
      { id: "kimi-k2.7-code-highspeed", name: "Kimi K2.7 Code HighSpeed" },
      { id: "kimi-k2.6", name: "Kimi K2.6" },
    ]);
    for (const model of config.models) {
      expect(model).toMatchObject({
        reasoning: true,
        input: ["text", "image"],
      });
      expect(model.contextWindow).toBeGreaterThan(0);
      expect(model.maxTokens).toBeGreaterThan(0);
    }
    expect(submitted).not.toHaveProperty("piActivateModelId");
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

  it("renders validation errors in the form and focuses the invalid field", async () => {
    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save invalid preset"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );

    fireEvent.click(screen.getByText("Kimi", { selector: "span" }));
    fireEvent.click(
      screen.getByRole("button", { name: "Save invalid preset" }),
    );

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("pi.form.credentialRequired");
    await waitFor(() =>
      expect(screen.getByLabelText("pi.form.credential")).toHaveFocus(),
    );
  });

  it("reuses the shared model fetch command and lets the user select a real result", async () => {
    const user = userEvent.setup();
    let requestBody: Record<string, unknown> | undefined;
    server.use(
      http.post(
        `${TAURI_ENDPOINT}/fetch_models_for_config`,
        async ({ request }) => {
          requestBody = (await request.json()) as Record<string, unknown>;
          return HttpResponse.json([
            { id: "remote-model-a", ownedBy: "remote" },
            { id: "remote-model-b", ownedBy: "remote" },
          ]);
        },
      ),
    );

    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save fetched provider"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    fireEvent.change(screen.getByLabelText("pi.form.credential"), {
      target: { value: "literal-key" },
    });
    fireEvent.change(
      screen.getByPlaceholderText("https://api.example.com/v1"),
      {
        target: { value: "https://models.example/v1" },
      },
    );
    fireEvent.click(screen.getByRole("button", { name: "pi.form.addModel" }));
    await user.click(
      screen.getByRole("button", { name: "展开或收起模型详情" }),
    );
    fireEvent.change(screen.getByLabelText("pi.form.maxTokens"), {
      target: { value: "777" },
    });
    openAdvancedOptions();
    await user.click(screen.getByRole("button", { name: "Add header" }));
    const headerName = screen.getByLabelText("Header");
    fireEvent.change(headerName, { target: { value: "user-agent" } });
    fireEvent.blur(headerName);
    fireEvent.change(screen.getByLabelText("Value"), {
      target: { value: "pi-test-agent/1.0" },
    });
    await fetchModelsAndClosePicker(user);

    await waitFor(() =>
      expect(requestBody).toEqual({
        baseUrl: "https://models.example/v1",
        apiKey: "literal-key",
        customUserAgent: "pi-test-agent/1.0",
        apiFormat: "openai-completions",
        requestHeaders: {
          "user-agent": "pi-test-agent/1.0",
        },
      }),
    );

    const modelIdInput = screen.getByLabelText("pi.form.modelId");
    await user.click(
      within(modelIdInput.parentElement as HTMLElement).getByRole("button"),
    );
    await user.click(
      await screen.findByRole("option", { name: "remote-model-b" }),
    );
    expect(modelIdInput).toHaveValue("remote-model-b");
    expect(screen.getByLabelText("pi.form.modelName")).toHaveValue(
      "remote-model-b",
    );
    expect(screen.getByLabelText("pi.form.maxTokens")).toHaveValue(777);
    expect(screen.getByLabelText("pi.form.contextWindow")).toHaveValue(null);
  });

  it("ignores a stale model-list response after the request config changes", async () => {
    const user = userEvent.setup();
    let slowRequestStarted = false;
    let releaseSlowRequest: (() => void) | undefined;
    const slowRequestGate = new Promise<void>((resolve) => {
      releaseSlowRequest = resolve;
    });
    server.use(
      http.post(
        `${TAURI_ENDPOINT}/fetch_models_for_config`,
        async ({ request }) => {
          const body = (await request.json()) as { baseUrl?: string };
          if (body.baseUrl === "https://slow.example/v1") {
            slowRequestStarted = true;
            await slowRequestGate;
            return HttpResponse.json([{ id: "stale-model" }]);
          }
          return HttpResponse.json([{ id: "current-model" }]);
        },
      ),
    );

    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save race-safe provider"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    fireEvent.change(screen.getByLabelText("pi.form.credential"), {
      target: { value: "literal-key" },
    });
    const endpoint = screen.getByPlaceholderText("https://api.example.com/v1");
    fireEvent.change(endpoint, {
      target: { value: "https://slow.example/v1" },
    });
    await user.click(screen.getByRole("button", { name: "pi.form.addModel" }));
    await user.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await waitFor(() => expect(slowRequestStarted).toBe(true));

    fireEvent.change(endpoint, {
      target: { value: "https://current.example/v1" },
    });
    await fetchModelsAndClosePicker(user);

    const modelIdInput = screen.getByLabelText("pi.form.modelId");
    await waitFor(() =>
      expect(
        within(modelIdInput.parentElement as HTMLElement).getByRole("button"),
      ).toBeInTheDocument(),
    );
    await user.click(
      within(modelIdInput.parentElement as HTMLElement).getByRole("button"),
    );
    await user.click(
      await screen.findByRole("option", { name: "current-model" }),
    );
    expect(modelIdInput).toHaveValue("current-model");

    releaseSlowRequest?.();
    await waitFor(() => expect(modelIdInput).toHaveValue("current-model"));
    await user.click(
      within(modelIdInput.parentElement as HTMLElement).getByRole("button"),
    );
    expect(
      await screen.findByRole("option", { name: "current-model" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("option", { name: "stale-model" }),
    ).not.toBeInTheDocument();
  });

  it("uses fetched model ownership only as display data, never as capability metadata", async () => {
    const user = userEvent.setup();
    server.use(
      http.post(`${TAURI_ENDPOINT}/fetch_models_for_config`, () =>
        HttpResponse.json([{ id: "first", ownedBy: "provider-a" }]),
      ),
    );

    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save owner-safe provider"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    await user.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    fireEvent.change(screen.getByLabelText("pi.form.credential"), {
      target: { value: "literal-key" },
    });
    fireEvent.change(
      screen.getByPlaceholderText("https://api.example.com/v1"),
      {
        target: { value: "https://provider-a.example/v1" },
      },
    );
    await user.click(screen.getByRole("button", { name: "pi.form.addModel" }));
    await fetchModelsAndClosePicker(user);
    const modelIdInput = screen.getByLabelText("pi.form.modelId");
    await user.click(
      within(modelIdInput.parentElement as HTMLElement).getByRole("button"),
    );
    await user.click(await screen.findByRole("option", { name: "first" }));
    await waitFor(() =>
      expect(screen.getByLabelText("pi.form.modelName")).toHaveValue("first"),
    );

    fireEvent.change(modelIdInput, { target: { value: "shared" } });
    await waitFor(() =>
      expect(screen.getByLabelText("pi.form.modelName")).toHaveValue("shared"),
    );
    await user.click(
      screen.getByRole("button", { name: "展开或收起模型详情" }),
    );
    expect(screen.getByLabelText("pi.form.reasoning")).not.toBeChecked();
    expect(screen.getByLabelText("pi.form.contextWindow")).toHaveValue(null);
    expect(screen.getByLabelText("pi.form.maxTokens")).toHaveValue(null);
  });

  it("keeps unknown fetched models on Pi defaults when the endpoint changes", async () => {
    const user = userEvent.setup();
    server.use(
      http.post(`${TAURI_ENDPOINT}/fetch_models_for_config`, () =>
        HttpResponse.json([{ id: "shared", ownedBy: "provider-a" }]),
      ),
    );

    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save endpoint-safe provider"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    await user.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    fireEvent.change(screen.getByLabelText("pi.form.credential"), {
      target: { value: "literal-key" },
    });
    const endpoint = screen.getByPlaceholderText("https://api.example.com/v1");
    fireEvent.change(endpoint, {
      target: { value: "https://provider-a.example/v1" },
    });
    await user.click(screen.getByRole("button", { name: "pi.form.addModel" }));
    await fetchModelsAndClosePicker(user);
    const modelIdInput = screen.getByLabelText("pi.form.modelId");
    await user.click(
      within(modelIdInput.parentElement as HTMLElement).getByRole("button"),
    );
    await user.click(await screen.findByRole("option", { name: "shared" }));
    await waitFor(() =>
      expect(screen.getByLabelText("pi.form.modelName")).toHaveValue("shared"),
    );

    fireEvent.change(endpoint, {
      target: { value: "https://provider-b.example/v1" },
    });
    await waitFor(() =>
      expect(screen.getByLabelText("pi.form.modelName")).toHaveValue("shared"),
    );
    await user.click(
      screen.getByRole("button", { name: "展开或收起模型详情" }),
    );
    expect(screen.getByLabelText("pi.form.reasoning")).not.toBeChecked();
    expect(screen.getByLabelText("pi.form.contextWindow")).toHaveValue(null);
    expect(screen.getByLabelText("pi.form.maxTokens")).toHaveValue(null);
  });

  it("does not enrich an id-only legacy model from fetched ownership data", async () => {
    const user = userEvent.setup();
    server.use(
      http.post(`${TAURI_ENDPOINT}/fetch_models_for_config`, () =>
        HttpResponse.json([{ id: "shared", ownedBy: "provider-a" }]),
      ),
    );

    render(
      <PiProviderForm
        appId="pi"
        providerId="legacy-provider"
        submitLabel="Save legacy provider"
        onSubmit={vi.fn()}
        onCancel={() => {}}
        initialData={{
          name: "Legacy provider",
          settingsConfig: {
            name: "Legacy provider",
            baseUrl: "https://provider-a.example/v1",
            apiKey: "literal-key",
            api: "openai-completions",
            models: [{ id: "shared" }],
          },
        }}
      />,
    );
    await fetchModelsAndClosePicker(user);
    const modelIdInput = screen.getByLabelText("pi.form.modelId");
    await user.click(
      within(modelIdInput.parentElement as HTMLElement).getByRole("button"),
    );
    await user.click(await screen.findByRole("option", { name: "shared" }));
    expect(screen.getByLabelText("pi.form.modelName")).toHaveValue("");

    fireEvent.change(
      screen.getByPlaceholderText("https://api.example.com/v1"),
      {
        target: { value: "https://provider-b.example/v1" },
      },
    );
    const configEditor = screen.getByLabelText(
      "provider.configJson",
    ) as HTMLTextAreaElement;
    await waitFor(() =>
      expect(JSON.parse(configEditor.value).models).toEqual([{ id: "shared" }]),
    );
    expect(screen.getByLabelText("pi.form.modelName")).toHaveValue("");
  });

  it("leaves a fetched model unknown to presets and models.dev on Pi defaults", async () => {
    const user = userEvent.setup();
    server.use(
      http.post(`${TAURI_ENDPOINT}/fetch_models_for_config`, () =>
        HttpResponse.json([{ id: "gpt-5.6-sol", ownedBy: "proxy" }]),
      ),
    );

    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save manual provider"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    fireEvent.change(screen.getByPlaceholderText("my-provider"), {
      target: { value: "autofilled-provider" },
    });
    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "Autofilled provider" },
    });
    fireEvent.change(screen.getByLabelText("pi.form.credential"), {
      target: { value: "literal-key" },
    });
    fireEvent.change(
      screen.getByPlaceholderText("https://api.example.com/v1"),
      {
        target: { value: "https://api.example.com/v1" },
      },
    );
    await user.click(screen.getByRole("button", { name: "pi.form.addModel" }));
    await fetchModelsAndClosePicker(user);

    const modelIdInput = screen.getByLabelText("pi.form.modelId");
    await user.click(
      within(modelIdInput.parentElement as HTMLElement).getByRole("button"),
    );
    await user.click(
      await screen.findByRole("option", { name: "gpt-5.6-sol" }),
    );

    const modelNameInput = screen.getByLabelText("pi.form.modelName");
    await waitFor(() => expect(modelNameInput).toHaveValue("gpt-5.6-sol"));
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

  it("fills the capabilities of a fetched model known to models.dev", async () => {
    const user = userEvent.setup();
    server.use(
      http.post(`${TAURI_ENDPOINT}/fetch_models_for_config`, () =>
        HttpResponse.json([{ id: "gpt-5.6-sol", ownedBy: "proxy" }]),
      ),
      http.get(MODELS_DEV_API_URL, () =>
        HttpResponse.json({
          example: {
            api: "https://api.example.com/v1",
            models: {
              "gpt-5.6-sol": {
                reasoning: true,
                reasoning_options: [
                  {
                    type: "effort",
                    values: ["low", "medium", "high", "xhigh"],
                  },
                ],
                modalities: { input: ["text", "image"], output: ["text"] },
                limit: { context: 400000, output: 128000 },
              },
            },
          },
        }),
      ),
    );

    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save manual provider"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    fireEvent.change(screen.getByPlaceholderText("my-provider"), {
      target: { value: "autofilled-provider" },
    });
    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "Autofilled provider" },
    });
    fireEvent.change(screen.getByLabelText("pi.form.credential"), {
      target: { value: "literal-key" },
    });
    fireEvent.change(
      screen.getByPlaceholderText("https://api.example.com/v1"),
      {
        target: { value: "https://api.example.com/v1" },
      },
    );
    await user.click(screen.getByRole("button", { name: "pi.form.addModel" }));
    await fetchModelsAndClosePicker(user);

    const modelIdInput = screen.getByLabelText("pi.form.modelId");
    await user.click(
      within(modelIdInput.parentElement as HTMLElement).getByRole("button"),
    );
    await user.click(
      await screen.findByRole("option", { name: "gpt-5.6-sol" }),
    );

    const modelNameInput = screen.getByLabelText("pi.form.modelName");
    await waitFor(() => expect(modelNameInput).toHaveValue("gpt-5.6-sol"));
    await user.click(
      screen.getByRole("button", { name: "展开或收起模型详情" }),
    );
    // models.dev 按地址认出了这家，GPT 窗口照用它登记的值，不封顶。
    await waitFor(() =>
      expect(screen.getByLabelText("pi.form.contextWindow")).toHaveValue(
        400000,
      ),
    );
    expect(screen.getByLabelText("pi.form.maxTokens")).toHaveValue(128000);
    expect(screen.getByLabelText("pi.form.reasoning")).toBeChecked();
    expect(screen.getByLabelText("pi.form.imageInput")).toBeChecked();
    // Chat Completions 会把档位原样作为 reasoning_effort 发出，按 Pi 官方规则生成映射。
    const configEditor = screen.getByLabelText(
      "provider.configJson",
    ) as HTMLTextAreaElement;
    expect(JSON.parse(configEditor.value).models[0].thinkingLevelMap).toEqual({
      off: null,
      minimal: null,
      low: "low",
      medium: "medium",
      high: "high",
      xhigh: "xhigh",
      max: null,
    });
  });

  it("adds the compat a preset thinking map depends on", async () => {
    const user = userEvent.setup();
    server.use(
      http.post(`${TAURI_ENDPOINT}/fetch_models_for_config`, () =>
        HttpResponse.json([{ id: "kimi-k3", ownedBy: "moonshot" }]),
      ),
      http.get(MODELS_DEV_API_URL, () => HttpResponse.error()),
    );

    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save manual provider"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    fireEvent.change(screen.getByPlaceholderText("my-provider"), {
      target: { value: "my-moonshot" },
    });
    fireEvent.change(screen.getByLabelText("pi.form.credential"), {
      target: { value: "literal-key" },
    });
    fireEvent.change(
      screen.getByPlaceholderText("https://api.example.com/v1"),
      { target: { value: "https://api.moonshot.cn/v1" } },
    );
    await user.click(screen.getByRole("button", { name: "pi.form.addModel" }));
    await fetchModelsAndClosePicker(user);
    const modelIdInput = screen.getByLabelText("pi.form.modelId");
    await user.click(
      within(modelIdInput.parentElement as HTMLElement).getByRole("button"),
    );
    await user.click(await screen.findByRole("option", { name: "kimi-k3" }));

    // Moonshot 的地址在 Pi 里默认不发 reasoning_effort，映射要靠预设的 compat 才生效。
    const configEditor = screen.getByLabelText(
      "provider.configJson",
    ) as HTMLTextAreaElement;
    await waitFor(() =>
      expect(JSON.parse(configEditor.value).models[0]).toMatchObject({
        thinkingLevelMap: piThinkingProfiles.kimi3.map,
        compat: { supportsReasoningEffort: true, thinkingFormat: "openai" },
      }),
    );
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
    });

    await user.click(document.querySelector("#pi-provider-api-select")!);
    await user.click(
      await screen.findByRole("option", { name: "OpenAI Responses" }),
    );
    expect(JSON.parse(configEditor.value).models[0].thinkingLevelMap).toEqual({
      off: null,
    });

    await user.click(document.querySelector("#pi-provider-api-select")!);
    await user.click(
      await screen.findByRole("option", {
        name: "OpenAI Chat Completions",
      }),
    );
    expect(JSON.parse(configEditor.value).models[0].thinkingLevelMap).toEqual({
      off: null,
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

  describe("fetched model picker", () => {
    function serveModels(ids: string[]) {
      server.use(
        http.post(`${TAURI_ENDPOINT}/fetch_models_for_config`, () =>
          HttpResponse.json(ids.map((id) => ({ id, ownedBy: "example" }))),
        ),
        http.get(MODELS_DEV_API_URL, () =>
          HttpResponse.json({
            example: {
              api: "https://api.example.com/v1",
              models: {
                "model-x": {
                  reasoning: true,
                  reasoning_options: [
                    { type: "effort", values: ["low", "medium", "high"] },
                  ],
                  modalities: { input: ["text", "image"], output: ["text"] },
                  limit: { context: 123456, output: 32000 },
                },
              },
            },
          }),
        ),
      );
    }

    async function fillCustomProvider(
      user: User,
      appId: "pi" | "omp",
    ): Promise<HTMLTextAreaElement> {
      render(
        <PiProviderForm
          appId={appId}
          submitLabel="Save picked provider"
          onSubmit={vi.fn()}
          onCancel={() => {}}
        />,
      );
      await user.click(
        screen.getByRole("button", { name: "providerPreset.custom" }),
      );
      fireEvent.change(screen.getByLabelText("pi.form.credential"), {
        target: { value: "literal-key" },
      });
      fireEvent.change(
        screen.getByPlaceholderText("https://api.example.com/v1"),
        { target: { value: "https://api.example.com/v1" } },
      );
      return screen.getByLabelText(
        "provider.configJson",
      ) as HTMLTextAreaElement;
    }

    async function pickModels(user: User, toggle: string[]) {
      await user.click(
        screen.getByRole("button", { name: "providerForm.fetchModels" }),
      );
      const dialog = await screen.findByRole("dialog");
      for (const id of toggle) {
        await user.click(within(dialog).getByRole("checkbox", { name: id }));
      }
      await user.click(
        within(dialog).getByRole("button", {
          name: "providerForm.modelPicker.apply",
        }),
      );
    }

    it("adds checked models with models.dev metadata and removes unchecked ones", async () => {
      const user = userEvent.setup();
      serveModels(["model-x", "model-y", "model-z"]);
      const configEditor = await fillCustomProvider(user, "pi");
      await user.click(
        screen.getByRole("button", { name: "pi.form.addModel" }),
      );
      fireEvent.change(screen.getByLabelText("pi.form.modelId"), {
        target: { value: "model-z" },
      });

      // model-z 已在列表里、默认勾着：取消勾选就是移除。
      await pickModels(user, ["model-x", "model-y", "model-z"]);

      await waitFor(() =>
        expect(JSON.parse(configEditor.value).models[0]).toMatchObject({
          contextWindow: 123456,
        }),
      );
      const models = JSON.parse(configEditor.value).models;
      expect(models.map((model: { id: string }) => model.id)).toEqual([
        "model-x",
        "model-y",
      ]);
      expect(models[0]).toMatchObject({
        name: "model-x",
        reasoning: true,
        input: ["text", "image"],
        contextWindow: 123456,
        maxTokens: 32000,
        thinkingLevelMap: {
          off: null,
          minimal: null,
          low: "low",
          medium: "medium",
          high: "high",
          xhigh: null,
          max: null,
        },
      });
      // models.dev 没有这款：留 Pi 默认值，窗口和输出上限由用户自己填。
      expect(models[1]).toMatchObject({
        name: "model-y",
        reasoning: false,
        input: ["text"],
      });
      expect(models[1]).not.toHaveProperty("contextWindow");
      expect(models[1]).not.toHaveProperty("thinkingLevelMap");
    });

    it("adds picked models to omp without a thinking map", async () => {
      const user = userEvent.setup();
      serveModels(["model-x"]);
      const configEditor = await fillCustomProvider(user, "omp");

      await pickModels(user, ["model-x"]);

      await waitFor(() =>
        expect(JSON.parse(configEditor.value).models[0]).toMatchObject({
          id: "model-x",
          reasoning: true,
          contextWindow: 123456,
        }),
      );
      expect(JSON.parse(configEditor.value).models[0]).not.toHaveProperty(
        "thinkingLevelMap",
      );
    });
  });
});
