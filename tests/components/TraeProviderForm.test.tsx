import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { TraeProviderForm } from "@/components/providers/forms/TraeProviderForm";
import { createTestQueryClient } from "../utils/testQueryClient";

beforeAll(() => {
  Element.prototype.scrollIntoView ??= vi.fn();
});

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    id,
    value,
    onChange,
  }: {
    id: string;
    value: string;
    onChange: (text: string) => void;
  }) => (
    <textarea
      id={id}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

const custom = {
  provider: {
    name: "Example",
    base_url: "https://example.com/v1",
    wire_api: "responses",
    experimental_bearer_token: "sk-test",
    http_headers: { "X-Custom": "keep" },
  },
  model: "gpt-5",
};

function setup(
  settingsConfig: Record<string, unknown> = custom,
  providerId = "example",
  onSubmit = vi.fn(),
) {
  const view = render(
    <QueryClientProvider client={createTestQueryClient()}>
      <TraeProviderForm
        appId="trae"
        providerId={providerId}
        initialData={{ name: "Example", settingsConfig }}
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />
    </QueryClientProvider>,
  );
  return { ...view, onSubmit };
}

describe("TraeProviderForm", () => {
  it("edits the model and keeps native-only provider fields", async () => {
    const { container, onSubmit } = setup();
    const key = container.querySelector<HTMLInputElement>("#trae-provider-key");
    expect(key?.value).toBe("example");
    expect(key?.disabled).toBe(true);
    const model = container.querySelector<HTMLInputElement>("#trae-model")!;
    fireEvent.change(model, { target: { value: " gpt-5.1 " } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalled());
    const submitted = onSubmit.mock.calls[0][0];
    expect(submitted.providerKey).toBe("example");
    expect(JSON.parse(submitted.settingsConfig)).toEqual({
      provider: { ...custom.provider, name: "Example" },
      model: "gpt-5.1",
    });
  });

  it("accepts an env_key instead of a stored API key", async () => {
    const { container, onSubmit } = setup();
    fireEvent.change(container.querySelector("#trae-api-key")!, {
      target: { value: "" },
    });
    const save = screen.getByRole("button", { name: "Save" });
    await waitFor(() => expect(save).toBeDisabled());
    fireEvent.change(container.querySelector("#trae-env-key")!, {
      target: { value: "MY_KEY" },
    });
    await waitFor(() => expect(save).toBeEnabled());
    fireEvent.click(save);
    await waitFor(() => expect(onSubmit).toHaveBeenCalled());
    const provider = JSON.parse(
      onSubmit.mock.calls[0][0].settingsConfig,
    ).provider;
    expect(provider.env_key).toBe("MY_KEY");
    expect(provider).not.toHaveProperty("experimental_bearer_token");
  });

  it("shows only the login hint for the TRAE account provider", async () => {
    const { container, onSubmit } = setup({ official: true }, "trae-official");
    expect(container.querySelector("#trae-base-url")).toBeNull();
    expect(container.querySelector("#trae-settings-config")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalled());
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toEqual({
      official: true,
    });
  });
});
