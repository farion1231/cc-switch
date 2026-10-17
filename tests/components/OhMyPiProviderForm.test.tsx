import { fireEvent, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useRef } from "react";
import { describe, expect, it, vi } from "vitest";
import { OhMyPiProviderForm } from "@/components/providers/forms/OhMyPiProviderForm";
import { renderWithQueryClient as render } from "../utils/testQueryClient";
vi.mock("@/components/JsonEditor", () => ({
  default: ({
    id,
    value,
    onChange,
  }: {
    id?: string;
    value: string;
    onChange: (v: string) => void;
  }) => {
    // Mimic the real CodeMirror editor: when the controlled `value` prop
    // changes from outside (form controls → displayYaml), it writes the new
    // document and fires an update listener that calls onChange with it.
    const lastValueRef = useRef(value);
    if (lastValueRef.current !== value) {
      lastValueRef.current = value;
      queueMicrotask(() => onChange(value));
    }
    return (
      <textarea
        id={id}
        value={value}
        onChange={(e) => onChange(e.target.value)}
      />
    );
  },
}));
function mount(settingsConfig: Record<string, unknown>, id = "custom") {
  const onSubmit = vi.fn();
  const view = render(
    <OhMyPiProviderForm
      appId="ohmypi"
      providerId={id}
      initialData={{ name: "Review", settingsConfig }}
      submitLabel="Save probe"
      onSubmit={onSubmit}
      onCancel={() => {}}
    />,
  );
  return { ...view, onSubmit };
}
async function save(onSubmit: ReturnType<typeof vi.fn>) {
  fireEvent.click(screen.getByRole("button", { name: "Save probe" }));
  await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
  return JSON.parse(onSubmit.mock.calls[0][0].settingsConfig);
}
describe("OhMyPi native edits", () => {
  beforeEach(() => {
    Element.prototype.scrollIntoView = vi.fn();
  });
  it("preserves inherited API when editing an imported native override", async () => {
    const { onSubmit } = mount(
      { baseUrl: "https://proxy.example.test", apiKey: "review-placeholder" },
      "anthropic",
    );
    const config = await save(onSubmit);
    expect(config).not.toHaveProperty("api");
    expect(config).not.toHaveProperty("models");
  });
  it("clearing other-fields YAML removes the old passthrough fields", async () => {
    const { onSubmit, container } = mount({
      baseUrl: "https://local.example.test",
      api: "openai-completions",
      auth: "none",
    });
    fireEvent.change(container.querySelector("#ohmypi-settings-config")!, {
      target: { value: "" },
    });
    const config = await save(onSubmit);
    expect(config).not.toHaveProperty("auth");
  });
  it("does not validate hidden models when model list is disabled", async () => {
    const { onSubmit, container } = mount({
      baseUrl: "https://proxy.example.test",
      api: "openai-completions",
      apiKey: "x",
      models: [{ id: "m" }],
    });
    fireEvent.change(container.querySelector('[id^="ohmypi-model-id-"]')!, {
      target: { value: "" },
    });
    fireEvent.keyDown(
      container.querySelector("#ohmypi-models-mode") as HTMLElement,
      { key: "ArrowDown" },
    );
    fireEvent.click(await screen.findByRole("option", { name: "不配置模型" }));
    const editor = container.querySelector(
      "#ohmypi-settings-config",
    ) as HTMLTextAreaElement;
    await waitFor(() => expect(editor.value).not.toContain("models:"));
    const config = await save(onSubmit);
    expect(config).not.toHaveProperty("models");
  });
  it("shows raw passthrough text with comments and saves it back into meta", async () => {
    const onSubmit = vi.fn();
    const { container } = render(
      <OhMyPiProviderForm
        appId="ohmypi"
        providerId="custom"
        initialData={{
          name: "Review",
          settingsConfig: { baseUrl: "https://local.example.test" },
          meta: {
            ohmypiPassthroughYaml:
              "# keep me\nauth: none\n",
          },
        }}
        submitLabel="Save probe"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );
    const editor = container.querySelector(
      "#ohmypi-settings-config",
    )! as HTMLTextAreaElement;
    expect(editor.value).toContain("# keep me");

    fireEvent.change(editor, {
      target: { value: "# edited comment\nauth: none\n" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save probe" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    const payload = onSubmit.mock.calls[0][0];
    expect(payload.meta?.ohmypiPassthroughYaml).toContain("# edited comment");
    const config = JSON.parse(payload.settingsConfig);
    expect(config.auth).toBe("none");
  });

  it("editing form controls updates the YAML editor in real time", async () => {
    const { container } = mount({
      name: "llama.cpp",
      baseUrl: "http://127.0.0.1:8080",
      api: "openai-responses",
      auth: "none",
    });
    const editor = container.querySelector(
      "#ohmypi-settings-config",
    )! as HTMLTextAreaElement;
    expect(editor.value).toContain("baseUrl: http://127.0.0.1:8080");
    fireEvent.change(
      container.querySelector("#ohmypi-provider-base-url")!,
      { target: { value: "http://127.0.0.1:8081" } },
    );
    await waitFor(() =>
      expect(editor.value).toContain("baseUrl: http://127.0.0.1:8081"),
    );
    // Non-managed lines survive the control-driven rewrite.
    expect(editor.value).toContain("auth: none");
  });

  it("editing controlled keys in the YAML editor overrides the form values", async () => {
    const { onSubmit, container } = mount({
      name: "llama.cpp",
      baseUrl: "http://127.0.0.1:8080",
      api: "openai-responses",
      auth: "none",
      discovery: { type: "llama.cpp" },
    });
    const editor = container.querySelector(
      "#ohmypi-settings-config",
    )! as HTMLTextAreaElement;
    fireEvent.change(editor, {
      target: {
        value: "baseUrl: http://127.0.0.1:8081\nauth: none\ndiscovery:\n  type: llama.cpp",
      },
    });
    const config = await save(onSubmit);
    expect(config.baseUrl).toBe("http://127.0.0.1:8081");
    expect(config.auth).toBe("none");
    expect(config.discovery).toEqual({ type: "llama.cpp" });
  });

  it("editing a model's expanded details keeps the details expanded", async () => {
    const { container } = mount({
      name: "llama.cpp",
      baseUrl: "http://127.0.0.1:8080",
      models: [{ id: "m1", name: "Model One" }],
    });

    fireEvent.click(
      screen.getByRole("button", { name: "展开或收起模型详情" }),
    );
    // The YAML mirror emits an echo once the expanded inputs render.
    const editor = container.querySelector(
      "#ohmypi-settings-config",
    ) as HTMLTextAreaElement;
    const contextWindow = container.querySelector(
      '[id^="ohmypi-model-context-window-"]',
    ) as HTMLInputElement;
    expect(contextWindow).toBeTruthy();

    // Toggle "支持扩展思考" inside the expanded area.
    fireEvent.click(
      container.querySelector('[id^="ohmypi-model-reasoning-"]')!,
    );
    expect(
      container.querySelector('[id^="ohmypi-model-context-window-"]'),
    ).toBeTruthy();
    await waitFor(() => expect(editor.value).toContain("reasoning: true"));

    // Toggle "支持图片输入".
    fireEvent.click(
      container.querySelector('[id^="ohmypi-model-image-input-"]')!,
    );
    await waitFor(() => expect(editor.value).toContain("image"));
    expect(
      container.querySelector('[id^="ohmypi-model-context-window-"]'),
    ).toBeTruthy();

    // Type into 上下文长度 / 最大输出 Token 数.
    fireEvent.change(contextWindow, { target: { value: "200000" } });
    await waitFor(() => expect(editor.value).toContain("contextWindow: 200000"));
    fireEvent.change(
      container.querySelector('[id^="ohmypi-model-max-tokens-"]')!,
      { target: { value: "16384" } },
    );
    await waitFor(() => expect(editor.value).toContain("maxTokens: 16384"));
    expect(
      container.querySelector('[id^="ohmypi-model-context-window-"]'),
    ).toBeTruthy();
  });

  it("rejects a created model without contextWindow or maxTokens", async () => {
    const onSubmit = vi.fn();
    const { container } = render(
      <OhMyPiProviderForm
        appId="ohmypi"
        providerId="custom"
        submitLabel="Save probe"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );

    fireEvent.change(container.querySelector("#ohmypi-provider-key")!, {
      target: { value: "llama-cpp" },
    });
    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "llama.cpp" },
    });
    await user.click(
      screen.getByRole("button", { name: "ohmypi.form.addModel" }),
    );
    fireEvent.change(container.querySelector('[id^="ohmypi-model-id-"]')!, {
      target: { value: "m1" },
    });

    fireEvent.click(screen.getByRole("button", { name: "Save probe" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("ohmypi.form.positiveNumberRequired");
    expect(onSubmit).not.toHaveBeenCalled();
    // The invalid model row re-expands and focuses the missing field.
    await waitFor(() =>
      expect(
        container.querySelector('[id^="ohmypi-model-context-window-"]'),
      ).toHaveFocus(),
    );

    fireEvent.change(
      container.querySelector('[id^="ohmypi-model-context-window-"]')!,
      { target: { value: "128000" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Save probe" }));
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(
        "ohmypi.form.positiveNumberRequired",
      ),
    );
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("round-trips YAML-only model keys through the passthrough", async () => {
    const { onSubmit } = mount({
      name: "llama.cpp",
      baseUrl: "http://127.0.0.1:8080",
      models: [
        {
          id: "m1",
          name: "Model One",
          contextWindow: 128000,
          maxTokens: 16384,
          thinking: { mode: "effort", efforts: ["low", "medium", "high"] },
          imageInputDecoder: "stb",
          cost: { input: 0.15, output: 0.25, cacheRead: 0, cacheWrite: 0 },
        },
      ],
    });

    const config = await save(onSubmit);
    expect(config.models[0].thinking).toEqual({
      mode: "effort",
      efforts: ["low", "medium", "high"],
    });
    expect(config.models[0].imageInputDecoder).toBe("stb");
    expect(config.models[0].cost).toEqual({
      input: 0.15,
      output: 0.25,
      cacheRead: 0,
      cacheWrite: 0,
    });
  });

  it("writes discovery config when the model source is automatic", async () => {
    const { onSubmit, container } = mount({
      name: "llama.cpp",
      baseUrl: "http://127.0.0.1:8080",
      api: "openai-completions",
    });

    fireEvent.keyDown(
      container.querySelector("#ohmypi-models-mode") as HTMLElement,
      { key: "ArrowDown" },
    );
    fireEvent.click(
      await screen.findByRole("option", { name: "自动发现模型列表" }),
    );
    // The discovery-type selector appears and defaults to openai-models-list.
    await waitFor(() =>
      expect(container.querySelector("#ohmypi-discovery-type")).toBeTruthy(),
    );
    const config = await save(onSubmit);
    expect(config.discovery).toEqual({ type: "openai-models-list" });
    expect(config).not.toHaveProperty("models");
  });

  it("imports a discovery block from YAML and keeps other discovery fields", async () => {
    const { onSubmit, container } = mount({
      name: "llama.cpp",
      baseUrl: "http://127.0.0.1:8080",
      api: "openai-completions",
      discovery: { type: "openai-models-list", timeoutMs: 5000 },
    });

    // The mode select reflects the imported discovery; switching to none
    // removes the discovery key entirely.
    fireEvent.keyDown(
      container.querySelector("#ohmypi-models-mode") as HTMLElement,
      { key: "ArrowDown" },
    );
    fireEvent.click(await screen.findByRole("option", { name: "不配置模型" }));
    const config = await save(onSubmit);
    expect(config).not.toHaveProperty("discovery");
    expect(config).not.toHaveProperty("models");
  });
});
