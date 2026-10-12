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
});
