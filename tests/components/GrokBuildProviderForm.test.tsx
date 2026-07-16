import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { parse as parseToml } from "smol-toml";
import { describe, expect, it, vi } from "vitest";
import { GrokBuildProviderForm } from "@/components/providers/forms/GrokBuildProviderForm";

describe("GrokBuildProviderForm", () => {
  it("offers Codex-compatible provider presets and applies one", async () => {
    const user = userEvent.setup();
    const { container } = render(
      <GrokBuildProviderForm
        submitLabel="Save"
        onSubmit={() => {}}
        onCancel={() => {}}
      />,
    );

    await user.click(screen.getByRole("button", { name: /PatewayAI/ }));

    const baseUrlInput =
      container.querySelector<HTMLInputElement>("#codexBaseUrl");
    const nameInput =
      container.querySelector<HTMLInputElement>('input[name="name"]');
    expect(baseUrlInput?.value).toBe("https://api.pateway.ai/v1");
    expect(nameInput?.value).toBe("PatewayAI");
  });

  it("submits a complete config.toml payload with Grok defaults", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    const { container } = render(
      <GrokBuildProviderForm
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );

    const nameInput =
      container.querySelector<HTMLInputElement>('input[name="name"]');
    const baseUrlInput =
      container.querySelector<HTMLInputElement>("#codexBaseUrl");
    expect(nameInput).not.toBeNull();
    expect(baseUrlInput).not.toBeNull();

    fireEvent.change(nameInput!, { target: { value: "Example Relay" } });
    fireEvent.change(baseUrlInput!, {
      target: { value: "https://relay.example.com/v1" },
    });
    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "secret-key" },
    });
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(onSubmit).toHaveBeenCalledTimes(1);
    const submitted = onSubmit.mock.calls[0][0];
    const settings = JSON.parse(submitted.settingsConfig);
    const config = parseToml(settings.config) as any;

    expect(config.models.default).toBe("grok-4.5");
    expect(config.model["grok-4.5"]).toEqual({
      model: "grok-4.5",
      base_url: "https://relay.example.com/v1",
      name: "Example Relay",
      api_key: "secret-key",
      api_backend: "responses",
      context_window: 500000,
    });
  });
});
