import { fireEvent, screen, waitFor } from "@testing-library/react";
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
  }) => (
    <textarea
      id={id}
      value={value}
      onChange={(e) => onChange(e.target.value)}
    />
  ),
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
    fireEvent.click(container.querySelector("#ohmypi-models-toggle")!);
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
});
