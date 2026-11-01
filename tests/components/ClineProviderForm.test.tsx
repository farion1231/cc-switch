import { QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { ClineProviderForm } from "@/components/providers/forms/ClineProviderForm";
import type { ProviderFormProps } from "@/components/providers/forms/ProviderForm";
import { fetchModelsForConfig } from "@/lib/api/model-fetch";
import type { Provider } from "@/types";
import { setProviders } from "../msw/state";
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
      aria-label="provider.configJson"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

vi.mock("@/lib/api/model-fetch", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api/model-fetch")>()),
  fetchModelsForConfig: vi.fn(),
}));

const toastInfo = vi.hoisted(() => vi.fn());
vi.mock("@/lib/toast", () => ({
  toast: { info: toastInfo, success: vi.fn(), error: vi.fn() },
}));

function setup(props: Partial<ProviderFormProps> = {}) {
  const onSubmit = vi.fn();
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ClineProviderForm
        appId="cline"
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={() => {}}
        {...props}
      />
    </QueryClientProvider>,
  );
  return onSubmit;
}

const existing = (id: string, settingsConfig: Record<string, unknown>) =>
  ({ id, name: id, settingsConfig }) as Provider;

async function pickPreset(group: string, name: string) {
  const user = userEvent.setup();
  await user.click(
    screen.getByRole("button", {
      name: new RegExp(`^providerPreset\\.group\\.${group} `),
    }),
  );
  await user.click(screen.getByRole("button", { name }));
  return user;
}

const saved = (submit: ReturnType<typeof vi.fn>) =>
  JSON.parse(submit.mock.calls[0][0].settingsConfig);

describe("ClineProviderForm", () => {
  it("adds a custom Anthropic provider with a generated id and client", async () => {
    setProviders("cline", {
      "ccs-my-relay": existing("ccs-my-relay", {
        settings: {},
        provider: { name: "My Relay", baseUrl: "https://a.example/v1" },
        models: { a: { id: "a" } },
      }),
    });
    const submit = setup();
    const user = await pickPreset("thirdparty", "cline.preset.customAnthropic");
    expect(screen.getByText("cline.restartHint")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "My Relay" },
    });
    await waitFor(() =>
      expect(screen.getByLabelText("cline.providerId")).toHaveValue(
        "ccs-my-relay-2",
      ),
    );
    fireEvent.change(screen.getByLabelText("cline.baseUrl *"), {
      target: { value: "https://relay.example.com/v1 " },
    });
    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "sk-test" },
    });
    await user.click(screen.getByRole("button", { name: "cline.addModel" }));
    fireEvent.change(screen.getByLabelText("cline.modelId *"), {
      target: { value: "claude-x" },
    });
    fireEvent.change(screen.getByLabelText("cline.contextWindow"), {
      target: { value: "200000" },
    });
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    expect(submit.mock.calls[0][0]).toMatchObject({
      name: "My Relay",
      providerKey: "ccs-my-relay-2",
    });
    expect(saved(submit)).toEqual({
      settings: { apiKey: "sk-test", model: "claude-x" },
      provider: {
        name: "My Relay",
        baseUrl: "https://relay.example.com/v1",
        protocol: "anthropic",
        client: "anthropic",
        defaultModelId: "claude-x",
      },
      models: {
        "claude-x": { id: "claude-x", contextWindow: 200000 },
      },
    });
    expect(toastInfo).toHaveBeenCalledWith("cline.restartHint");
  }, 20_000);

  it("drops the anthropic client when the protocol changes", async () => {
    const submit = setup({
      providerId: "ccs-relay",
      initialData: {
        name: "Relay",
        settingsConfig: {
          settings: { apiKey: "k", model: "m" },
          provider: {
            name: "Relay",
            baseUrl: "https://relay.example/v1",
            protocol: "anthropic",
            client: "anthropic",
          },
          models: { m: { id: "m" } },
        },
      },
    });
    fireEvent.change(screen.getByLabelText("provider.configJson"), {
      target: {
        value: JSON.stringify({
          settings: { apiKey: "k", model: "m" },
          provider: {
            name: "Relay",
            baseUrl: "https://relay.example/v1",
            protocol: "openai-chat",
            client: "anthropic",
          },
          models: { m: { id: "m" } },
        }),
      },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    expect(submit.mock.calls[0][0].providerKey).toBe("ccs-relay");
    expect(saved(submit).provider).toEqual({
      name: "Relay",
      baseUrl: "https://relay.example/v1",
      protocol: "openai-chat",
      defaultModelId: "m",
    });
  });

  it("reports validation errors instead of submitting", async () => {
    const submit = setup({
      providerId: "ccs-relay",
      initialData: {
        name: "Relay",
        settingsConfig: {
          settings: { apiKey: "k", model: "a" },
          provider: { name: "Relay", baseUrl: "https://relay.example/v1" },
          models: { a: { id: "a" }, b: { id: "b" } },
        },
      },
    });
    fireEvent.change(screen.getByLabelText("cline.baseUrl *"), {
      target: { value: "ftp://relay.example" },
    });
    fireEvent.change(screen.getAllByLabelText("cline.modelId *")[1], {
      target: { value: "a" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(
      await screen.findByText("cline.validation.baseUrl"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("cline.validation.duplicateModel"),
    ).toBeInTheDocument();
    expect(submit).not.toHaveBeenCalled();

    // Removing every model disables saving outright.
    while (
      screen.queryAllByRole("button", { name: "cline.removeModel" }).length
    ) {
      fireEvent.click(
        screen.getAllByRole("button", { name: "cline.removeModel" })[0],
      );
    }
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("adds a built-in provider under its Cline id without models", async () => {
    const submit = setup();
    const user = await pickPreset("vendor", "DeepSeek");
    expect(screen.getByLabelText("cline.providerId")).toHaveValue("deepseek");
    expect(screen.queryByText("cline.restartHint")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("cline.baseUrl *")).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "sk-ds" },
    });
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    expect(submit.mock.calls[0][0].providerKey).toBe("deepseek");
    const config = saved(submit);
    expect(config.settings).toMatchObject({ apiKey: "sk-ds" });
    expect(config.settings.model).toBeTruthy();
    expect(config).not.toHaveProperty("provider");
    expect(config).not.toHaveProperty("models");
    expect(toastInfo).not.toHaveBeenCalled();
  });

  it("blocks adding a built-in provider that Cline already has", async () => {
    setProviders("cline", {
      deepseek: existing("deepseek", { settings: { apiKey: "old" } }),
    });
    setup();
    await pickPreset("vendor", "DeepSeek");
    expect(await screen.findByText("cline.builtinExists")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "sk-ds" },
    });
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("keeps OAuth credentials read-only and untouched", async () => {
    const native = {
      settings: {
        model: "anthropic/claude-sonnet-5",
        auth: { accessToken: "token", refreshToken: "refresh" },
      },
      tokenSource: "oauth",
    };
    const submit = setup({
      providerId: "cline",
      initialData: { name: "cline", settingsConfig: native },
    });
    expect(screen.getByText("cline.oauthReadOnly")).toBeInTheDocument();
    expect(screen.queryByLabelText("API Key")).not.toBeInTheDocument();
    expect(
      screen.queryByLabelText("provider.configJson"),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    expect(submit.mock.calls[0][0].providerKey).toBe("cline");
    expect(saved(submit)).toEqual(native);
  });

  it("adds fetched models to a custom provider and selects one", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValue([
      { id: "gpt-x", ownedBy: null },
      { id: "m", ownedBy: null },
    ]);
    const user = userEvent.setup();
    const submit = setup({
      providerId: "ccs-relay",
      initialData: {
        name: "Relay",
        settingsConfig: {
          settings: { apiKey: "sk", model: "m" },
          provider: {
            name: "Relay",
            baseUrl: "http://localhost:9999/v1",
            protocol: "openai-chat",
          },
          models: { m: { id: "m" } },
        },
      },
    });
    await user.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    expect(fetchModelsForConfig).toHaveBeenCalledWith(
      "http://localhost:9999/v1",
      "sk",
      undefined,
      undefined,
      undefined,
      { apiFormat: undefined, requestHeaders: undefined },
    );
    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("checkbox", { name: "gpt-x" }));
    await user.click(
      within(dialog).getByRole("button", {
        name: "providerForm.modelPicker.apply",
      }),
    );
    await waitFor(() =>
      expect(screen.getAllByLabelText("cline.modelId *")).toHaveLength(2),
    );
    await user.click(screen.getAllByRole("radio")[1]);
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const config = saved(submit);
    expect(config.models).toEqual({
      m: { id: "m" },
      "gpt-x": { id: "gpt-x", name: "gpt-x" },
    });
    expect(config.settings.model).toBe("gpt-x");
    expect(config.provider.defaultModelId).toBe("gpt-x");
  });

  it("fetches models for a local built-in provider from Cline's default endpoint", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValue([
      { id: "qwen3", ownedBy: null },
    ]);
    setup();
    const user = await pickPreset("local", "Ollama");
    await user.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    expect(fetchModelsForConfig).toHaveBeenCalledWith(
      "http://localhost:11434/v1",
      "",
      undefined,
      undefined,
      undefined,
      { apiFormat: undefined, requestHeaders: undefined },
    );
  });
});
