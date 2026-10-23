import type { ReactElement } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render as rtlRender,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { beforeAll, describe, expect, it, vi } from "vitest";
import {
  CodebuddyProviderForm,
  codebuddyModelsBase,
} from "@/components/providers/forms/CodebuddyProviderForm";
import { codebuddyProviderPresets } from "@/config/codebuddyProviderPresets";
import { fetchModelsForConfig } from "@/lib/api/model-fetch";
import { MODELS_DEV_API_URL } from "@/lib/modelsDev";
import { server } from "../msw/server";
import { setProviders } from "../msw/state";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/lib/api/model-fetch", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api/model-fetch")>()),
  fetchModelsForConfig: vi.fn(),
}));

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

// 模型参数收在模型行的展开区里，和 Kimi、Pi、OpenCode 一样。
const expandModel = () =>
  fireEvent.click(
    screen.getByRole("button", { name: "opencode.toggleModelDetails" }),
  );

const render = (ui: ReactElement) =>
  rtlRender(
    <QueryClientProvider client={createTestQueryClient()}>
      {ui}
    </QueryClientProvider>,
  );

beforeAll(() => {
  Element.prototype.scrollIntoView ??= vi.fn();
});

describe("CodebuddyProviderForm", () => {
  it("edits native fields while preserving unknown model settings", async () => {
    const submit = vi.fn();
    render(
      <CodebuddyProviderForm
        appId="codebuddy"
        providerId="my-model"
        initialData={{
          name: "My Model",
          settingsConfig: {
            id: "my-model",
            name: "My Model",
            vendor: "OpenAI",
            url: "https://api.example.com/v1/chat/completions",
            apiKey: "local-test-key",
            maxOutputTokens: 4096,
            temperature: 0.2,
            reasoning: { defaultEffort: "high" },
          },
        }}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    expect(screen.getByLabelText("opencode.modelId")).toBeDisabled();
    expect(screen.getByLabelText("opencode.modelName")).toHaveValue("My Model");
    expect(
      screen.queryByRole("button", { name: "providerForm.fetchModels" }),
    ).not.toBeInTheDocument();
    expect(screen.getByLabelText("API Key")).toHaveAttribute(
      "type",
      "password",
    );
    expect(
      screen.queryByLabelText("codebuddy.maxOutputTokens"),
    ).not.toBeInTheDocument();
    expandModel();
    expect(
      screen.getByRole("switch", { name: "kimi.capabilities.tool_use" }),
    ).not.toBeChecked();
    // 显示名称就是卡片名称：两处改的是同一个值。
    fireEvent.change(screen.getByLabelText("opencode.modelName"), {
      target: { value: "Renamed" },
    });
    expect(screen.getByLabelText("provider.name")).toHaveValue("Renamed");
    fireEvent.change(screen.getByLabelText("codebuddy.maxOutputTokens"), {
      target: { value: "8192" },
    });
    fireEvent.click(
      screen.getByRole("switch", { name: "kimi.capabilities.thinking" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const values = submit.mock.calls[0][0];
    expect(values.providerKey).toBe("my-model");
    expect(JSON.parse(values.settingsConfig)).toEqual({
      name: "Renamed",
      vendor: "OpenAI",
      url: "https://api.example.com/v1/chat/completions",
      apiKey: "local-test-key",
      maxOutputTokens: 8192,
      supportsReasoning: true,
      temperature: 0.2,
      reasoning: { defaultEffort: "high" },
    });
  });

  it("requires a model id and a valid endpoint, and rejects taken ids", async () => {
    setProviders("codebuddy", {
      taken: {
        id: "taken",
        name: "Taken",
        settingsConfig: { url: "https://e.example" },
      },
    });
    const submit = vi.fn();
    render(
      <CodebuddyProviderForm
        appId="codebuddy"
        submitLabel="Add"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    const save = screen.getByRole("button", { name: "Add" });
    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "New" },
    });
    fireEvent.change(screen.getByLabelText("providerForm.apiEndpoint"), {
      target: { value: "not a url" },
    });
    fireEvent.change(screen.getByLabelText("opencode.modelId"), {
      target: { value: "new-model" },
    });
    expect(save).toBeDisabled();
    expect(screen.getByText("codebuddy.urlInvalid")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("providerForm.apiEndpoint"), {
      target: { value: "https://api.example.com/v1/chat/completions" },
    });
    fireEvent.change(screen.getByLabelText("opencode.modelId"), {
      target: { value: "taken" },
    });
    await waitFor(() =>
      expect(
        screen.getByText("codebuddy.modelIdDuplicate"),
      ).toBeInTheDocument(),
    );
    expect(save).toBeDisabled();

    fireEvent.change(screen.getByLabelText("opencode.modelId"), {
      target: { value: "new-model" },
    });
    await waitFor(() => expect(save).toBeEnabled());
    fireEvent.click(save);
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const values = submit.mock.calls[0][0];
    expect(values.providerKey).toBe("new-model");
    expect(JSON.parse(values.settingsConfig)).toEqual({
      name: "New",
      url: "https://api.example.com/v1/chat/completions",
      supportsToolCall: true,
    });
  });

  it("fetches models from the endpoint derived from the url", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValue([
      { id: "deepseek-chat", ownedBy: null },
    ]);
    render(
      <CodebuddyProviderForm
        appId="codebuddy"
        submitLabel="Add"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    fireEvent.change(screen.getByLabelText("providerForm.apiEndpoint"), {
      target: { value: "https://api.example.com/v1/chat/completions" },
    });
    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "local-test-key" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await waitFor(() =>
      expect(fetchModelsForConfig).toHaveBeenCalledWith(
        "https://api.example.com/v1",
        "local-test-key",
        false,
      ),
    );
  });

  it("fills the picked model's parameters from models.dev", async () => {
    const user = userEvent.setup();
    vi.mocked(fetchModelsForConfig).mockResolvedValue([
      { id: "model-x", ownedBy: "example" },
    ]);
    server.use(
      http.get(MODELS_DEV_API_URL, () =>
        HttpResponse.json({
          example: {
            api: "https://api.example.com/v1",
            models: {
              "model-x": {
                limit: { context: 123456, output: 8192 },
                reasoning: true,
                modalities: { input: ["text", "image"], output: ["text"] },
              },
            },
          },
        }),
      ),
    );
    render(
      <CodebuddyProviderForm
        appId="codebuddy"
        submitLabel="Add"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    fireEvent.change(screen.getByLabelText("providerForm.apiEndpoint"), {
      target: { value: "https://api.example.com/v1/chat/completions" },
    });
    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "local-test-key" },
    });
    await user.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await user.click(
      await screen.findByRole("button", { name: "Select model" }),
    );
    await user.click(await screen.findByRole("option", { name: "model-x" }));
    expect(screen.getByLabelText("opencode.modelId")).toHaveValue("model-x");
    expandModel();
    await waitFor(() =>
      expect(screen.getByLabelText("codebuddy.maxInputTokens")).toHaveValue(
        123456,
      ),
    );
    expect(screen.getByLabelText("codebuddy.maxOutputTokens")).toHaveValue(
      8192,
    );
    expect(
      screen.getByRole("switch", { name: "kimi.capabilities.image_in" }),
    ).toBeChecked();
    expect(
      screen.getByRole("switch", { name: "kimi.capabilities.thinking" }),
    ).toBeChecked();
  });

  it("derives the models endpoint from the chat-completions url", () => {
    expect(
      codebuddyModelsBase("https://api.example.com/v1/chat/completions/"),
    ).toEqual({ baseUrl: "https://api.example.com/v1", isFullUrl: false });
    expect(
      codebuddyModelsBase("https://gateway.example.com/openai/chat"),
    ).toEqual({
      baseUrl: "https://gateway.example.com/openai/chat",
      isFullUrl: true,
    });
  });

  it("has no API format menu and keeps an existing api field on save", async () => {
    const submit = vi.fn();
    render(
      <CodebuddyProviderForm
        appId="codebuddy"
        providerId="legacy"
        initialData={{
          name: "Legacy",
          settingsConfig: {
            id: "legacy",
            name: "Legacy",
            url: "https://api.example.com/v1/responses",
            api: "openai-responses",
          },
        }}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    expect(
      screen.queryByLabelText("opencode.npmPackage"),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    expect(screen.getByText("codebuddy.urlHint")).toBeInTheDocument();
    expect(
      (screen.getByLabelText("provider.configJson") as HTMLTextAreaElement)
        .value,
    ).toContain('"api": "openai-responses"');
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    expect(JSON.parse(submit.mock.calls[0][0].settingsConfig)).toEqual({
      name: "Legacy",
      url: "https://api.example.com/v1/responses",
      api: "openai-responses",
    });
  });

  describe("official sign-in card", () => {
    const account = (uid: string, domain: string, login: string) => ({
      id: `${uid}@${domain}`,
      provider: "codebuddy_oauth",
      login,
      avatar_url: null,
      authenticated_at: 0,
      is_default: false,
      github_domain: domain,
      requires_reauth: false,
    });
    const useAccounts = (current: string | null) =>
      server.use(
        http.post("http://tauri.local/auth_get_status", () =>
          HttpResponse.json({
            provider: "codebuddy_oauth",
            authenticated: true,
            default_account_id: current,
            accounts: [
              account("1", "www.codebuddy.cn", "alice"),
              account("2", "www.codebuddy.ai", "bob"),
            ],
          }),
        ),
        http.post("http://tauri.local/codebuddy_account_live", () =>
          HttpResponse.json({
            state: "collected",
            account_id: current,
            label: null,
            domain: null,
          }),
        ),
        http.post("http://tauri.local/codebuddy_account_quota", () =>
          HttpResponse.json({
            credentialStatus: "valid",
            success: true,
            planName: "体验版",
            isPaid: false,
            enterprise: false,
            unlimited: false,
            total: 200,
            remaining: 150,
            used: 50,
            error: null,
            queriedAt: Date.now(),
          }),
        ),
      );

    it("binds a new card to the current account and names it after the account", async () => {
      useAccounts("1@www.codebuddy.cn");
      const submit = vi.fn();
      render(
        <CodebuddyProviderForm
          appId="codebuddy"
          submitLabel="Add"
          onSubmit={submit}
          onCancel={() => {}}
        />,
      );
      fireEvent.click(
        await screen.findByRole("button", {
          name: /codebuddy\.official\.presetName/,
        }),
      );
      // 不填模型、地址、Key，只选账号
      expect(
        screen.queryByLabelText("providerForm.apiEndpoint"),
      ).not.toBeInTheDocument();
      expect(screen.queryByLabelText("API Key")).not.toBeInTheDocument();
      const alice = await screen.findByRole("radio", { name: /alice/ });
      await waitFor(() =>
        expect(alice).toHaveAttribute("aria-checked", "true"),
      );
      expect(screen.getByLabelText("provider.name")).toHaveValue("alice");
      expect(screen.getByText("CodeBuddy 在用")).toBeInTheDocument();

      // 换成 bob，名字跟着换
      fireEvent.click(screen.getByRole("radio", { name: /bob/ }));
      expect(screen.getByLabelText("provider.name")).toHaveValue("bob");
      const save = screen.getByRole("button", { name: "Add" });
      await waitFor(() => expect(save).toBeEnabled());
      fireEvent.click(save);
      await waitFor(() => expect(submit).toHaveBeenCalledOnce());
      const values = submit.mock.calls[0][0];
      expect(values.presetCategory).toBe("official");
      expect(values.providerKey).toMatch(/^codebuddy-official-/);
      expect(JSON.parse(values.settingsConfig)).toEqual({});
      expect(values.meta.authBinding).toEqual({
        source: "managed_account",
        authProvider: "codebuddy_oauth",
        accountId: "2@www.codebuddy.ai",
      });
    });

    it("flags a card whose bound account was removed", async () => {
      useAccounts(null);
      const submit = vi.fn();
      render(
        <CodebuddyProviderForm
          appId="codebuddy"
          providerId="codebuddy-official-x"
          initialData={{
            name: "Old",
            category: "official",
            settingsConfig: {},
            meta: {
              authBinding: {
                source: "managed_account",
                authProvider: "codebuddy_oauth",
                accountId: "9@www.codebuddy.ai",
              },
            },
          }}
          submitLabel="Save"
          onSubmit={submit}
          onCancel={() => {}}
        />,
      );
      expect(
        await screen.findByText("账号已删除，请重新选择"),
      ).toBeInTheDocument();
      fireEvent.click(screen.getByRole("radio", { name: /alice/ }));
      // 用户自己起的名字不被账号名覆盖
      expect(screen.getByLabelText("provider.name")).toHaveValue("Old");
      fireEvent.click(screen.getByRole("button", { name: "Save" }));
      await waitFor(() => expect(submit).toHaveBeenCalledOnce());
      const values = submit.mock.calls[0][0];
      expect(values.providerKey).toBe("codebuddy-official-x");
      expect(values.meta.authBinding.accountId).toBe("1@www.codebuddy.cn");
    });
  });

  it("ships presets that use the documented chat-completions endpoints", () => {
    expect(codebuddyProviderPresets.length).toBeGreaterThan(0);
    for (const preset of codebuddyProviderPresets) {
      // 官方登录预设不是 models.json 里的模型，没有 url
      if (preset.category === "official") continue;
      expect(preset.providerKey.trim()).toBe(preset.providerKey);
      expect(preset.settingsConfig.url).toMatch(
        /^https:\/\/.+\/chat\/completions$/,
      );
      expect(preset.settingsConfig).not.toHaveProperty("id");
    }
  });
});
