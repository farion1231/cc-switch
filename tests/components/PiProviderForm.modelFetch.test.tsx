import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PiProviderForm } from "@/components/providers/forms/PiProviderForm";
import { http, HttpResponse } from "msw";
import { server } from "../msw/server";
import { renderWithQueryClient as render } from "../utils/testQueryClient";
import { piThinkingProfiles } from "@/config/piThinkingProfiles";
import { waitForOverlayTransition } from "../utils/overlayTransition";
import { ompNode, openAdvancedOptions } from "./piProviderFormTestHelpers";

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

describe("PiProviderForm model fetching", () => {
  beforeEach(() => {
    Element.prototype.scrollIntoView = vi.fn();
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
    await waitForOverlayTransition();

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
      http.post(`${TAURI_ENDPOINT}/fetch_models_dev`, () =>
        HttpResponse.json(
          JSON.stringify({
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
      http.post(`${TAURI_ENDPOINT}/fetch_models_dev`, () =>
        HttpResponse.error(),
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

  describe("fetched model picker", () => {
    function serveModels(ids: string[]) {
      server.use(
        http.post(`${TAURI_ENDPOINT}/fetch_models_for_config`, () =>
          HttpResponse.json(ids.map((id) => ({ id, ownedBy: "example" }))),
        ),
        http.post(`${TAURI_ENDPOINT}/fetch_models_dev`, () =>
          HttpResponse.json(
            JSON.stringify({
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
        appId === "omp"
          ? "providerForm.nativeConfig.title"
          : "provider.configJson",
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
        expect(ompNode(configEditor).models[0]).toMatchObject({
          id: "model-x",
          reasoning: true,
          contextWindow: 123456,
        }),
      );
      expect(ompNode(configEditor).models[0]).not.toHaveProperty(
        "thinkingLevelMap",
      );
    });
  });
});
