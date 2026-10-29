import { beforeAll, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import { readFileSync } from "node:fs";
import { parse as parseToml } from "smol-toml";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import tr from "@/i18n/locales/tr.json";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import {
  buildGrokBuildConfig,
  parseGrokBuildConfig,
} from "@/utils/grokBuildConfig";
import { CLAUDE_DESKTOP_ROLE_ROUTE_IDS } from "@/config/claudeDesktopProviderPresets";
import {
  extractProviderTransferFields,
  prepareProviderTransfer,
  type ProviderTransferFields,
} from "@/utils/providerTransfer";

vi.mock("@/lib/api/providers", () => ({
  providersApi: {
    getClaudeDesktopDefaultRoutes: vi.fn(async () =>
      Object.entries(CLAUDE_DESKTOP_ROLE_ROUTE_IDS).map(([role, routeId]) => ({
        routeId,
        envKey: `ANTHROPIC_DEFAULT_${role.toUpperCase()}_MODEL`,
        supports1m: true,
      })),
    ),
  },
}));

const locales = { zh, "zh-TW": zhTW, en, ja, tr };
beforeAll(async () => {
  for (const [lng, resources] of Object.entries(locales))
    i18n.addResourceBundle(lng, "translation", resources, true, true);
  await i18n.changeLanguage("zh");
});

const URL = "https://custom.example/prefix/v1";
const KEY = "test-key";
const fields = (
  overrides: Partial<ProviderTransferFields> = {},
): ProviderTransferFields => ({
  sourceApp: "claude",
  name: "Copied card",
  websiteUrl: "https://custom.example",
  notes: "notes",
  icon: "anthropic",
  iconColor: "#123456",
  baseUrl: URL,
  apiKey: KEY,
  apiKeyField: "ANTHROPIC_API_KEY",
  apiFormat: "anthropic",
  models: [{ id: "model-a", name: "Model A" }],
  ...overrides,
});
const card = (
  settingsConfig: Provider["settingsConfig"],
  meta?: Provider["meta"],
): Provider => ({
  id: "old-id",
  name: "Source",
  settingsConfig,
  meta,
  notes: "notes",
  icon: "anthropic",
  iconColor: "#123456",
  websiteUrl: "https://custom.example",
  createdAt: 123,
  sortIndex: 99,
  inFailoverQueue: true,
});
const env = {
  ANTHROPIC_BASE_URL: URL,
  ANTHROPIC_API_KEY: KEY,
  ANTHROPIC_MODEL: "model-a",
};
const codexConfig = `model_provider = "selected"\nmodel = "model-a"\n[model_providers.other]\nbase_url = "https://wrong.example"\n[model_providers.selected]\nbase_url = "${URL}"\nwire_api = "responses"`;
const configs: Record<AppId, Provider["settingsConfig"]> = {
  claude: { env },
  "claude-desktop": { env },
  codex: { auth: { OPENAI_API_KEY: KEY }, config: codexConfig },
  gemini: {
    env: {
      GOOGLE_GEMINI_BASE_URL: URL,
      GEMINI_API_KEY: KEY,
      GEMINI_MODEL: "model-a",
    },
  },
  agy: {
    env: { GOOGLE_GEMINI_BASE_URL: URL, GEMINI_API_KEY: KEY },
    config: { model: { name: "model-a" } },
  },
  grokbuild: {
    config: buildGrokBuildConfig({
      ...parseGrokBuildConfig(undefined),
      name: "Source",
      apiKey: KEY,
      baseUrl: URL,
      upstreamModel: "model-a",
    }),
  },
  opencode: {
    npm: "@ai-sdk/anthropic",
    options: { baseURL: URL, apiKey: KEY },
    models: { "model-a": { name: "A" } },
  },
  mcode: {
    kind: "custom",
    api: "anthropic-messages",
    options: { baseURL: URL, apiKey: KEY },
    models: { "model-a": { name: "A" } },
  },
  dsh: {
    kind: "custom",
    enabled: true,
    api: "anthropic-messages",
    options: { baseURL: URL, apiKey: KEY },
    models: { "model-a": { name: "A", limit: { context: 200000 } } },
  },
  openclaw: {
    api: "anthropic-messages",
    baseUrl: URL,
    apiKey: KEY,
    models: [{ id: "model-a", name: "A" }],
  },
  pi: {
    api: "anthropic-messages",
    baseUrl: URL,
    apiKey: KEY,
    models: [{ id: "model-a", name: "A" }],
  },
  omp: {
    api: "anthropic-messages",
    baseUrl: URL,
    apiKey: KEY,
    models: [{ id: "model-a", name: "A" }],
  },
  hermes: {
    api_mode: "anthropic_messages",
    base_url: URL,
    api_key: KEY,
    models: [{ id: "model-a", name: "A" }],
  },
  kimi: {
    provider: { type: "anthropic", base_url: URL, api_key: KEY },
    models: {
      alias: { model: "model-a", display_name: "A", max_context_size: 200000 },
    },
  },
};

const apps = Object.keys(configs) as AppId[];
describe("provider transfer extraction", () => {
  it.each(apps)(
    "extracts common fields from %s without carrying source config",
    (app) => {
      const meta =
        app === "claude-desktop"
          ? {
              claudeDesktopMode: "proxy" as const,
              claudeDesktopModelRoutes: {
                "claude-sonnet-5": { model: "model-a" },
              },
            }
          : undefined;
      const source = card(
        {
          ...configs[app],
          permissions: { allow: ["*"] },
          hooks: { secret: "do-not-copy" },
          mcpServers: { secret: {} },
        },
        meta,
      );
      const before = JSON.stringify(source);
      const copied = extractProviderTransferFields(source, app);
      expect(copied).toMatchObject({
        sourceApp: app,
        baseUrl: URL,
        apiKey: KEY,
        name: "Source",
        icon: "anthropic",
        models: expect.arrayContaining([
          expect.objectContaining({ id: "model-a" }),
        ]),
      });
      expect(copied).not.toHaveProperty("settingsConfig");
      expect(JSON.stringify(copied)).not.toContain("do-not-copy");
      expect(JSON.stringify(copied)).not.toContain("old-id");
      expect(JSON.stringify(source)).toBe(before);
    },
  );

  it("reads Codex auth and selected table rather than legacy env or unselected URL", () => {
    const source = card({
      ...configs.codex,
      env: { CODEX_API_KEY: "wrong" },
      modelCatalog: {
        models: [
          { model: "model-a", displayName: "A", contextWindow: "1000000" },
          { model: "model-b", display_name: "B", context_window: 200000 },
        ],
      },
    });
    expect(extractProviderTransferFields(source, "codex")).toMatchObject({
      apiKey: KEY,
      baseUrl: URL,
      models: [
        { id: "model-a", name: "A", primary: true, contextWindow: 1000000 },
        { id: "model-b", name: "B", primary: false, contextWindow: 200000 },
      ],
    });
  });

  it("supports Codex experimental bearer token without copying login identity", () => {
    const source = card({
      auth: { tokens: { access_token: "do-not-copy" } },
      config: `${codexConfig}\nexperimental_bearer_token = "static-key"`,
    });
    const copied = extractProviderTransferFields(source, "codex");
    expect(copied.apiKey).toBe("static-key");
    expect(JSON.stringify(copied)).not.toContain("access_token");
  });

  it("extracts OpenCode V2 native settings and upstream model IDs", () => {
    const source = card({
      package: "@ai-sdk/anthropic",
      settings: { baseURL: URL, apiKey: KEY },
      models: { alias: { modelID: "upstream", name: "Display" } },
    });
    expect(extractProviderTransferFields(source, "opencode")).toMatchObject({
      baseUrl: URL,
      apiKey: KEY,
      apiFormat: "anthropic",
      models: [{ id: "upstream", name: "Display" }],
    });
  });

  it.each([
    card({ env }, { providerType: "codex_oauth" }),
    card(
      { env },
      { authBinding: { source: "managed_account", accountId: "account" } },
    ),
    card({ env: { ...env, CLAUDE_CODE_USE_BEDROCK: "1" } }),
    card({ env: { ...env, ANTHROPIC_API_KEY: "PROXY_MANAGED" } }),
    card({ env: { ...env, ANTHROPIC_API_KEY: "${SECRET}" } }),
    card({ env: {} }),
  ])(
    "rejects account authentication, cloud auth and unresolved credentials",
    (source) => {
      expect(() => extractProviderTransferFields(source, "claude")).toThrow();
    },
  );

  it("keeps auth field, protocol and only necessary request overrides", () => {
    const source = card(
      {
        env: {
          ...env,
          ANTHROPIC_AUTH_TOKEN: "other-key",
          UNRELATED_SECRET: "do-not-copy",
        },
        hooks: {},
      },
      {
        apiKeyField: "ANTHROPIC_API_KEY",
        apiFormat: "openai_chat",
        isFullUrl: true,
        localProxyRequestOverrides: {
          headers: { "x-route": "a" },
          body: { store: false },
        },
        customUserAgent: "client",
        commonConfigEnabled: true,
        liveConfigManaged: true,
      },
    );
    const copied = extractProviderTransferFields(source, "claude");
    expect(copied).toMatchObject({
      apiKey: KEY,
      apiKeyField: "ANTHROPIC_API_KEY",
      apiFormat: "openai_chat",
      isFullUrl: true,
      requestOverrides: { headers: { "x-route": "a" }, body: { store: false } },
    });
    expect(JSON.stringify(copied)).not.toMatch(
      /do-not-copy|other-key|liveConfigManaged|commonConfigEnabled/,
    );
  });
});

describe("provider transfer target drafts", () => {
  it.each(apps)(
    "builds a new %s draft with target credentials and metadata",
    async (app) => {
      const input = fields();
      const before = JSON.stringify(input);
      const { provider } = await prepareProviderTransfer(input, app);
      expect(provider).toMatchObject({
        id: "",
        name: input.name,
        icon: input.icon,
        websiteUrl: input.websiteUrl,
        notes: input.notes,
        category: "custom",
      });
      expect(provider).not.toHaveProperty("createdAt");
      expect(provider).not.toHaveProperty("sortIndex");
      expect(provider).not.toHaveProperty("inFailoverQueue");
      expect(provider.meta?.authBinding).toBeUndefined();
      const copied = extractProviderTransferFields(provider, app);
      expect(copied).toMatchObject({
        baseUrl: URL,
        apiKey: KEY,
        models: expect.arrayContaining([
          expect.objectContaining({ id: "model-a" }),
        ]),
      });
      expect(JSON.stringify(input)).toBe(before);
    },
  );

  it("preserves Codex default template and writes all catalog models", async () => {
    const { provider } = await prepareProviderTransfer(
      fields({
        models: [
          { id: "a", name: "A", contextWindow: 1000000 },
          { id: "b", primary: true },
        ],
      }),
      "codex",
    );
    const toml = parseToml(provider.settingsConfig.config);
    expect(toml).toMatchObject({
      model_provider: "custom",
      model: "b",
      model_reasoning_effort: "high",
      disable_response_storage: true,
      model_providers: {
        custom: {
          base_url: URL,
          wire_api: "responses",
          requires_openai_auth: true,
        },
      },
    });
    expect(provider.settingsConfig.auth).toEqual({ OPENAI_API_KEY: KEY });
    expect(provider.settingsConfig.modelCatalog.models).toEqual([
      { model: "a", displayName: "A", contextWindow: 1000000 },
      { model: "b", displayName: "b" },
    ]);
  });

  it("preserves role labels and 1m across Claude CLI/Desktop", async () => {
    const source = card({
      env: {
        ...env,
        ANTHROPIC_DEFAULT_OPUS_MODEL: "upstream[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL_NAME: "Opus label",
        ANTHROPIC_DEFAULT_FABLE_MODEL: "fable-upstream",
      },
    });
    const desktop = await prepareProviderTransfer(
      extractProviderTransferFields(source, "claude"),
      "claude-desktop",
    );
    expect(
      desktop.provider.meta?.claudeDesktopModelRoutes?.[
        CLAUDE_DESKTOP_ROLE_ROUTE_IDS.opus
      ],
    ).toMatchObject({
      model: "upstream",
      labelOverride: "Opus label",
      supports1m: true,
    });
    const cli = await prepareProviderTransfer(
      extractProviderTransferFields(desktop.provider, "claude-desktop"),
      "claude",
    );
    expect(cli.provider.settingsConfig.env).toMatchObject({
      ANTHROPIC_DEFAULT_OPUS_MODEL: "upstream[1m]",
      ANTHROPIC_DEFAULT_OPUS_MODEL_NAME: "Opus label",
      ANTHROPIC_DEFAULT_FABLE_MODEL: "fable-upstream",
    });
    expect(cli.provider.meta?.claudeDesktopMode).toBeUndefined();
  });

  it("fills only the four native Desktop slots and names every omitted model", async () => {
    const result = await prepareProviderTransfer(
      fields({
        models: [
          { id: "a", role: "sonnet", name: "First" },
          { id: "b", role: "sonnet", name: "Second" },
          { id: "c", name: "Custom" },
          { id: "d" },
          { id: "e" },
          { id: "f" },
        ],
      }),
      "claude-desktop",
    );
    const routes = result.provider.meta?.claudeDesktopModelRoutes ?? {};
    // The Desktop form normalizes proxy routes to exactly these four IDs.
    expect(Object.keys(routes).sort()).toEqual(
      Object.values(CLAUDE_DESKTOP_ROLE_ROUTE_IDS).sort(),
    );
    expect(
      Object.fromEntries(
        Object.entries(CLAUDE_DESKTOP_ROLE_ROUTE_IDS).map(([role, id]) => [
          role,
          routes[id].model,
        ]),
      ),
    ).toEqual({ sonnet: "a", opus: "c", fable: "d", haiku: "e" });
    expect(routes[CLAUDE_DESKTOP_ROLE_ROUTE_IDS.opus].labelOverride).toBe(
      "Custom",
    );
    const unmapped = result.warnings.find((warning) =>
      warning.includes("四个角色槽"),
    )!;
    expect(unmapped).toContain("b (Second)");
    expect(unmapped).toContain("f");
    expect(unmapped).not.toContain("Custom");
  });

  it("puts the primary model in the first free Desktop slot before other unassigned models", async () => {
    const result = await prepareProviderTransfer(
      fields({
        models: [
          { id: "x", role: "opus" },
          { id: "y" },
          { id: "main", primary: true },
        ],
      }),
      "claude-desktop",
    );
    const routes = result.provider.meta!.claudeDesktopModelRoutes!;
    const model = (role: keyof typeof CLAUDE_DESKTOP_ROLE_ROUTE_IDS) =>
      routes[CLAUDE_DESKTOP_ROLE_ROUTE_IDS[role]].model;
    expect([
      model("sonnet"),
      model("opus"),
      model("fable"),
      model("haiku"),
    ]).toEqual(["main", "x", "y", "main"]);
    expect(result.warnings.join()).not.toContain("四个角色槽");
  });

  it("does not hand Desktop a full URL or request overrides its form would drop", async () => {
    const result = await prepareProviderTransfer(
      fields({
        isFullUrl: true,
        customUserAgent: "agent/1",
        requestOverrides: { headers: { "X-Test": "1" } },
      }),
      "claude-desktop",
    );
    expect(result.provider.meta?.isFullUrl).toBeUndefined();
    expect(result.provider.meta?.customUserAgent).toBeUndefined();
    expect(result.provider.meta?.localProxyRequestOverrides).toBeUndefined();
    expect(result.warnings).toEqual(
      expect.arrayContaining([
        i18n.t("provider.transfer.warnings.fullUrl"),
        i18n.t("provider.transfer.warnings.advancedDropped"),
      ]),
    );
  });

  it.each(["codex", "grokbuild"] as const)(
    "does not claim %s can route a Gemini native endpoint",
    async (app) => {
      const result = await prepareProviderTransfer(
        fields({ sourceApp: "gemini", apiFormat: "gemini_native" }),
        app,
      );
      expect(result.provider.meta?.apiFormat).toBe("openai_responses");
      expect(result.warnings).toContain(
        i18n.t("provider.transfer.warnings.protocolUnsupported", {
          protocol: "gemini_native",
        }),
      );
      expect(result.warnings).not.toContain(
        i18n.t("provider.transfer.warnings.proxyRouting"),
      );
    },
  );

  it("keeps a supported non-native protocol for local routing on Codex", async () => {
    const result = await prepareProviderTransfer(fields(), "codex");
    expect(result.provider.meta?.apiFormat).toBe("anthropic");
    expect(result.warnings).toContain(
      i18n.t("provider.transfer.warnings.proxyRouting"),
    );
  });

  it("keeps ambiguous reverse mappings in Claude stack models with an explanation", async () => {
    const result = await prepareProviderTransfer(
      fields({
        sourceApp: "claude-desktop",
        models: [
          { id: "a", role: "opus" },
          { id: "b", role: "opus", name: "Second" },
        ],
      }),
      "claude",
    );
    expect(result.provider.meta?.stackModels).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ model: "b", displayName: "Second" }),
      ]),
    );
    expect(result.warnings.join(" ")).toContain("b (Second)");
  });

  it("maps explicit catalog context lengths to Claude 1m declarations", async () => {
    const { provider } = await prepareProviderTransfer(
      fields({
        sourceApp: "codex",
        models: [{ id: "large", contextWindow: 1000000 }],
      }),
      "claude",
    );
    expect(provider.settingsConfig.env.ANTHROPIC_MODEL).toBe("large[1m]");
  });

  it("reports a discarded Desktop alias even when its upstream model ID was kept", async () => {
    const result = await prepareProviderTransfer(
      fields({
        sourceApp: "claude-desktop",
        models: [
          { id: "same", role: "sonnet", name: "First" },
          { id: "same", role: "sonnet", name: "Second" },
        ],
      }),
      "claude-desktop",
    );
    expect(
      result.provider.meta?.claudeDesktopModelRoutes?.[
        CLAUDE_DESKTOP_ROLE_ROUTE_IDS.sonnet
      ].labelOverride,
    ).toBe("First");
    expect(result.warnings.join(" ")).toContain("Second");
  });

  it("merges repeated catalog IDs without losing context and reports conflicting labels", async () => {
    const result = await prepareProviderTransfer(
      fields({
        models: [
          { id: "same", name: "First", contextWindow: 1000000 },
          { id: "same", role: "opus", name: "Second" },
        ],
      }),
      "opencode",
    );
    expect(result.provider.settingsConfig.models.same).toMatchObject({
      name: "First",
      limit: { context: 1000000 },
    });
    expect(result.warnings.join()).toContain("Second");
  });

  it("uses Desktop defaults for an ordinary key card with no model", async () => {
    const result = await prepareProviderTransfer(
      fields({ models: [] }),
      "claude-desktop",
    );
    expect(
      Object.keys(result.provider.meta?.claudeDesktopModelRoutes ?? {}),
    ).toHaveLength(4);
    expect(result.warnings.join()).toContain("默认");
  });

  it("does not claim an unknown model has a 1M context", async () => {
    const result = await prepareProviderTransfer(fields(), "claude-desktop");
    expect(
      Object.values(result.provider.meta!.claudeDesktopModelRoutes!).every(
        (route) => !route.supports1m,
      ),
    ).toBe(true);
  });

  it("warns about incompatible target protocols and genuinely single-model targets", async () => {
    const result = await prepareProviderTransfer(
      fields({
        models: [
          { id: "a", primary: true },
          { id: "b", name: "Second" },
        ],
      }),
      "gemini",
    );
    expect(result.provider.settingsConfig.env.GEMINI_MODEL).toBe("a");
    expect(result.warnings.join()).toContain("不支持该协议");
    expect(result.warnings.join()).toContain("b (Second)");
  });

  it.each([
    null,
    { name: 123 },
    { models: "wrong" },
    { models: [{}] },
    { models: [{ id: "a", supports1m: "yes" }] },
    { requestOverrides: { headers: { bad: 123 } } },
    { baseUrl: "javascript:alert(1)" },
    { apiKey: "PROXY_MANAGED" },
  ])("rejects malformed or unsupported clipboard fields", async (invalid) => {
    const value = invalid === null ? null : { ...fields(), ...invalid };
    await expect(
      prepareProviderTransfer(value as ProviderTransferFields, "claude"),
    ).rejects.toThrow();
  });

  it("matches known target presets without confusing coding plans and pay-as-you-go", async () => {
    const result = await prepareProviderTransfer(
      fields({ baseUrl: "https://opencode.ai/zen/go", models: [] }),
      "codex",
    );
    const toml = parseToml(result.provider.settingsConfig.config);
    expect(toml.model_providers).toMatchObject({
      custom: { base_url: "https://opencode.ai/zen/go/v1" },
    });
    expect(toml.model).toBe("glm-5.3");
    expect(result.warnings.join()).toContain("同套餐");
    const back = await prepareProviderTransfer(
      extractProviderTransferFields(result.provider, "codex"),
      "claude",
    );
    expect(back.provider.settingsConfig.env).toMatchObject({
      ANTHROPIC_BASE_URL: "https://opencode.ai/zen/go",
      ANTHROPIC_API_KEY: KEY,
    });
    expect(
      back.provider.settingsConfig.env.ANTHROPIC_AUTH_TOKEN,
    ).toBeUndefined();
  });

  it.each(["pi", "omp"] as const)(
    "merges %s target model defaults before common-field overrides",
    async (app) => {
      const presets =
        app === "pi"
          ? (await import("@/config/piProviderPresets")).piProviderPresets
          : (await import("@/config/ompProviderPresets")).ompProviderPresets;
      const preset = presets.find((entry) => entry.name === "OpenCode Go")!;
      const defaults = preset.settingsConfig.models.find(
        (entry) => entry.id === "glm-5.3",
      )!;
      const before = JSON.stringify(preset);
      const result = await prepareProviderTransfer(
        fields({
          sourceApp: "claude",
          baseUrl: "https://opencode.ai/zen/go",
          models: [{ id: "glm-5.3", name: "My label", maxTokens: 16384 }],
        }),
        app,
      );
      expect(result.provider.settingsConfig.baseUrl).toBe(
        preset.settingsConfig.baseUrl,
      );
      expect(result.provider.settingsConfig.models).toEqual([
        {
          ...JSON.parse(JSON.stringify(defaults)),
          name: "My label",
          maxTokens: 16384,
        },
      ]);
      expect(
        result.provider.settingsConfig.models[0].contextWindow,
      ).toBeGreaterThan(0);
      expect(result.provider.settingsConfig.models[0].compat).toEqual(
        defaults.compat,
      );
      if (app === "omp")
        expect(result.provider.settingsConfig.models[0]).not.toHaveProperty(
          "thinkingLevelMap",
        );
      expect(result.warnings.join()).not.toContain("缺少");
      expect(JSON.stringify(preset)).toBe(before);
    },
  );

  it.each(["pi", "omp"] as const)(
    "keeps %s preset defaults when source models are unspecified",
    async (app) => {
      const presets =
        app === "pi"
          ? (await import("@/config/piProviderPresets")).piProviderPresets
          : (await import("@/config/ompProviderPresets")).ompProviderPresets;
      const preset = presets.find((entry) => entry.name === "OpenCode Go")!;
      const result = await prepareProviderTransfer(
        fields({ baseUrl: "https://opencode.ai/zen/go", models: [] }),
        app,
      );
      expect(result.provider.settingsConfig.models).toEqual(
        JSON.parse(JSON.stringify(preset.settingsConfig.models)),
      );
      expect(result.warnings.join()).not.toContain("缺少");
    },
  );

  it.each(["pi", "omp"] as const)(
    "names unknown %s models and only their missing required parameters",
    async (app) => {
      const result = await prepareProviderTransfer(
        fields({
          baseUrl: "https://opencode.ai/zen/go",
          models: [
            { id: "unknown-model" },
            { id: "context-only", contextWindow: 65536 },
            { id: "complete", contextWindow: 65536, maxTokens: 8192 },
          ],
        }),
        app,
      );
      const [unknown, partial, complete] =
        result.provider.settingsConfig.models;
      expect(unknown).not.toHaveProperty("contextWindow");
      expect(unknown).not.toHaveProperty("maxTokens");
      expect(unknown).not.toHaveProperty("compat");
      expect(partial).toMatchObject({ contextWindow: 65536 });
      expect(partial).not.toHaveProperty("maxTokens");
      expect(complete).toMatchObject({ contextWindow: 65536, maxTokens: 8192 });
      const unknownWarning = result.warnings.find((warning) =>
        warning.includes("unknown-model"),
      )!;
      expect(unknownWarning).toContain("contextWindow");
      expect(unknownWarning).toContain("maxTokens");
      const partialWarning = result.warnings.find((warning) =>
        warning.includes("context-only"),
      )!;
      expect(partialWarning).toContain("maxTokens");
      expect(partialWarning).not.toContain("contextWindow");
      expect(
        result.warnings.some((warning) => warning.includes("模型 complete")),
      ).toBe(false);
    },
  );

  it("keeps a custom prefix on a known hostname instead of guessing an endpoint", async () => {
    const result = await prepareProviderTransfer(
      fields({
        baseUrl: "https://opencode.ai/private/team/messages",
        isFullUrl: true,
      }),
      "codex",
    );
    expect(
      parseToml(result.provider.settingsConfig.config).model_providers,
    ).toMatchObject({
      custom: { base_url: "https://opencode.ai/private/team/messages" },
    });
    expect(result.provider.meta?.isFullUrl).toBe(true);
  });

  it("reads Grok Build preset carrier using the Codex preset structure", async () => {
    const result = await prepareProviderTransfer(
      fields({
        sourceApp: "codex",
        baseUrl: "https://opencode.ai/zen/go/v1",
        models: [],
      }),
      "grokbuild",
    );
    const parsed = parseGrokBuildConfig(result.provider.settingsConfig.config);
    expect(parsed.baseUrl).toBeTruthy();
    expect(parsed.apiKey).toBe(KEY);
    expect(parsed.upstreamModel).toBeTruthy();
  });

  it("strips untrusted extra fields before constructing a target draft", async () => {
    const untrusted = {
      ...fields(),
      settingsConfig: { hooks: "do-not-copy" },
      authBinding: { source: "managed_account" },
    };
    const { provider } = await prepareProviderTransfer(untrusted, "claude");
    expect(JSON.stringify(provider)).not.toContain("do-not-copy");
    expect(provider.meta?.authBinding).toBeUndefined();
  });

  it("uses the DSH catalog defaults, not the MCode catalog, for a known endpoint", async () => {
    const { dshProviderPresets } = await import("@/config/dshProviderPresets");
    const preset = dshProviderPresets.find(
      (entry) => entry.name === "OpenCode Go",
    )!;
    const defaults = preset.settingsConfig.models["glm-5.3"];
    const result = await prepareProviderTransfer(
      fields({
        baseUrl: "https://opencode.ai/zen/go",
        models: [{ id: "glm-5.3", name: "My label" }],
      }),
      "dsh",
    );
    const config = result.provider.settingsConfig;
    expect(config).toMatchObject({
      kind: "custom",
      api: preset.settingsConfig.api,
      options: { baseURL: preset.settingsConfig.options.baseURL, apiKey: KEY },
    });
    expect(config.models).toEqual({
      "glm-5.3": { ...JSON.parse(JSON.stringify(defaults)), name: "My label" },
    });
    expect(config).not.toHaveProperty("apiKeyEnv");
  });

  it("drops DSH preset compat flags when the copied protocol differs from the preset", async () => {
    const result = await prepareProviderTransfer(
      fields({
        sourceApp: "codex",
        apiFormat: "openai_responses",
        baseUrl: "https://opencode.ai/zen/go/v1",
        models: [{ id: "glm-5.3" }],
      }),
      "dsh",
    );
    const config = result.provider.settingsConfig;
    expect(config.api).toBe("openai-responses");
    expect(config).not.toHaveProperty("compat");
    expect(config.models["glm-5.3"]).not.toHaveProperty("compat");
    expect(config.models["glm-5.3"].limit.context).toBeGreaterThan(0);
  });

  it("does not offer DSH a protocol its adapter cannot speak", async () => {
    const result = await prepareProviderTransfer(
      fields({ sourceApp: "gemini", apiFormat: "gemini_native" }),
      "dsh",
    );
    expect(result.provider.settingsConfig.api).toBe("anthropic-messages");
    expect(result.warnings).toContain(
      i18n.t("provider.transfer.warnings.protocolUnsupported", {
        protocol: "gemini_native",
      }),
    );
  });

  it("refuses to turn a DSH environment key reference into a plaintext key", () => {
    expect(() =>
      extractProviderTransferFields(
        card({
          kind: "custom",
          api: "anthropic-messages",
          apiKeyEnv: "CC_SWITCH_DSH_GATEWAY",
          options: { baseURL: URL },
          models: { "model-a": {} },
        }),
        "dsh",
      ),
    ).toThrow(i18n.t("provider.transfer.errors.noPlainKey"));
  });
});

describe("provider transfer messages", () => {
  it.each(Object.keys(locales))(
    "localizes converter errors and warnings in %s",
    async (lng) => {
      await i18n.changeLanguage(lng);
      try {
        const transfer = (locales as Record<string, any>)[lng].provider
          .transfer;
        await expect(
          prepareProviderTransfer(
            fields({ apiKey: "PROXY_MANAGED" }),
            "claude",
          ),
        ).rejects.toThrow(transfer.errors.placeholderKey);
        const { warnings } = await prepareProviderTransfer(
          fields({ models: [] }),
          "claude-desktop",
        );
        expect(warnings).toEqual([
          transfer.warnings.desktopDefaults,
          transfer.warnings.desktopProxy,
        ]);
        expect(Object.keys(transfer.errors).sort()).toEqual(
          Object.keys(en.provider.transfer.errors).sort(),
        );
        expect(Object.keys(transfer.warnings).sort()).toEqual(
          Object.keys(en.provider.transfer.warnings).sort(),
        );
        if (lng === "en" || lng === "tr")
          expect(warnings.join()).not.toMatch(/\p{Script=Han}/u);
      } finally {
        await i18n.changeLanguage("zh");
      }
    },
  );

  it("has a translation for every message key the converter uses", () => {
    const source = readFileSync(
      new globalThis.URL(
        "../../src/utils/providerTransfer.ts",
        import.meta.url,
      ),
      "utf8",
    );
    const used = [
      ...source.matchAll(/(?:tr\("|fail\(")((?:errors|warnings)\.\w+|\w+)"/g),
    ].map(([, key]) => (key.includes(".") ? key : `errors.${key}`));
    expect(used.length).toBeGreaterThan(20);
    for (const resources of Object.values(locales))
      for (const key of used) {
        const [group, name] = key.split(".");
        expect(
          (resources.provider.transfer as Record<string, any>)[group]?.[name],
          key,
        ).toBeTruthy();
      }
  });
});
