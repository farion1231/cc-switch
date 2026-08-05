import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PiProviderForm } from "@/components/providers/forms/PiProviderForm";
import {
  MODELS_DEV_API_URL,
  MODELS_DEV_QUERY_KEY,
} from "@/lib/modelsDevPricing";
import { queryClient } from "@/lib/query";
import { http, HttpResponse } from "msw";
import { server } from "../msw/server";

const TAURI_ENDPOINT = "http://tauri.local";

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

describe("PiProviderForm", () => {
  beforeEach(() => {
    Element.prototype.scrollIntoView = vi.fn();
    queryClient.removeQueries({ queryKey: MODELS_DEV_QUERY_KEY });
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

    expect(container.querySelector("#provider-form")).toHaveClass(
      "glass",
      "rounded-xl",
      "p-6",
    );
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
    expect(
      screen.getByLabelText("providerForm.apiEndpoint"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "pi.form.addModel" }));

    expect(screen.getByText("接口格式")).toBeInTheDocument();
    expect(screen.queryByText("自定义接口格式")).not.toBeInTheDocument();
    expect(document.querySelector("#pi-provider-api-select")).toHaveAttribute(
      "role",
      "combobox",
    );
    expect(screen.getByLabelText("pi.form.modelId")).toBeInTheDocument();
    expect(screen.getByLabelText("pi.form.modelName")).toBeInTheDocument();
    expect(screen.queryByText("pi.form.modelApi")).not.toBeInTheDocument();
    expect(screen.queryByText("pi.form.modelBaseUrl")).not.toBeInTheDocument();
    expect(screen.getByText("模型配置")).toHaveClass("font-normal");
    expect(screen.getByText("Headers")).toHaveClass("font-normal");
  });

  it("keeps native headers compact while the family-style config JSON stays visible", async () => {
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

    expect(screen.getByText("Headers")).toBeInTheDocument();
    expect(
      screen.queryByText("No custom headers configured"),
    ).not.toBeInTheDocument();
    expect(
      document.querySelector("#pi-header-identity"),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Add header" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByLabelText("pi.form.compatibility"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByLabelText(/pi\.form\.modelAdditionalConfig/),
    ).not.toBeInTheDocument();
    expect(screen.getByText("provider.configJson")).toBeInTheDocument();
    expect(screen.getByLabelText("provider.configJson")).toHaveAttribute(
      "readonly",
    );

    await user.click(screen.getByRole("button", { name: "Add header" }));
    expect(screen.getByText("Headers")).toBeInTheDocument();
    expect(screen.getByLabelText("Header")).toBeInTheDocument();
    expect(screen.getByLabelText("Value")).toBeInTheDocument();
  });

  it("keeps the expanded config JSON synchronized with structured fields", async () => {
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
      models: [{ id: "preview-model" }],
    });
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
    await user.click(screen.getByLabelText("pi.form.reasoning"));
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
      models: [{ id: "model-a" }],
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
    ]);
    expect(
      config.models.map((model: { id: string; name?: string }) => ({
        id: model.id,
        name: model.name,
      })),
    ).toEqual([
      { id: "kimi-k2.7-code", name: "Kimi K2.7 Code" },
      { id: "kimi-k3", name: "Kimi K3" },
    ]);
    expect(submitted).not.toHaveProperty("piActivateModelId");
  });

  it("submits only explicit model fields and leaves pinned defaults to Pi", async () => {
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

    fireEvent.click(screen.getByRole("button", { name: "Save Pi provider" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const submitted = onSubmit.mock.calls[0][0];
    expect(submitted.providerKey).toBe("verified-provider");
    expect(JSON.parse(submitted.settingsConfig)).toEqual({
      name: "Verified provider",
      api: "openai-completions",
      baseUrl: "https://api.example.com/v1",
      models: [{ id: "opaque-model" }],
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
    await user.click(screen.getByRole("button", { name: "Add header" }));
    const headerName = screen.getByLabelText("Header");
    fireEvent.change(headerName, { target: { value: "user-agent" } });
    fireEvent.blur(headerName);
    fireEvent.change(screen.getByLabelText("Value"), {
      target: { value: "pi-test-agent/1.0" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );

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
      await screen.findByRole("menuitem", { name: "remote-model-b" }),
    );
    expect(modelIdInput).toHaveValue("remote-model-b");
    expect(screen.getByLabelText("pi.form.modelName")).toHaveValue(
      "remote-model-b",
    );
  });

  it("prefills selected model metadata and keeps every field overridable", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    server.use(
      http.get(MODELS_DEV_API_URL, () =>
        HttpResponse.json({
          openai: {
            id: "openai",
            name: "OpenAI",
            models: {
              "gpt-5.6-luna": {
                id: "gpt-5.6-luna",
                name: "GPT-5.6 Luna",
                reasoning: true,
                modalities: { input: ["text", "image", "pdf"] },
                limit: { context: 1_050_000, output: 128_000 },
              },
            },
          },
        }),
      ),
      http.post(`${TAURI_ENDPOINT}/fetch_models_for_config`, () =>
        HttpResponse.json([{ id: "gpt-5.6-luna", ownedBy: "openai" }]),
      ),
    );

    render(
      <PiProviderForm
        appId="pi"
        submitLabel="Save autofilled provider"
        onSubmit={onSubmit}
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
    await user.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );

    const modelIdInput = screen.getByLabelText("pi.form.modelId");
    await user.click(
      within(modelIdInput.parentElement as HTMLElement).getByRole("button"),
    );
    await user.click(
      await screen.findByRole("menuitem", { name: "gpt-5.6-luna" }),
    );

    const modelNameInput = screen.getByLabelText("pi.form.modelName");
    await waitFor(() => expect(modelNameInput).toHaveValue("GPT-5.6 Luna"));
    await user.click(
      screen.getByRole("button", { name: "展开或收起模型详情" }),
    );
    expect(screen.getByLabelText("pi.form.reasoning")).toBeChecked();
    expect(screen.getByLabelText("pi.form.imageInput")).toBeChecked();
    expect(screen.getByLabelText("pi.form.contextWindow")).toHaveValue(
      1_050_000,
    );
    expect(screen.getByLabelText("pi.form.maxTokens")).toHaveValue(128_000);
    fireEvent.change(modelNameInput, {
      target: { value: "My Luna" },
    });
    await user.click(screen.getByLabelText("pi.form.reasoning"));
    expect(
      screen.getByText("pi.form.modelMetadataOverridden"),
    ).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "pi.form.restoreModelAutofill" }),
    );
    expect(modelNameInput).toHaveValue("GPT-5.6 Luna");
    expect(screen.getByLabelText("pi.form.reasoning")).toBeChecked();

    fireEvent.change(modelNameInput, {
      target: { value: "My Luna" },
    });
    await user.click(screen.getByLabelText("pi.form.reasoning"));
    await user.click(screen.getByLabelText("pi.form.imageInput"));
    await user.click(
      screen.getByRole("button", { name: "Save autofilled provider" }),
    );

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig).models).toEqual(
      [
        {
          id: "gpt-5.6-luna",
          name: "My Luna",
          reasoning: false,
          input: ["text"],
          contextWindow: 1_050_000,
          maxTokens: 128_000,
        },
      ],
    );
  });

  it("does not persist discovered metadata during an otherwise no-op edit", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    server.use(
      http.get(MODELS_DEV_API_URL, () =>
        HttpResponse.json({
          openai: {
            id: "openai",
            name: "OpenAI",
            models: {
              "known-model": {
                id: "known-model",
                name: "Known Model",
                reasoning: true,
                modalities: { input: ["text", "image"] },
                limit: { context: 256_000, output: 32_000 },
              },
            },
          },
        }),
      ),
    );
    const input = {
      name: "Existing provider",
      baseUrl: "https://api.example.com/v1",
      api: "openai-responses",
      models: [{ id: "known-model" }],
    };

    render(
      <PiProviderForm
        appId="pi"
        providerId="existing-provider"
        submitLabel="Save existing provider"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{ name: input.name, settingsConfig: input }}
      />,
    );

    await waitFor(() =>
      expect(screen.getByLabelText("pi.form.modelName")).toHaveValue(
        "Known Model",
      ),
    );
    expect(
      JSON.parse(
        (screen.getByLabelText("provider.configJson") as HTMLTextAreaElement)
          .value,
      ).models,
    ).toEqual([{ id: "known-model" }]);

    fireEvent.click(
      screen.getByRole("button", { name: "Save existing provider" }),
    );
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toEqual(input);
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

  it("preserves pinned exact model IDs instead of trimming them", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const input = {
      name: "Exact IDs",
      api: "openai-responses",
      baseUrl: "https://api.example.com/v1",
      models: [{ id: " " }, { id: " model " }],
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
      models: [{ id: "model", reasoning: false, input: ["text"] }],
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
      models: [{ id: "model" }],
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
      models: [{ id: "model" }],
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
      models: [{ id: "model" }],
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

  it("submits the edit-open config as the optimistic concurrency baseline", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const initialConfig = {
      name: "Managed",
      api: "openai-completions",
      baseUrl: "https://api.example.com/v1",
      models: [{ id: "model-a" }, { id: "model-b" }],
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
    expect(onSubmit.mock.calls[0][0].expectedSettingsConfig).toEqual(
      initialConfig,
    );
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
            models: [{ id: "model" }],
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
