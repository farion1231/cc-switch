import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { http, HttpResponse } from "msw";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import { createTestQueryClient } from "../utils/testQueryClient";
import { server } from "../msw/server";
import { setLiveProviderIds, setSettings } from "../msw/state";

const toastError = vi.hoisted(() => vi.fn());
vi.mock("sonner", () => ({ toast: { error: toastError, success: vi.fn() } }));

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      aria-label="raw-config"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

function renderNativeForm(
  config: Record<string, unknown>,
  native = true,
  providerId = "anthropic",
  liveProviderIds = [providerId],
) {
  setLiveProviderIds("opencode", liveProviderIds);
  const client = createTestQueryClient();
  const onSubmit = vi.fn();
  const view = render(
    <QueryClientProvider client={client}>
      <ProviderForm
        appId="opencode"
        providerId={providerId}
        initialData={{
          name: "Anthropic override",
          settingsConfig: config,
          meta: native ? { opencodeConfigFormat: "v2" } : undefined,
        }}
        submitLabel="save-provider"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
      />
    </QueryClientProvider>,
  );
  return { ...view, client, onSubmit };
}

describe("native OpenCode provider form", () => {
  beforeEach(() => {
    toastError.mockReset();
    setSettings({ commonConfigConfirmed: true });
    server.use(
      http.post("http://tauri.local/auth_get_status", () =>
        HttpResponse.json({ authenticated: false, accounts: [] }),
      ),
      http.post("http://tauri.local/get_common_config_snippet", () =>
        HttpResponse.json(""),
      ),
    );
  });
  it.each([{}, { models: { alias: { modelID: "upstream" } } }])(
    "saves a package-less built-in override without injecting V1 fields: %j",
    async (config) => {
      const { container, client, onSubmit } = renderNativeForm(config);
      expect(container.querySelector("#opencode-npm")).toBeNull();
      expect(screen.getByText("opencode.nativeConfigHint")).toBeInTheDocument();
      await waitFor(() => expect(client.isFetching()).toBe(0));
      fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
      await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
      const submitted = onSubmit.mock.calls[0][0];
      expect(JSON.parse(submitted.settingsConfig)).toEqual(config);
      expect(submitted.meta.opencodeConfigFormat).toBe("v2");
    },
  );

  it("edits native JSON without applying the V1 structured editor", async () => {
    const config = {
      package: "aisdk:@ai-sdk/anthropic",
      settings: { baseURL: "https://native.example", apiKey: "test" },
      headers: { "X-Tenant": "example" },
      body: { metadata: { keep: true } },
      models: {
        model: { variants: [{ id: "low" }, { id: "high" }] },
      },
    };
    const { container, client, onSubmit } = renderNativeForm(config, false);
    expect(container.querySelector("#opencode-npm")).toBeNull();
    const edited = {
      ...config,
      settings: { ...config.settings, baseURL: "https://edited.example" },
    };
    fireEvent.change(screen.getByRole("textbox", { name: "raw-config" }), {
      target: { value: JSON.stringify(edited) },
    });
    await waitFor(() => expect(client.isFetching()).toBe(0));
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toEqual(
      edited,
    );
    expect(onSubmit.mock.calls[0][0].meta.opencodeConfigFormat).toBe("v2");
  });

  it("continues to expose the structured editor for a V1 provider", () => {
    const { container } = renderNativeForm(
      {
        npm: "@ai-sdk/anthropic",
        options: {},
        models: { model: { name: "Model" } },
      },
      false,
    );
    expect(container.querySelector("#opencode-npm")).toBeInTheDocument();
    expect(
      screen.queryByText("opencode.nativeConfigHint"),
    ).not.toBeInTheDocument();
  });

  it("keeps an existing native provider ID even when it contains underscores", async () => {
    const { client, onSubmit } = renderNativeForm({}, true, "custom_provider");
    expect(
      screen.queryByText("opencode.providerKeyInvalid"),
    ).not.toBeInTheDocument();
    await waitFor(() => expect(client.isFetching()).toBe(0));
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit.mock.calls[0][0].providerKey).toBe("custom_provider");
  });
  it("keeps a stored native override editable after removal from live", async () => {
    const config = { settings: { apiKey: "edited" } };
    const { client, onSubmit } = renderNativeForm(
      config,
      true,
      "anthropic",
      [],
    );
    await waitFor(() => expect(client.isFetching()).toBe(0));
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toEqual(
      config,
    );
    expect(toastError).not.toHaveBeenCalled();
  });

  it.each([{}, { package: "aisdk:@ai-sdk/anthropic", models: {} }])(
    "requires a package and models for a renamed native ID: %j",
    async (config) => {
      const { client, onSubmit } = renderNativeForm(
        config,
        true,
        "anthropic",
        [],
      );
      await waitFor(() => expect(client.isFetching()).toBe(0));
      fireEvent.change(document.getElementById("opencode-key")!, {
        target: { value: "anthropic-copy" },
      });
      fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
      await waitFor(() =>
        expect(toastError).toHaveBeenCalledWith(
          "opencode.nativeCustomProviderRequired",
        ),
      );
      expect(onSubmit).not.toHaveBeenCalled();
    },
  );

  it("does not exempt a row that only looks native", async () => {
    // Without stored V2 metadata this may be an old copy missing its definition.
    const { client, onSubmit } = renderNativeForm(
      { settings: { apiKey: "test" } },
      false,
      "opencode-go-copy",
      [],
    );
    await waitFor(() => expect(client.isFetching()).toBe(0));
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
    await waitFor(() =>
      expect(toastError).toHaveBeenCalledWith(
        "opencode.nativeCustomProviderRequired",
      ),
    );
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it.each([
    ["custom", true],
    ["other", false],
  ])(
    "reads the definition of a pasted full config under providers.%s",
    async (providerKey, accepted) => {
      setLiveProviderIds("opencode", []);
      const client = createTestQueryClient();
      const onSubmit = vi.fn();
      render(
        <QueryClientProvider client={client}>
          <ProviderForm
            appId="opencode"
            submitLabel="save-provider"
            onSubmit={onSubmit}
            onCancel={vi.fn()}
          />
        </QueryClientProvider>,
      );
      const full = {
        providers: {
          custom: {
            package: "aisdk:@ai-sdk/openai",
            models: { "gpt-5": {} },
          },
        },
      };
      fireEvent.change(screen.getByRole("textbox", { name: "raw-config" }), {
        target: { value: JSON.stringify(full) },
      });
      fireEvent.change(document.getElementById("opencode-key")!, {
        target: { value: providerKey },
      });
      fireEvent.change(document.querySelector('input[name="name"]')!, {
        target: { value: "Custom" },
      });
      await waitFor(() => expect(client.isFetching()).toBe(0));
      fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
      if (accepted) {
        await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
        expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toEqual(
          full,
        );
        expect(toastError).not.toHaveBeenCalled();
      } else {
        await waitFor(() =>
          expect(toastError).toHaveBeenCalledWith(
            "opencode.nativeCustomProviderRequired",
          ),
        );
        expect(onSubmit).not.toHaveBeenCalled();
      }
    },
  );
});
