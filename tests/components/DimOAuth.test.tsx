import i18n from "i18next";
import {
  act,
  fireEvent,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AppId } from "@/lib/api";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import { DimOAuthFields } from "@/components/providers/forms/DimOAuthFields";
import { DimOAuthSection } from "@/components/providers/forms/DimOAuthSection";
import { geminiProviderPresets } from "@/config/geminiProviderPresets";
import { dimFormatsForApp } from "@/lib/dimProviderConfig";
import { providerPresets } from "@/config/claudeProviderPresets";
import {
  codexProviderPresets,
  generateThirdPartyConfig,
} from "@/config/codexProviderPresets";
import { presetGroup } from "@/components/providers/forms/presetGroups";
import { providerNeedsRouting } from "@/utils/providerCapabilities";
import { dimOAuthApi, type DimAccount } from "@/lib/api/dim-oauth";
import { providersApi } from "@/lib/api/providers";
import { deliverLoginLink } from "@/lib/authLoginLink";
import { renderWithQueryClient } from "../utils/testQueryClient";

vi.mock("@/lib/api/dim-oauth", () => ({
  dimOAuthApi: {
    status: vi.fn(),
    start: vi.fn(),
    poll: vi.fn(),
    cancel: vi.fn(),
    remove: vi.fn(),
    setDefault: vi.fn(),
    models: vi.fn(),
    usage: vi.fn(),
  },
}));
vi.mock("@/lib/authLoginLink", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/authLoginLink")>()),
  deliverLoginLink: vi.fn().mockResolvedValue({ linkOnly: false }),
}));
vi.mock("@/components/JsonEditor", () => ({
  default: ({ value }: { value: string }) => (
    <textarea readOnly value={value} />
  ),
}));
vi.mock("@/components/providers/forms/hooks/useManagedAccountUsers", () => ({
  useManagedAccountUsers: () => () => [],
}));

const account: DimAccount = {
  id: "dim-cn-user",
  site: "cn",
  login: "user@example.test",
  authenticated_at: 1700000000,
  requires_reauth: false,
};
const model = {
  id: "vendor/full-model-id",
  name: "Full model",
  api_formats: ["openai_chat" as const, "anthropic" as const],
  efforts: ["high", "max"],
  zdr: true,
  context_window: 65536,
  max_output_tokens: 4096,
};

function renderForm(accountId = account.id, effort = "high") {
  const submit = vi.fn();
  renderWithQueryClient(
    <ProviderForm
      appId="claude"
      onSubmit={submit}
      onCancel={vi.fn()}
      submitLabel="save-provider"
      initialData={{
        name: "DimAgent",
        category: "third_party",
        settingsConfig: {
          env: {
            ANTHROPIC_MODEL: model.id,
            ANTHROPIC_BASE_URL: "https://dimagent.cn",
          },
        },
        meta: {
          providerType: "dim_oauth",
          apiFormat: "openai_chat",
          dimOauthEffort: effort,
          authBinding: {
            source: "managed_account",
            authProvider: "dim_oauth",
            accountId,
          },
        },
      }}
    />,
  );
  return submit;
}

beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn();
  vi.spyOn(providersApi, "getClaudeDesktopDefaultRoutes").mockResolvedValue([]);
  i18n.addResource(
    "zh",
    "translation",
    "providerPreset.loginWith.dimagent",
    "使用 DimAgent 账号登录",
  );
  vi.mocked(dimOAuthApi.status).mockResolvedValue({
    accounts: [account],
    default_account_id: account.id,
  });
  vi.mocked(dimOAuthApi.models).mockResolvedValue([model]);
  vi.mocked(dimOAuthApi.usage).mockResolvedValue({
    plan_name: "Pro",
    remaining_units: 1234,
    windows: [
      {
        name: "5h",
        used: 10,
        total: 100,
        unit: "units",
        resets_at: "2026-11-04T12:00:00Z",
      },
    ],
    subscription_url: "https://dimagent.cn/account/subscription",
    pricing_url: "https://dimagent.cn/pricing",
  });
  vi.mocked(dimOAuthApi.cancel).mockResolvedValue(undefined);
  vi.mocked(dimOAuthApi.remove).mockResolvedValue(undefined);
  vi.mocked(dimOAuthApi.setDefault).mockResolvedValue(undefined);
});

describe("DimAgent native sign-in user flow", () => {
  it("appears in account sign-in for Claude Code and Codex and always requires routing", () => {
    for (const [appId, presets] of [
      ["claude", providerPresets],
      ["codex", codexProviderPresets],
    ] as const) {
      const preset = presets.find((item) => item.providerType === "dim_oauth")!;
      expect(presetGroup(preset)).toBe("login");
      expect(
        providerNeedsRouting(appId, {
          id: "dim",
          name: "DimAgent",
          settingsConfig: {},
          meta: { providerType: "dim_oauth", apiFormat: "openai_responses" },
        }),
      ).toBe(true);
    }
  });

  it("saves an explicit account binding, full model ID, protocol and declared effort", async () => {
    const submit = renderForm();
    await screen.findByLabelText("dimOauth.effort");
    await waitFor(() =>
      expect(dimOAuthApi.models).toHaveBeenCalledWith(account.id),
    );
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const payload = submit.mock.calls[0][0];
    expect(payload.meta).toMatchObject({
      providerType: "dim_oauth",
      apiFormat: "openai_chat",
      dimOauthEffort: "high",
      authBinding: {
        source: "managed_account",
        authProvider: "dim_oauth",
        accountId: account.id,
      },
    });
    expect(JSON.parse(payload.settingsConfig).env.ANTHROPIC_MODEL).toBe(
      model.id,
    );
    expect(payload.settingsConfig).not.toContain("access_token");
  });

  it.each(["claude", "codex"] as const)(
    "preserves the %s DimAgent protocol and model when imported with an official category",
    async (appId) => {
      const submit = vi.fn();
      const catalog = [
        {
          model: model.id,
          displayName: model.name,
          reasoningLevels: model.efforts,
        },
      ];
      renderWithQueryClient(
        <ProviderForm
          appId={appId}
          onSubmit={submit}
          onCancel={vi.fn()}
          submitLabel="save-provider"
          initialData={{
            name: "DimAgent",
            category: "official",
            settingsConfig:
              appId === "claude"
                ? { env: { ANTHROPIC_MODEL: model.id } }
                : {
                    auth: { OPENAI_API_KEY: "" },
                    config: generateThirdPartyConfig(
                      "dimagent",
                      "https://dimagent.cn/v1",
                      model.id,
                      { requiresOpenAiAuth: false },
                    ),
                    modelCatalog: { models: catalog },
                  },
            meta: {
              providerType: "dim_oauth",
              apiFormat: "anthropic",
              dimOauthEffort: "high",
              authBinding: {
                source: "managed_account",
                authProvider: "dim_oauth",
                accountId: account.id,
              },
            },
          }}
        />,
      );
      await screen.findByLabelText("dimOauth.effort");
      fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
      await waitFor(() => expect(submit).toHaveBeenCalledOnce());
      const payload = submit.mock.calls[0][0];
      expect(payload.presetCategory).toBe("third_party");
      expect(payload.meta).toMatchObject({
        providerType: "dim_oauth",
        apiFormat: "anthropic",
        dimOauthEffort: "high",
      });
      const settings = JSON.parse(payload.settingsConfig);
      if (appId === "codex")
        expect(settings.modelCatalog.models).toEqual(catalog);
      else expect(settings.env.ANTHROPIC_MODEL).toBe(model.id);
    },
  );

  it.each(["claude", "codex"] as const)(
    "adds a %s provider from the DimAgent sign-in preset",
    async (appId) => {
      const user = userEvent.setup();
      const submit = vi.fn();
      renderWithQueryClient(
        <ProviderForm
          appId={appId}
          onSubmit={submit}
          onCancel={vi.fn()}
          submitLabel="save-provider"
        />,
      );
      const tile = screen.getByRole("button", { name: /DimAgent/ });
      expect(
        within(tile).getByText("使用 DimAgent 账号登录"),
      ).toBeInTheDocument();
      await user.click(tile);
      await user.click(await screen.findByLabelText("dimOauth.account"));
      await user.click(
        await screen.findByRole("option", { name: /user@example.test/ }),
      );
      const modelSelect = await screen.findByLabelText("dimOauth.model");
      await waitFor(() => expect(modelSelect).not.toBeDisabled());
      await user.click(modelSelect);
      await user.click(
        await screen.findByRole("option", { name: /vendor\/full-model-id/ }),
      );
      await user.click(await screen.findByLabelText("dimOauth.format"));
      await user.click(
        await screen.findByRole("option", {
          name: "dimOauth.formats.anthropic",
        }),
      );
      await user.click(await screen.findByLabelText("dimOauth.effort"));
      await user.click(await screen.findByRole("option", { name: "max" }));
      await user.click(screen.getByRole("button", { name: "save-provider" }));
      await waitFor(() => expect(submit).toHaveBeenCalledOnce());
      const payload = submit.mock.calls[0][0];
      expect(payload.meta).toMatchObject({
        providerType: "dim_oauth",
        apiFormat: "anthropic",
        dimOauthEffort: "max",
        authBinding: {
          source: "managed_account",
          authProvider: "dim_oauth",
          accountId: account.id,
        },
      });
      const config = JSON.parse(payload.settingsConfig);
      if (appId === "claude") expect(config.env.ANTHROPIC_MODEL).toBe(model.id);
      else {
        expect(config.config).toContain(`model = "${model.id}"`);
        expect(config.auth.OPENAI_API_KEY).toBe("");
      }
    },
  );

  it("invalidates the selected model when switching to another site's account", async () => {
    const globalAccount = {
      ...account,
      id: "dim-global-user",
      site: "global" as const,
      login: "global@example.test",
    };
    vi.mocked(dimOAuthApi.status).mockResolvedValue({
      accounts: [account, globalAccount],
      default_account_id: account.id,
    });
    vi.mocked(dimOAuthApi.models).mockImplementation(async (id) =>
      id === account.id ? [model] : [],
    );
    const user = userEvent.setup();
    const submit = renderForm();
    await screen.findByLabelText("dimOauth.effort");
    await user.click(screen.getByLabelText("dimOauth.account"));
    await user.click(
      await screen.findByRole("option", { name: /global@example.test/ }),
    );
    await waitFor(() =>
      expect(dimOAuthApi.models).toHaveBeenCalledWith(globalAccount.id),
    );
    await user.click(screen.getByRole("button", { name: "save-provider" }));
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 30));
    });
    expect(submit).not.toHaveBeenCalled();
  });

  it("does not replace a missing bound account with the default", async () => {
    const submit = renderForm("deleted-account");
    await screen.findByText("dimOauth.loginRequired");
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 30));
    });
    expect(submit).not.toHaveBeenCalled();
  });

  it("blocks saving an effort removed from the selected model catalog", async () => {
    const submit = renderForm(account.id, "invented-effort");
    await screen.findByLabelText("dimOauth.effort");
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 30));
    });
    expect(submit).not.toHaveBeenCalled();
  });

  it("filters Gemini-only models for Codex and presents only declared efforts", async () => {
    vi.mocked(dimOAuthApi.models).mockResolvedValue([
      model,
      {
        id: "gemini-only",
        name: "Gemini only",
        api_formats: ["gemini_native"],
        efforts: [],
        zdr: false,
        context_window: null,
        max_output_tokens: null,
      },
    ]);
    const user = userEvent.setup();
    renderWithQueryClient(
      <DimOAuthFields
        appId="codex"
        accountId={account.id}
        onAccountChange={vi.fn()}
        model={model.id}
        apiFormat="openai_chat"
        effort=""
        onSelect={vi.fn()}
        onValidityChange={vi.fn()}
      />,
    );
    const effort = await screen.findByLabelText("dimOauth.effort");
    await user.click(effort);
    expect(screen.getByRole("option", { name: "high" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "max" })).toBeInTheDocument();
    expect(
      screen.queryByRole("option", { name: "low" }),
    ).not.toBeInTheDocument();
    await user.keyboard("{Escape}");
    await user.click(screen.getByLabelText("dimOauth.model"));
    expect(
      screen.getByRole("option", { name: /vendor\/full-model-id.*ZDR/ }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("option", { name: /Gemini only/ }),
    ).not.toBeInTheDocument();
  });

  it("disables explicit effort for Gemini Native and clears an old override", async () => {
    const gemini = { ...model, api_formats: ["gemini_native" as const] };
    vi.mocked(dimOAuthApi.models).mockResolvedValue([gemini]);
    const select = vi.fn();
    renderWithQueryClient(
      <DimOAuthFields
        appId="claude"
        accountId={account.id}
        onAccountChange={vi.fn()}
        model={model.id}
        apiFormat="gemini_native"
        effort="high"
        onSelect={select}
        onValidityChange={vi.fn()}
      />,
    );
    expect(await screen.findByLabelText("dimOauth.effort")).toBeDisabled();
    await waitFor(() =>
      expect(select).toHaveBeenCalledWith(gemini, "gemini_native", ""),
    );
  });

  it("does not offer DimAgent for Gemini CLI or Antigravity", () => {
    expect(
      geminiProviderPresets.some(
        (preset) => preset.providerType === "dim_oauth",
      ),
    ).toBe(false);
    expect(dimFormatsForApp("gemini")).toEqual([]);
    expect(dimFormatsForApp("agy")).toEqual([]);
  });

  it.each([
    "claude",
    "codex",
    "claude-desktop",
    "grokbuild",
    "mcode",
    "dsh",
    "pi",
    "omp",
    "kimi",
    "opencode",
    "openclaw",
    "hermes",
  ] as AppId[])(
    "saves a real managed %s client from its native preset flow",
    async (appId) => {
      const user = userEvent.setup();
      const submit = vi.fn();
      vi.mocked(dimOAuthApi.models).mockResolvedValue([model]);
      renderWithQueryClient(
        <ProviderForm
          appId={appId}
          onSubmit={submit}
          onCancel={vi.fn()}
          submitLabel="save-expanded-provider"
        />,
      );
      await user.click(
        await screen.findByRole("button", { name: /^DimAgent$/ }),
      );
      await user.click(await screen.findByLabelText("dimOauth.account"));
      await user.click(
        await screen.findByRole("option", { name: /user@example.test/ }),
      );
      const picker = await screen.findByLabelText("dimOauth.model");
      await waitFor(() => expect(picker).not.toBeDisabled());
      await user.click(picker);
      await user.click(
        await screen.findByRole("option", { name: /vendor\/full-model-id/ }),
      );
      await user.click(
        screen.getByRole("button", { name: "save-expanded-provider" }),
      );
      await waitFor(() => expect(submit).toHaveBeenCalledOnce());
      const payload = submit.mock.calls[0][0];
      expect(payload.meta).toMatchObject({
        providerType: "dim_oauth",
        authBinding: {
          source: "managed_account",
          authProvider: "dim_oauth",
          accountId: account.id,
        },
      });
      if (appId === "claude-desktop")
        expect(Object.values(payload.meta.claudeDesktopModelRoutes)).toEqual(
          expect.arrayContaining([
            expect.objectContaining({ model: model.id }),
          ]),
        );
      else expect(payload.settingsConfig).toContain(model.id);
      expect(payload.settingsConfig).not.toMatch(/access_token|refresh_token/);
      const config = JSON.parse(payload.settingsConfig);
      if (appId === "mcode" || appId === "dsh")
        expect(config).toMatchObject({
          api: "openai-completions",
          options: {
            baseURL: "https://dimagent.cn/v1",
            apiKey: "PROXY_MANAGED",
          },
          models: {
            [model.id]: {
              name: model.name,
              limit: { context: 65536, output: 4096 },
            },
          },
        });
      if (appId === "kimi")
        expect(Object.values(config.models)[0]).toMatchObject({
          model: model.id,
          max_context_size: 65536,
          provider: "cc-switch-dimagent",
        });
      if (appId === "pi" || appId === "omp")
        expect(config.models[0]).toMatchObject({
          id: model.id,
          contextWindow: 65536,
          maxTokens: 4096,
        });
      if (appId === "claude-desktop")
        expect(payload.meta.claudeDesktopMode).toBe("proxy");
      if (
        [
          "mcode",
          "dsh",
          "pi",
          "omp",
          "kimi",
          "opencode",
          "openclaw",
          "hermes",
        ].includes(appId)
      )
        expect(
          providerNeedsRouting(appId, {
            id: "dim",
            name: "DimAgent",
            settingsConfig: config,
            meta: payload.meta,
          }),
        ).toBe(false);
    },
  );

  it("preserves an imported native OpenCode declaration when changing Dim protocol", async () => {
    const user = userEvent.setup();
    const submit = vi.fn();
    renderWithQueryClient(
      <ProviderForm
        appId="opencode"
        providerId="dim-v2"
        onSubmit={submit}
        onCancel={vi.fn()}
        submitLabel="save-native-dim"
        initialData={{
          name: "DimAgent",
          settingsConfig: {
            package: "@ai-sdk/openai-compatible",
            settings: {
              baseURL: "https://dimagent.cn/v1",
              apiKey: "PROXY_MANAGED",
            },
            models: { [model.id]: { name: model.name } },
          },
          meta: {
            providerType: "dim_oauth",
            opencodeConfigFormat: "v2",
            apiFormat: "openai_chat",
            authBinding: {
              source: "managed_account",
              authProvider: "dim_oauth",
              accountId: account.id,
            },
          },
        }}
      />,
    );
    await user.click(await screen.findByLabelText("dimOauth.format"));
    await user.click(
      await screen.findByRole("option", { name: "dimOauth.formats.anthropic" }),
    );
    await user.click(screen.getByRole("button", { name: "save-native-dim" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const payload = submit.mock.calls[0][0];
    expect(payload.meta.opencodeConfigFormat).toBe("v2");
    expect(JSON.parse(payload.settingsConfig)).toMatchObject({
      package: "@ai-sdk/anthropic",
      settings: { baseURL: "https://dimagent.cn/v1", apiKey: "PROXY_MANAGED" },
      models: { [model.id]: { name: model.name } },
    });
  });

  it.each(["kimi", "pi", "opencode"] as AppId[])(
    "uses the %s SDK's actual Gemini base convention",
    async (appId) => {
      const user = userEvent.setup();
      const submit = vi.fn();
      vi.mocked(dimOAuthApi.models).mockResolvedValue([
        { ...model, api_formats: ["gemini_native"] },
      ]);
      renderWithQueryClient(
        <ProviderForm
          appId={appId}
          onSubmit={submit}
          onCancel={vi.fn()}
          submitLabel="save-gemini-dim"
        />,
      );
      await user.click(
        await screen.findByRole("button", { name: /^DimAgent$/ }),
      );
      await user.click(await screen.findByLabelText("dimOauth.account"));
      await user.click(
        await screen.findByRole("option", { name: /user@example.test/ }),
      );
      const picker = await screen.findByLabelText("dimOauth.model");
      await waitFor(() => expect(picker).not.toBeDisabled());
      await user.click(picker);
      await user.click(
        await screen.findByRole("option", { name: /vendor\/full-model-id/ }),
      );
      await user.click(screen.getByRole("button", { name: "save-gemini-dim" }));
      await waitFor(() => expect(submit).toHaveBeenCalledOnce());
      const config = JSON.parse(submit.mock.calls[0][0].settingsConfig);
      expect(
        appId === "kimi"
          ? config.provider.base_url
          : appId === "pi"
            ? config.baseUrl
            : config.options.baseURL,
      ).toBe(
        appId === "kimi" ? "https://dimagent.cn" : "https://dimagent.cn/v1beta",
      );
    },
  );

  it("clears a MiniMax Dim selection when another preset replaces its native configuration", async () => {
    const user = userEvent.setup();
    const submit = vi.fn();
    renderWithQueryClient(
      <ProviderForm
        appId="mcode"
        onSubmit={submit}
        onCancel={vi.fn()}
        submitLabel="save-returned-dim"
      />,
    );
    await user.click(await screen.findByRole("button", { name: /^DimAgent$/ }));
    await user.click(await screen.findByLabelText("dimOauth.account"));
    await user.click(
      await screen.findByRole("option", { name: /user@example.test/ }),
    );
    const picker = await screen.findByLabelText("dimOauth.model");
    await waitFor(() => expect(picker).not.toBeDisabled());
    await user.click(picker);
    await user.click(
      await screen.findByRole("option", { name: /vendor\/full-model-id/ }),
    );
    await user.click(screen.getByRole("button", { name: /^OpenAI API$/ }));
    await user.click(screen.getByRole("button", { name: /^DimAgent$/ }));
    expect(await screen.findByLabelText("dimOauth.model")).toHaveTextContent(
      "dimOauth.selectModel",
    );
    expect(
      screen.getByRole("button", { name: "save-returned-dim" }),
    ).toBeDisabled();
    expect(submit).not.toHaveBeenCalled();
  });

  it("excludes Kimi models without an explicit positive context window", async () => {
    vi.mocked(dimOAuthApi.models).mockResolvedValue([
      { ...model, context_window: null },
    ]);
    renderWithQueryClient(
      <DimOAuthFields
        appId="kimi"
        accountId={account.id}
        onAccountChange={vi.fn()}
        model={model.id}
        apiFormat="openai_chat"
        effort=""
        onSelect={vi.fn()}
        onValidityChange={vi.fn()}
      />,
    );
    expect(
      await screen.findByText("dimOauth.kimiContextRequired"),
    ).toBeInTheDocument();
  });

  it("starts browser login, shows the bound site and real usage after completion", async () => {
    i18n.addResource(
      "zh",
      "translation",
      "dimOauth.remaining",
      "Remaining credits: {{amount}} units",
    );
    vi.mocked(dimOAuthApi.status).mockResolvedValueOnce({
      accounts: [],
      default_account_id: null,
    });
    vi.mocked(dimOAuthApi.start).mockResolvedValue({
      flow_id: "flow-1",
      auth_url: "https://dimagent.cn/oauth/authorize?state=test",
      expires_in: 300,
      interval: 0.01,
    });
    vi.mocked(dimOAuthApi.poll).mockResolvedValue(account);
    renderWithQueryClient(<DimOAuthSection />);
    fireEvent.click(await screen.findByRole("button", { name: /添加账号/ }));
    await waitFor(() => expect(dimOAuthApi.start).toHaveBeenCalledWith("cn"));
    await waitFor(() =>
      expect(deliverLoginLink).toHaveBeenCalledWith(
        "https://dimagent.cn/oauth/authorize?state=test",
        expect.anything(),
      ),
    );
    await screen.findByText(account.login);
    expect(await screen.findByText(/1,234/)).toBeInTheDocument();
    expect(screen.getByText(/10 \/ 100 units/)).toBeInTheDocument();
  });
});
