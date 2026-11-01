import type { ReactElement } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render as rtlRender,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { WorkBuddyProviderForm } from "@/components/providers/forms/WorkBuddyProviderForm";
import { setProviders } from "../msw/state";
import { createTestQueryClient } from "../utils/testQueryClient";

const modelFetchApiMock = vi.hoisted(() => ({
  fetchModelsForConfig: vi.fn(),
  showFetchModelsError: vi.fn(),
}));

vi.mock("@/lib/api/model-fetch", () => ({
  fetchModelsForConfig: modelFetchApiMock.fetchModelsForConfig,
  showFetchModelsError: modelFetchApiMock.showFetchModelsError,
}));

const authMocks = vi.hoisted(() => ({
  getStatus: vi.fn(),
  liveState: vi.fn(),
  prepareNewLogin: vi.fn(),
  openApp: vi.fn(),
}));

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    authApi: {
      ...actual.authApi,
      authGetStatus: authMocks.getStatus,
      workbuddyAccountLiveState: authMocks.liveState,
      workbuddyAccountPrepareNewLogin: authMocks.prepareNewLogin,
      workbuddyAccountOpenApp: authMocks.openApp,
    },
  };
});

const wbAccount = (id: string, login: string, plan?: string) => ({
  id,
  provider: "workbuddy_account",
  login,
  avatar_url: null,
  authenticated_at: 0,
  is_default: id === "u-1",
  github_domain: "workbuddy",
  requires_reauth: false,
  ...(plan ? { account_kind: plan } : {}),
});

const officialCard = (accountId: string) => ({
  name: "Work account",
  settingsConfig: {},
  category: "official" as const,
  meta: {
    authBinding: {
      source: "managed_account" as const,
      authProvider: "workbuddy_account",
      accountId,
    },
  },
});

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    id,
    value,
    onChange,
  }: {
    id: string;
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      id={id}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

const render = (ui: ReactElement) =>
  rtlRender(
    <QueryClientProvider client={createTestQueryClient()}>
      {ui}
    </QueryClientProvider>,
  );

const existing = {
  vendor: "DeepSeek",
  url: "https://api.deepseek.com/v1/chat/completions",
  apiKey: "local-test-key",
  models: [
    { id: "deepseek-chat", name: "DeepSeek Chat", supportsToolCall: true },
  ],
  futureSetting: { keep: true },
};

describe("WorkBuddyProviderForm", () => {
  beforeEach(() => {
    for (const mock of Object.values(authMocks)) mock.mockReset();
    authMocks.getStatus.mockResolvedValue({
      provider: "workbuddy_account",
      authenticated: true,
      default_account_id: "u-1",
      accounts: [wbAccount("u-1", "Alice", "pro"), wbAccount("u-2", "Bob")],
    });
    authMocks.liveState.mockResolvedValue({
      running: false,
      signedIn: true,
      recognized: true,
      imported: true,
    });
    authMocks.openApp.mockResolvedValue(undefined);
  });

  it("creates an official sign-in card bound to the current account", async () => {
    setProviders("workbuddy", {});
    const submit = vi.fn();
    render(
      <WorkBuddyProviderForm
        appId="workbuddy"
        initialData={undefined}
        submitLabel="Add"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(await screen.findByText("workbuddy.official.presetName"));

    // 账号区代替接口地址、Key、模型和 JSON
    expect(
      await screen.findByLabelText("workbuddy.official.account"),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText(/workbuddy.form.url/)).toBeNull();
    expect(screen.queryByLabelText(/opencode.providerKey/)).toBeNull();
    expect(screen.queryByLabelText("provider.configJson")).toBeNull();

    // 默认绑定 WorkBuddy 现在登着的号，名称用它的昵称
    await waitFor(() =>
      expect(screen.getByLabelText("provider.name")).toHaveValue("Alice"),
    );
    const save = screen.getByRole("button", { name: "Add" });
    await waitFor(() => expect(save).toBeEnabled());
    fireEvent.click(save);
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const call = submit.mock.calls[0][0];
    expect(call).toMatchObject({
      name: "Alice",
      presetCategory: "official",
      providerKey: "workbuddy-official",
      meta: {
        authBinding: {
          source: "managed_account",
          authProvider: "workbuddy_account",
          accountId: "u-1",
        },
      },
    });
    expect(JSON.parse(call.settingsConfig)).toEqual({});
  });

  it("keeps the binding when editing an official card", async () => {
    const submit = vi.fn();
    render(
      <WorkBuddyProviderForm
        appId="workbuddy"
        providerId="workbuddy-official"
        initialData={officialCard("u-2")}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    const save = screen.getByRole("button", { name: "Save" });
    await waitFor(() => expect(save).toBeEnabled());
    // 用户起的名称不被昵称覆盖
    expect(screen.getByLabelText("provider.name")).toHaveValue("Work account");
    fireEvent.click(save);
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    expect(submit.mock.calls[0][0]).toMatchObject({
      providerKey: "workbuddy-official",
      presetCategory: "official",
      meta: { authBinding: { accountId: "u-2" } },
    });
  });

  it("asks to choose again when the bound account was deleted", async () => {
    render(
      <WorkBuddyProviderForm
        appId="workbuddy"
        providerId="workbuddy-official"
        initialData={officialCard("u-9")}
        submitLabel="Save"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    expect(
      (await screen.findAllByText("workbuddy.official.accountDeleted")).length,
    ).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("prepares a new sign-in and opens WorkBuddy", async () => {
    authMocks.prepareNewLogin.mockResolvedValue(null);
    render(
      <WorkBuddyProviderForm
        appId="workbuddy"
        providerId="workbuddy-official"
        initialData={officialCard("u-1")}
        submitLabel="Save"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: "authCenter.workbuddy.prepareNewLogin",
      }),
    );
    await waitFor(() => expect(authMocks.prepareNewLogin).toHaveBeenCalled());
    await waitFor(() => expect(authMocks.openApp).toHaveBeenCalled());
  });

  it("edits fields while keeping the key and unknown settings", async () => {
    const submit = vi.fn();
    render(
      <WorkBuddyProviderForm
        appId="workbuddy"
        providerId="deepseek"
        initialData={{ name: "DeepSeek", settingsConfig: existing }}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    expect(screen.getByLabelText(/opencode.providerKey/)).toBeDisabled();
    fireEvent.change(screen.getByLabelText("opencode.modelName"), {
      target: { value: "Chat" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const call = submit.mock.calls[0][0];
    expect(call.providerKey).toBe("deepseek");
    expect(JSON.parse(call.settingsConfig)).toEqual({
      ...existing,
      models: [{ ...existing.models[0], name: "Chat" }],
    });
  });

  it("blocks saving until a model and endpoint are filled in", async () => {
    const submit = vi.fn();
    render(
      <WorkBuddyProviderForm
        appId="workbuddy"
        initialData={undefined}
        submitLabel="Add"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    const save = screen.getByRole("button", { name: "Add" });
    expect(save).toBeDisabled();

    fireEvent.change(screen.getByLabelText(/opencode.providerKey/), {
      target: { value: "My Key" },
    });
    expect(screen.getByLabelText(/opencode.providerKey/)).toHaveValue("mykey");
    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "Mine" },
    });
    fireEvent.change(screen.getByLabelText(/workbuddy.form.url/), {
      target: { value: "https://api.example.com/v1/chat/completions" },
    });
    expect(save).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: /opencode.addModel/ }));
    fireEvent.change(screen.getByLabelText("opencode.modelId"), {
      target: { value: " my-model " },
    });
    await waitFor(() => expect(save).toBeEnabled());
    fireEvent.click(save);
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const call = submit.mock.calls[0][0];
    expect(call.providerKey).toBe("mykey");
    expect(JSON.parse(call.settingsConfig)).toMatchObject({
      url: "https://api.example.com/v1/chat/completions",
      models: [{ id: "my-model", supportsToolCall: true }],
    });
  });

  it("uses the shared API key and Chat Completions endpoint fields", () => {
    render(
      <WorkBuddyProviderForm
        appId="workbuddy"
        providerId="deepseek"
        initialData={{ name: "DeepSeek", settingsConfig: existing }}
        submitLabel="Save"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    expect(document.querySelector("#workbuddy-api-format")).toBeNull();
    expect(screen.getByLabelText("API Key")).toHaveValue("local-test-key");
    expect(screen.getByText("workbuddy.form.apiKeyHint")).toBeInTheDocument();
    expect(screen.getByLabelText("workbuddy.form.url")).toHaveAttribute(
      "aria-required",
      "true",
    );
    expect(screen.getByLabelText("workbuddy.form.url")).toHaveAttribute(
      "placeholder",
      "https://api.example.com/v1/chat/completions",
    );
    expect(screen.getByText("workbuddy.form.urlHint")).toBeInTheDocument();
  });

  it("keeps an existing api field untouched when saving", async () => {
    const submit = vi.fn().mockResolvedValue(undefined);
    render(
      <WorkBuddyProviderForm
        appId="workbuddy"
        providerId="deepseek"
        initialData={{
          name: "DeepSeek",
          settingsConfig: {
            ...existing,
            url: "https://api.example.com/v1",
            api: "openai-responses",
          },
        }}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    expect(
      screen.queryByText("workbuddy.form.urlResponsesHint"),
    ).not.toBeInTheDocument();
    const save = screen.getByRole("button", { name: "Save" });
    await waitFor(() => expect(save).toBeEnabled());
    fireEvent.click(save);
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    expect(JSON.parse(submit.mock.calls[0][0].settingsConfig)).toMatchObject({
      url: "https://api.example.com/v1",
      api: "openai-responses",
    });
  });

  it("lays models out as rows with details behind the expand arrow", () => {
    render(
      <WorkBuddyProviderForm
        appId="workbuddy"
        providerId="deepseek"
        initialData={{
          name: "DeepSeek",
          settingsConfig: {
            ...existing,
            models: [
              { id: "first", maxInputTokens: 111 },
              { id: "second", maxInputTokens: 222 },
              { id: "third", maxInputTokens: 333 },
            ],
          },
        }}
        submitLabel="Save"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    expect(screen.getAllByLabelText("opencode.modelId")).toHaveLength(3);
    expect(
      screen.queryByLabelText("workbuddy.form.maxInputTokens"),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "opencode.models" }),
    ).toBeInTheDocument();

    const toggles = screen.getAllByRole("button", {
      name: "opencode.toggleModelDetails",
    });
    fireEvent.click(toggles[2]);
    expect(toggles[2]).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByLabelText("workbuddy.form.maxInputTokens")).toHaveValue(
      333,
    );
    expect(
      screen.getByLabelText("workbuddy.form.maxOutputTokens"),
    ).toBeInTheDocument();

    // 删掉第一行，展开的仍是原来那一行。
    fireEvent.click(
      screen.getAllByRole("button", { name: "common.delete" })[0],
    );
    expect(screen.getAllByLabelText("opencode.modelId")).toHaveLength(2);
    expect(screen.getByLabelText("workbuddy.form.maxInputTokens")).toHaveValue(
      333,
    );
  });

  it("toggles model capabilities with switches", async () => {
    const submit = vi.fn();
    render(
      <WorkBuddyProviderForm
        appId="workbuddy"
        providerId="deepseek"
        initialData={{ name: "DeepSeek", settingsConfig: existing }}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "opencode.toggleModelDetails" }),
    );
    const reasoning = screen.getByRole("switch", {
      name: "kimi.capabilities.thinking",
    });
    expect(reasoning).not.toBeChecked();
    fireEvent.click(reasoning);
    expect(reasoning).toBeChecked();
    fireEvent.click(
      screen.getByRole("switch", { name: "kimi.capabilities.tool_use" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    expect(
      JSON.parse(submit.mock.calls[0][0].settingsConfig).models[0],
    ).toMatchObject({ supportsReasoning: true, supportsToolCall: false });
  });

  it("rejects malformed and taken provider keys and non-http URLs", async () => {
    setProviders("workbuddy", {
      taken: { id: "taken", name: "Taken", settingsConfig: existing },
    });
    render(
      <WorkBuddyProviderForm
        appId="workbuddy"
        initialData={undefined}
        submitLabel="Add"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    const save = screen.getByRole("button", { name: "Add" });
    const key = screen.getByLabelText(/opencode.providerKey/);
    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "Mine" },
    });
    fireEvent.change(screen.getByLabelText("workbuddy.form.url"), {
      target: { value: "api.example.com/v1/chat/completions" },
    });
    fireEvent.click(screen.getByRole("button", { name: /opencode.addModel/ }));
    fireEvent.change(screen.getByLabelText("opencode.modelId"), {
      target: { value: "m" },
    });
    fireEvent.change(key, { target: { value: "mine" } });
    expect(save).toBeDisabled();

    fireEvent.change(screen.getByLabelText("workbuddy.form.url"), {
      target: { value: "https://api.example.com/v1/chat/completions" },
    });
    await waitFor(() => expect(save).toBeEnabled());

    fireEvent.change(key, { target: { value: "-bad-" } });
    expect(screen.getByText("opencode.providerKeyInvalid")).toBeInTheDocument();
    expect(save).toBeDisabled();
    fireEvent.change(key, { target: { value: "taken" } });
    expect(
      await screen.findByText("opencode.providerKeyDuplicate"),
    ).toBeInTheDocument();
    expect(save).toBeDisabled();
  });

  it("fetches models from the base of the Chat Completions URL", async () => {
    modelFetchApiMock.fetchModelsForConfig.mockResolvedValue([]);
    render(
      <WorkBuddyProviderForm
        appId="workbuddy"
        providerId="deepseek"
        initialData={{ name: "DeepSeek", settingsConfig: existing }}
        submitLabel="Save"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: /providerForm.fetchModels/ }),
    );
    await waitFor(() =>
      expect(modelFetchApiMock.fetchModelsForConfig).toHaveBeenCalledWith(
        "https://api.deepseek.com/v1",
        "local-test-key",
        undefined,
        undefined,
        undefined,
        { apiFormat: "openai-completions" },
      ),
    );
  });
});
