import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PiProviderForm } from "@/components/providers/forms/PiProviderForm";
import { parse as parseYaml } from "yaml";
import { http, HttpResponse } from "msw";
import { server } from "../msw/server";
import { renderWithQueryClient as render } from "../utils/testQueryClient";
import { piProviderPresets } from "@/config/piProviderPresets";
import {
  completeModel,
  ompNode,
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

  it("offers the Mistral API format in Pi but not in omp", async () => {
    const user = userEvent.setup();
    for (const appId of ["pi", "omp"] as const) {
      const { unmount } = render(
        <PiProviderForm
          appId={appId}
          submitLabel="Save"
          onSubmit={() => {}}
          onCancel={() => {}}
          initialData={{
            name: "gateway",
            settingsConfig: { api: "openai-completions" },
          }}
        />,
      );
      await user.click(document.querySelector("#pi-provider-api-select")!);
      await screen.findByRole("option", { name: "Anthropic Messages" });
      const mistral = screen.queryByRole("option", { name: "Mistral" });
      if (appId === "pi") expect(mistral).toBeInTheDocument();
      else expect(mistral).not.toBeInTheDocument();
      await user.keyboard("{Escape}");
      unmount();
    }
  });

  it("keeps showing an existing omp API value the format list no longer offers", () => {
    render(
      <PiProviderForm
        appId="omp"
        submitLabel="Save"
        onSubmit={() => {}}
        onCancel={() => {}}
        initialData={{
          name: "legacy",
          settingsConfig: { api: "mistral-conversations" },
        }}
      />,
    );
    expect(document.querySelector("#pi-provider-api-select")).toHaveTextContent(
      "mistral-conversations",
    );
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
      "providerForm.nativeConfig.title",
    ) as HTMLTextAreaElement;
    expect(parseYaml(configEditor.value)).toHaveProperty(
      "providers.custom.discovery.injectV1",
      false,
    );

    // omp 只允许 openai-models-list 带 injectV1，其余子键照旧保留
    openAdvancedOptions();
    await user.click(document.querySelector("#omp-discovery-type")!);
    await user.click(
      await screen.findByRole("option", {
        name: "omp.form.discoveryTypes.ollama",
      }),
    );
    expect(ompNode(configEditor).discovery).toEqual({
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
      "providerForm.nativeConfig.title",
    ) as HTMLTextAreaElement;
    const preset = ompNode(configEditor);
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
    expect(ompNode(configEditor).discovery).toEqual({
      type: "openai-models-list",
    });

    await user.click(document.querySelector("#omp-discovery-type")!);
    await user.click(
      await screen.findByRole("option", { name: "omp.form.discoveryOff" }),
    );
    expect(ompNode(configEditor)).not.toHaveProperty("discovery");

    fireEvent.change(screen.getByLabelText("pi.form.credential"), {
      target: { value: "literal-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save omp preset" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(
      JSON.parse(onSubmit.mock.calls[0][0].settingsConfig),
    ).not.toHaveProperty("name");
  });

  it("edits omp through its providers node in models.yml", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(
      <PiProviderForm
        appId="omp"
        providerId="gateway"
        submitLabel="Save omp provider"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{
          name: "gateway",
          settingsConfig: {
            baseUrl: "https://gateway.example.com/v1",
            api: "openai-completions",
            apiKey: "literal-key",
            models: [{ id: "model-x" }],
          },
        }}
      />,
    );
    const editor = screen.getByLabelText(
      "providerForm.nativeConfig.title",
    ) as HTMLTextAreaElement;
    expect(editor.value.startsWith("providers:\n  gateway:\n")).toBe(true);

    // 另一家的节点或全局设置不归这张卡：拦住保存，结构化字段也先不让动
    fireEvent.change(editor, {
      target: { value: "providers:\n  other:\n    baseUrl: x\n" },
    });
    expect(screen.getByText("providerForm.nativeConfig.scope")).toBeVisible();
    expect(
      screen.getByPlaceholderText("https://api.example.com/v1"),
    ).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Save omp provider" }));
    expect(onSubmit).not.toHaveBeenCalled();

    fireEvent.change(editor, {
      target: {
        value: [
          "providers:",
          "  gateway:",
          "    baseUrl: https://edited.example.com/v1",
          "    api: openai-completions",
          "    apiKey: literal-key",
          "    models:",
          "      - id: model-x",
          "",
        ].join("\n"),
      },
    });
    expect(
      screen.queryByText("providerForm.nativeConfig.scope"),
    ).not.toBeInTheDocument();
    expect(
      screen.getByPlaceholderText("https://api.example.com/v1"),
    ).toHaveValue("https://edited.example.com/v1");
    fireEvent.click(screen.getByRole("button", { name: "Save omp provider" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toMatchObject({
      baseUrl: "https://edited.example.com/v1",
    });
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
      "kimi-k3.1-pro",
      "kimi-k3.1-pro-highspeed",
      "kimi-k3",
      "kimi-k2.7-code",
      "kimi-k2.7-code-highspeed",
      "kimi-k2.6",
    ]);
    expect(
      config.models.map((model: { id: string; name?: string }) => ({
        id: model.id,
        name: model.name,
      })),
    ).toEqual([
      { id: "kimi-k3.1-pro", name: "Kimi K3.1 Pro" },
      { id: "kimi-k3.1-pro-highspeed", name: "Kimi K3.1 Pro HighSpeed" },
      { id: "kimi-k3", name: "Kimi K3" },
      { id: "kimi-k2.7-code", name: "Kimi K2.7 Code" },
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
});
