import type { AppId } from "@/lib/api";
import type { Provider, ProviderMeta } from "@/types";
import { providersApi } from "@/lib/api/providers";
import { parse as parseToml, stringify as stringifyToml } from "smol-toml";
import { getCodexCustomTemplate } from "@/config/codexTemplates";
import { emptyKimiConfig } from "@/config/kimiProviderPresets";
import { CLAUDE_DESKTOP_ROLE_ROUTE_IDS } from "@/config/claudeDesktopProviderPresets";
import {
  CLAUDE_DEFAULT_CONFIG,
  CLAUDE_DESKTOP_DEFAULT_CONFIG,
  GEMINI_DEFAULT_CONFIG,
  OPENCODE_DEFAULT_CONFIG,
  OPENCLAW_DEFAULT_CONFIG,
} from "@/components/providers/forms/helpers/opencodeFormUtils";
import {
  extractCodexExperimentalBearerToken,
  extractProviderBaseUrl,
} from "./providerConfigUtils";
import { buildGrokBuildConfig, parseGrokBuildConfig } from "./grokBuildConfig";
import { deepClone } from "./deepClone";
import i18n from "i18next";
import { z } from "zod";

const tr = (key: string, options?: Record<string, unknown>): string =>
  i18n.t(`provider.transfer.${key}`, options);
function fail(key: string): never {
  throw new Error(tr(`errors.${key}`));
}

export interface ProviderTransferModel {
  id: string;
  name?: string;
  role?: "sonnet" | "opus" | "fable" | "haiku";
  primary?: boolean;
  supports1m?: boolean;
  contextWindow?: number;
  maxTokens?: number;
  sourceLabel?: string;
}

export interface ProviderTransferFields {
  sourceApp: AppId;
  name: string;
  websiteUrl?: string;
  notes?: string;
  icon?: string;
  iconColor?: string;
  baseUrl: string;
  apiKey: string;
  apiKeyField?: "ANTHROPIC_API_KEY" | "ANTHROPIC_AUTH_TOKEN";
  apiFormat?: ProviderMeta["apiFormat"];
  models: ProviderTransferModel[];
  isFullUrl?: boolean;
  customUserAgent?: string;
  requestOverrides?: ProviderMeta["localProxyRequestOverrides"];
  warnings?: string[];
}

const transferAppSchema = z.enum([
  "claude",
  "claude-desktop",
  "codex",
  "gemini",
  "agy",
  "grokbuild",
  "opencode",
  "openclaw",
  "hermes",
  "pi",
  "omp",
  "kimi",
  "mcode",
  "dsh",
]);
const transferText = z.string().max(8192);
const unresolvedKey = (key: string) =>
  key === "PROXY_MANAGED" ||
  /^(\$|!|\{env:|\{file:)/.test(key) ||
  key.includes("${");
const transferSchema = z.object({
  sourceApp: transferAppSchema,
  name: transferText.trim().min(1),
  websiteUrl: transferText.optional(),
  notes: z.string().max(65536).optional(),
  icon: z.string().max(1_000_000).optional(),
  iconColor: transferText.optional(),
  baseUrl: transferText,
  apiKey: transferText.min(1),
  apiKeyField: z.enum(["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"]).optional(),
  apiFormat: z
    .enum(["anthropic", "openai_chat", "openai_responses", "gemini_native"])
    .optional(),
  models: z
    .array(
      z.object({
        id: transferText.min(1).refine((value) => value.trim().length > 0),
        name: transferText.optional(),
        role: z.enum(["sonnet", "opus", "fable", "haiku"]).optional(),
        primary: z.boolean().optional(),
        supports1m: z.boolean().optional(),
        contextWindow: z.number().int().positive().optional(),
        maxTokens: z.number().int().positive().optional(),
        sourceLabel: transferText.optional(),
      }),
    )
    .max(1000),
  isFullUrl: z.boolean().optional(),
  customUserAgent: transferText.optional(),
  requestOverrides: z
    .object({
      headers: z.record(z.string(), z.string()).optional(),
      body: z.record(z.string(), z.unknown()).optional(),
    })
    .optional(),
  warnings: z.array(transferText).max(100).optional(),
});

function validateFields(
  fields: ProviderTransferFields,
): ProviderTransferFields {
  const parsed = transferSchema.safeParse(fields);
  if (!parsed.success) fail("invalidPayload");
  const value = parsed.data;
  if (value.baseUrl) {
    try {
      const url = new URL(value.baseUrl);
      if (
        !["http:", "https:"].includes(url.protocol) ||
        url.username ||
        url.password
      )
        throw new Error();
    } catch {
      fail("invalidUrl");
    }
  }
  if (unresolvedKey(value.apiKey)) fail("placeholderKey");
  return value;
}

type Format = NonNullable<ProviderMeta["apiFormat"]>;
type Role = NonNullable<ProviderTransferModel["role"]>;
const roles = Object.keys(CLAUDE_DESKTOP_ROLE_ROUTE_IDS) as Role[];
const record = (value: unknown): Record<string, any> =>
  value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, any>)
    : {};
const text = (value: unknown): string =>
  typeof value === "string" ? value.trim() : "";
const positive = (value: unknown): number | undefined =>
  typeof value === "number" && Number.isFinite(value) && value > 0
    ? value
    : undefined;
const roleKey = (role: Role) => `ANTHROPIC_DEFAULT_${role.toUpperCase()}_MODEL`;
const roleFromId = (id: string) =>
  roles.find((role) => id.toLowerCase().includes(role));

const protocolFormats: Record<string, Format> = {
  anthropic: "anthropic",
  "anthropic-messages": "anthropic",
  anthropic_messages: "anthropic",
  openai: "openai_chat",
  "openai-completions": "openai_chat",
  chat_completions: "openai_chat",
  chat: "openai_chat",
  "openai-responses": "openai_responses",
  openai_responses: "openai_responses",
  codex_responses: "openai_responses",
  responses: "openai_responses",
  "google-generative-ai": "gemini_native",
  "google-genai": "gemini_native",
  gemini_native: "gemini_native",
  kimi: "openai_chat",
};
const piProtocol: Record<Format, string> = {
  anthropic: "anthropic-messages",
  openai_chat: "openai-completions",
  openai_responses: "openai-responses",
  gemini_native: "google-generative-ai",
};
const npmFormat = (npm: string): Format | undefined => {
  if (npm.includes("anthropic")) return "anthropic";
  if (npm.includes("google")) return "gemini_native";
  if (npm === "@ai-sdk/openai") return "openai_responses";
  if (npm.includes("openai-compatible") || npm.includes("openrouter"))
    return "openai_chat";
  return undefined;
};

function model(
  id: unknown,
  extra: Partial<ProviderTransferModel> = {},
  claudeEnv = false,
): ProviderTransferModel | undefined {
  const raw = typeof id === "string" ? id : "";
  if (!raw.trim()) return undefined;
  const marked = claudeEnv && /\[1m\]$/i.test(raw.trim());
  return {
    id: claudeEnv
      ? raw
          .trim()
          .replace(/\[1m\]$/i, "")
          .trim()
      : raw,
    ...extra,
    ...(marked ? { supports1m: true } : {}),
  };
}

function readFields(
  provider: Provider,
  sourceApp: AppId,
): Pick<
  ProviderTransferFields,
  "baseUrl" | "apiKey" | "apiKeyField" | "apiFormat" | "models"
> {
  const config = record(provider.settingsConfig);
  const env = record(config.env);
  const meta = provider.meta;
  let baseUrl = extractProviderBaseUrl(config) ?? "";
  let apiKey = "";
  let apiKeyField: ProviderTransferFields["apiKeyField"];
  let apiFormat = meta?.apiFormat;
  const models: ProviderTransferModel[] = [];
  const add = (id: unknown, extra?: Partial<ProviderTransferModel>) => {
    const value = model(id, extra, sourceApp === "claude");
    if (value?.id) models.push(value);
  };
  switch (sourceApp) {
    case "claude":
    case "claude-desktop": {
      apiKeyField =
        meta?.apiKeyField === "ANTHROPIC_API_KEY" ||
        (!text(env.ANTHROPIC_AUTH_TOKEN) &&
          typeof env.ANTHROPIC_API_KEY === "string")
          ? "ANTHROPIC_API_KEY"
          : "ANTHROPIC_AUTH_TOKEN";
      apiKey = text(env[apiKeyField]);
      apiFormat ??= "anthropic";
      if (sourceApp === "claude-desktop") {
        for (const [route, mapping] of Object.entries(
          meta?.claudeDesktopModelRoutes ?? {},
        ).sort(([a], [b]) => a.localeCompare(b))) {
          add(meta?.claudeDesktopMode === "proxy" ? mapping.model : route, {
            name: mapping.labelOverride,
            role: roleFromId(route),
            supports1m: mapping.supports1m,
            sourceLabel: route,
          });
        }
      } else {
        add(env.ANTHROPIC_MODEL, {
          primary: true,
          name: text(env.ANTHROPIC_MODEL_NAME) || undefined,
        });
        for (const role of roles) {
          const key = roleKey(role);
          add(env[key], { role, name: text(env[`${key}_NAME`]) || undefined });
        }
        for (const entry of meta?.stackModels ?? []) {
          add(entry.model, {
            name: entry.displayName,
            supports1m: entry.oneM,
          });
        }
      }
      break;
    }
    case "codex": {
      const toml = text(config.config);
      const parsed = toml ? parseToml(toml) : {};
      const selected = record(
        record(parsed.model_providers)[text(parsed.model_provider)],
      );
      apiKey =
        text(record(config.auth).OPENAI_API_KEY) ||
        extractCodexExperimentalBearerToken(toml) ||
        "";
      apiFormat ??=
        protocolFormats[text(selected.wire_api)] ?? "openai_responses";
      const catalog = record(config.modelCatalog);
      for (const entry of Array.isArray(catalog.models) ? catalog.models : []) {
        const row = record(entry);
        add(row.model, {
          primary: text(row.model) === text(parsed.model),
          name: text(row.displayName ?? row.display_name) || undefined,
          contextWindow: positive(
            Number(row.contextWindow ?? row.context_window),
          ),
        });
      }
      if (!models.some((entry) => entry.primary))
        add(parsed.model, { primary: true });
      break;
    }
    case "gemini":
    case "agy":
      apiKey = text(env.GEMINI_API_KEY);
      apiFormat ??= "gemini_native";
      add(env.GEMINI_MODEL || record(record(config.config).model).name, {
        primary: true,
      });
      break;
    case "grokbuild": {
      const parsed = parseGrokBuildConfig(text(config.config));
      baseUrl = parsed.baseUrl;
      apiKey = parsed.apiKey;
      apiFormat ??= protocolFormats[parsed.apiBackend] ?? "openai_responses";
      add(parsed.upstreamModel || parsed.model, {
        primary: true,
        contextWindow: parsed.contextWindow,
      });
      break;
    }
    case "opencode":
    case "mcode":
    case "dsh": {
      const options = record(config.options ?? config.settings);
      baseUrl = text(options.baseURL) || baseUrl;
      apiKey = text(options.apiKey);
      apiFormat ??=
        protocolFormats[text(config.api)] ??
        npmFormat(text(config.npm ?? config.package));
      for (const [key, entry] of Object.entries(record(config.models))) {
        const row = record(entry);
        add(row.id || row.modelID || key, {
          name: text(row.name) || undefined,
          contextWindow: positive(record(row.limit).context),
          maxTokens: positive(record(row.limit).output),
        });
      }
      break;
    }
    case "openclaw":
    case "pi":
    case "omp":
    case "hermes": {
      apiKey = text(config.apiKey ?? config.api_key);
      apiFormat ??=
        protocolFormats[text(config.api ?? config.api_mode)] ?? "openai_chat";
      const entries = Array.isArray(config.models)
        ? config.models
        : Object.entries(record(config.models)).map(([id, entry]) => ({
            ...record(entry),
            id,
          }));
      for (const entry of entries) {
        const row = record(entry);
        add(row.id, {
          name: text(row.name) || undefined,
          contextWindow: positive(row.contextWindow ?? row.context_length),
          maxTokens: positive(row.maxTokens),
        });
      }
      break;
    }
    case "kimi": {
      const source = record(config.provider);
      apiKey = text(source.api_key);
      apiFormat ??= protocolFormats[text(source.type)];
      for (const entry of Object.values(record(config.models))) {
        const row = record(entry);
        add(row.model, {
          name: text(row.display_name) || undefined,
          contextWindow: positive(row.max_context_size),
        });
      }
      break;
    }
  }
  return { baseUrl, apiKey, apiKeyField, apiFormat, models };
}

export function extractProviderTransferFields(
  provider: Provider,
  sourceApp: AppId,
): ProviderTransferFields {
  const config = record(provider.settingsConfig);
  const env = record(config.env);
  const meta = provider.meta;
  if (
    meta?.authBinding?.source === "managed_account" ||
    meta?.githubAccountId ||
    ["github_copilot", "codex_oauth", "xai_oauth", "agy_oauth"].includes(
      meta?.providerType ?? "",
    ) ||
    record(config.provider).oauth ||
    provider.category === "omo" ||
    provider.category === "omo-slim"
  )
    fail("accountAuth");
  if (
    [
      "CLAUDE_CODE_USE_BEDROCK",
      "CLAUDE_CODE_USE_VERTEX",
      "CLAUDE_CODE_USE_FOUNDRY",
    ].some(
      (key) =>
        env[key] === true ||
        (text(env[key]) && !["0", "false"].includes(text(env[key]))),
    ) ||
    /bedrock|vertex/i.test(
      text(config.api ?? config.api_mode ?? record(config.provider).type),
    )
  )
    fail("cloudAuth");
  if (["pi", "omp", "openclaw"].includes(sourceApp)) {
    const entries = Array.isArray(config.models)
      ? config.models
      : Object.values(record(config.models));
    if (
      entries.some((entry) => {
        const row = record(entry);
        return (
          (text(row.api) && text(row.api) !== text(config.api)) ||
          (text(row.baseUrl) && text(row.baseUrl) !== text(config.baseUrl))
        );
      })
    )
      fail("modelConnectionOverride");
  }
  let common: ReturnType<typeof readFields>;
  try {
    common = readFields(provider, sourceApp);
  } catch {
    fail("unparsable");
  }
  if (!common.apiKey || unresolvedKey(common.apiKey)) fail("noPlainKey");
  if (!common.baseUrl) {
    common.baseUrl =
      sourceApp === "claude" || sourceApp === "claude-desktop"
        ? "https://api.anthropic.com"
        : sourceApp === "gemini" || sourceApp === "agy"
          ? "https://generativelanguage.googleapis.com"
          : sourceApp === "codex"
            ? "https://api.openai.com/v1"
            : "";
  }
  if (
    /githubcopilot\.com|chatgpt\.com\/backend-api\/codex/i.test(common.baseUrl)
  )
    fail("managedEndpoint");
  const headers =
    record(config.options).headers ??
    config.headers ??
    record(config.provider).custom_headers;
  const overrides =
    meta?.localProxyRequestOverrides ?? (headers ? { headers } : undefined);
  return {
    sourceApp,
    name: provider.name,
    websiteUrl: provider.websiteUrl,
    notes: provider.notes,
    icon: provider.icon,
    iconColor: provider.iconColor,
    ...common,
    isFullUrl: meta?.isFullUrl,
    customUserAgent: meta?.customUserAgent,
    requestOverrides: overrides ? deepClone(overrides) : undefined,
  };
}

interface TransferPreset {
  name: string;
  websiteUrl?: string;
  family?: string;
  planKey?: string;
  regionKey?: string;
  category?: string;
  settingsConfig?: Record<string, any>;
  baseUrl?: string;
  apiFormat?: Format;
  apiKeyField?: ProviderTransferFields["apiKeyField"];
  providerType?: string;
  auth?: Record<string, any>;
  config?: string;
  modelCatalog?: unknown[];
}

async function presetsFor(app: AppId): Promise<TransferPreset[]> {
  switch (app) {
    case "claude":
      return (await import("@/config/claudeProviderPresets")).providerPresets;
    case "claude-desktop":
      return (await import("@/config/claudeDesktopProviderPresets"))
        .claudeDesktopProviderPresets;
    case "codex":
      return (await import("@/config/codexProviderPresets"))
        .codexProviderPresets;
    case "gemini":
    case "agy":
      return (await import("@/config/geminiProviderPresets"))
        .geminiProviderPresets;
    case "grokbuild":
      return (await import("@/config/grokBuildProviderPresets"))
        .grokBuildProviderPresets;
    case "opencode":
      return (await import("@/config/opencodeProviderPresets"))
        .opencodeProviderPresets;
    case "openclaw":
      return (await import("@/config/openclawProviderPresets"))
        .openclawProviderPresets;
    case "hermes":
      return (await import("@/config/hermesProviderPresets"))
        .hermesProviderPresets;
    case "pi":
      return (await import("@/config/piProviderPresets")).piProviderPresets;
    case "omp":
      return (await import("@/config/ompProviderPresets")).ompProviderPresets;
    case "mcode":
      return (await import("@/config/mcodeProviderPresets"))
        .mcodeProviderPresets;
    case "dsh":
      return (await import("@/config/dshProviderPresets")).dshProviderPresets;
    case "kimi":
      return (await import("@/config/kimiProviderPresets")).kimiProviderPresets;
  }
}

const normalizedUrl = (url: string) => url.trim().replace(/\/+$/, "");
function presetConfig(preset: TransferPreset, app: AppId): Record<string, any> {
  if (preset.settingsConfig) return preset.settingsConfig;
  if (preset.config !== undefined) {
    if (app === "grokbuild") {
      const fields = readFields(
        {
          id: "",
          name: preset.name,
          settingsConfig: { auth: preset.auth, config: preset.config },
        },
        "codex",
      );
      return {
        config: buildGrokBuildConfig({
          ...parseGrokBuildConfig(undefined),
          name: preset.name,
          baseUrl: fields.baseUrl,
          apiKey: fields.apiKey,
          ...(fields.models[0] ? { upstreamModel: fields.models[0].id } : {}),
        }),
      };
    }
    return {
      auth: preset.auth ?? {},
      config: preset.config,
      ...(preset.modelCatalog
        ? { modelCatalog: { models: preset.modelCatalog } }
        : {}),
    };
  }
  return { env: { ANTHROPIC_BASE_URL: preset.baseUrl } };
}

function presetConnection(preset: TransferPreset, app: AppId) {
  try {
    return readFields(
      {
        id: "",
        name: preset.name,
        settingsConfig: presetConfig(preset, app),
        meta: { apiFormat: preset.apiFormat, apiKeyField: preset.apiKeyField },
      },
      app,
    );
  } catch {
    return { baseUrl: "", apiKey: "", models: [] };
  }
}

async function matchingTargetPreset(
  fields: ProviderTransferFields,
  target: AppId,
) {
  if (!fields.baseUrl || fields.isFullUrl) return undefined;
  const [sourcePresets, targetPresets] = await Promise.all([
    presetsFor(fields.sourceApp),
    presetsFor(target),
  ]);
  const sources = sourcePresets.filter(
    (preset) =>
      normalizedUrl(presetConnection(preset, fields.sourceApp).baseUrl) ===
      normalizedUrl(fields.baseUrl),
  );
  // Hostname alone is not identity: coding plans, regions and custom URL prefixes can use different keys.
  const matches = targetPresets.filter(
    (candidate) =>
      !candidate.providerType &&
      candidate.category !== "official" &&
      sources.some((source) =>
        source.family && candidate.family
          ? candidate.family === source.family &&
            candidate.planKey === source.planKey &&
            candidate.regionKey === source.regionKey
          : candidate.name === source.name &&
            candidate.websiteUrl === source.websiteUrl,
      ),
  );
  return matches.length === 1 ? matches[0] : undefined;
}

function defaultConfig(app: AppId): Record<string, any> {
  switch (app) {
    case "claude":
      return JSON.parse(CLAUDE_DEFAULT_CONFIG);
    case "claude-desktop":
      return JSON.parse(CLAUDE_DESKTOP_DEFAULT_CONFIG);
    case "codex":
      return getCodexCustomTemplate();
    case "gemini":
    case "agy":
      return JSON.parse(GEMINI_DEFAULT_CONFIG);
    case "opencode":
      return JSON.parse(OPENCODE_DEFAULT_CONFIG);
    case "openclaw":
      return JSON.parse(OPENCLAW_DEFAULT_CONFIG);
    case "hermes":
      return { name: "", base_url: "", api_key: "" };
    case "pi":
    case "omp":
      return { baseUrl: "", apiKey: "", api: "openai-completions", models: [] };
    case "mcode":
    case "dsh":
      return {
        kind: "custom",
        enabled: true,
        api: "anthropic-messages",
        options: {},
        models: {},
      };
    case "kimi":
      return emptyKimiConfig();
    case "grokbuild":
      return { config: buildGrokBuildConfig(parseGrokBuildConfig(undefined)) };
  }
}

const describeModel = (entry: ProviderTransferModel) =>
  `${entry.sourceLabel ? `${entry.sourceLabel} → ` : ""}${entry.id}${entry.name && entry.name !== entry.id ? ` (${entry.name})` : ""}${entry.supports1m ? " [1m]" : ""}`;
const describeModels = (entries: ProviderTransferModel[]) =>
  entries.map(describeModel).join("; ");
// Formats each target's own form can select; anything else would be silently replaced on save.
const targetFormats: Partial<Record<AppId, Format[]>> = {
  claude: ["anthropic", "openai_chat", "openai_responses", "gemini_native"],
  "claude-desktop": [
    "anthropic",
    "openai_chat",
    "openai_responses",
    "gemini_native",
  ],
  codex: ["openai_responses", "openai_chat", "anthropic"],
  grokbuild: ["openai_responses", "openai_chat", "anthropic"],
};
// Only these forms expose full-URL mode, a User-Agent and request overrides.
const advancedProxyTargets: AppId[] = ["claude", "codex", "grokbuild"];
const cliModel = (entry: ProviderTransferModel) =>
  `${entry.id}${(entry.supports1m ?? (entry.contextWindow ?? 0) >= 1_000_000) ? "[1m]" : ""}`;

export async function prepareProviderTransfer(
  fields: ProviderTransferFields,
  targetApp: AppId,
): Promise<{ provider: Provider; warnings: string[] }> {
  fields = validateFields(fields);
  if (!transferAppSchema.safeParse(targetApp).success)
    fail("unsupportedTarget");
  const warnings = [...(fields.warnings ?? [])];
  const targetPreset = await matchingTargetPreset(fields, targetApp);
  const presetFields = targetPreset
    ? presetConnection(targetPreset, targetApp)
    : undefined;
  const config = deepClone(
    targetPreset
      ? presetConfig(targetPreset, targetApp)
      : defaultConfig(targetApp),
  );
  const baseUrl = presetFields?.baseUrl || fields.baseUrl;
  const format =
    normalizedUrl(baseUrl) === normalizedUrl(fields.baseUrl)
      ? (fields.apiFormat ?? presetFields?.apiFormat)
      : (presetFields?.apiFormat ?? fields.apiFormat);
  const meta: ProviderMeta = {};
  if (baseUrl !== fields.baseUrl)
    warnings.push(tr("warnings.presetEndpoint", { url: baseUrl }));
  const sourceModels = fields.models.filter((entry) => entry.id.trim());
  const models = sourceModels.length
    ? sourceModels
    : (presetFields?.models ?? []);
  const primary =
    models.find((entry) => entry.primary) ??
    models.find((entry) => entry.role === "sonnet") ??
    models[0];
  const modelMap = new Map<string, ProviderTransferModel>();
  for (const entry of models) {
    const existing = modelMap.get(entry.id);
    modelMap.set(
      entry.id,
      existing
        ? {
            ...entry,
            ...existing,
            name: existing.name ?? entry.name,
            supports1m: existing.supports1m ?? entry.supports1m,
            contextWindow: existing.contextWindow ?? entry.contextWindow,
            maxTokens: existing.maxTokens ?? entry.maxTokens,
          }
        : entry,
    );
    if (
      existing?.name &&
      entry.name &&
      existing.name !== entry.name &&
      targetApp !== "claude" &&
      targetApp !== "claude-desktop"
    ) {
      warnings.push(
        tr("warnings.duplicateName", {
          id: entry.id,
          kept: existing.name,
          dropped: entry.name,
        }),
      );
    }
  }
  const uniqueModels = [...modelMap.values()];
  const keyField =
    targetPreset?.apiKeyField ??
    fields.apiKeyField ??
    presetFields?.apiKeyField ??
    "ANTHROPIC_AUTH_TOKEN";
  const proxyTarget = [
    "claude",
    "claude-desktop",
    "codex",
    "grokbuild",
  ].includes(targetApp);
  const singleModelNotice = () => {
    const omitted = models.filter(
      (entry) =>
        entry !== primary &&
        (entry.id !== primary?.id || entry.name !== primary?.name),
    );
    if (omitted.length)
      warnings.push(
        tr("warnings.singleModel", {
          model: primary?.id,
          models: describeModels(omitted),
        }),
      );
  };
  const nativeProtocol = (fallback: Format, supported: Format[]) => {
    if (format && !supported.includes(format)) {
      warnings.push(tr("warnings.protocolUnsupported", { protocol: format }));
      return fallback;
    }
    if (!format) warnings.push(tr("warnings.protocolUnknown"));
    return format ?? fallback;
  };
  const fullUrlNotice = () => {
    if (fields.isFullUrl) warnings.push(tr("warnings.fullUrl"));
  };
  const applyHeaders = (destination: Record<string, any>, key = "headers") => {
    const headers = { ...fields.requestOverrides?.headers };
    if (fields.customUserAgent) headers["User-Agent"] = fields.customUserAgent;
    if (Object.keys(headers).length) destination[key] = headers;
    if (
      fields.requestOverrides?.body &&
      Object.keys(fields.requestOverrides.body).length
    ) {
      warnings.push(tr("warnings.requestBodyDropped"));
    }
    fullUrlNotice();
  };
  if (proxyTarget) {
    const supported = targetFormats[targetApp] ?? [];
    const fallback: Format =
      targetApp === "claude" || targetApp === "claude-desktop"
        ? "anthropic"
        : "openai_responses";
    meta.apiFormat = nativeProtocol(fallback, supported);
    meta.apiKeyField = keyField;
    if (advancedProxyTargets.includes(targetApp)) {
      meta.isFullUrl = fields.isFullUrl;
      meta.customUserAgent = fields.customUserAgent;
      meta.localProxyRequestOverrides = fields.requestOverrides
        ? deepClone(fields.requestOverrides)
        : undefined;
      if (
        meta.apiFormat !== fallback ||
        fields.isFullUrl ||
        fields.requestOverrides ||
        fields.customUserAgent
      )
        warnings.push(tr("warnings.proxyRouting"));
    } else {
      fullUrlNotice();
      if (fields.requestOverrides || fields.customUserAgent)
        warnings.push(tr("warnings.advancedDropped"));
    }
  }

  switch (targetApp) {
    case "claude": {
      const env = record(config.env);
      config.env = env;
      delete env.ANTHROPIC_AUTH_TOKEN;
      delete env.ANTHROPIC_API_KEY;
      env.ANTHROPIC_BASE_URL = baseUrl;
      env[keyField] = fields.apiKey;
      if (primary) {
        env.ANTHROPIC_MODEL = cliModel(primary);
        if (primary.name) env.ANTHROPIC_MODEL_NAME = primary.name;
      }
      const mapped = new Set<ProviderTransferModel>();
      if (primary) mapped.add(primary);
      for (const role of roles) {
        const entry =
          models.find((candidate) => candidate.role === role) ?? primary;
        if (!entry) continue;
        mapped.add(entry);
        env[roleKey(role)] = cliModel(entry);
        if (entry.name) env[`${roleKey(role)}_NAME`] = entry.name;
      }
      const extra = models.filter((entry) => !mapped.has(entry));
      if (extra.length) {
        meta.stackModels = uniqueModels.map((entry) => ({
          model: entry.id,
          displayName: entry.name,
          oneM: entry.supports1m,
        }));
        warnings.push(
          tr("warnings.claudeStack", { models: describeModels(extra) }),
        );
      }
      break;
    }
    case "claude-desktop": {
      const defaults = await providersApi.getClaudeDesktopDefaultRoutes();
      config.env = { ANTHROPIC_BASE_URL: baseUrl, [keyField]: fields.apiKey };
      meta.claudeDesktopMode = "proxy";
      // The Desktop form keeps exactly one proxy route per role, so extra routes would vanish on save.
      const slots = new Map<Role, ProviderTransferModel>();
      const placed = new Set<string>();
      const place = (role: Role, entry: ProviderTransferModel) => {
        slots.set(role, entry);
        placed.add(entry.id);
      };
      for (const role of roles) {
        const entry = models.find((candidate) => candidate.role === role);
        if (entry) place(role, entry);
      }
      // Fill free roles with distinct unassigned models, primary first, so every slot stays selectable in Desktop.
      const spare = [primary, ...models].filter(
        (entry): entry is ProviderTransferModel => !!entry && !entry.role,
      );
      for (const role of roles) {
        const entry = spare.find((candidate) => !placed.has(candidate.id));
        if (!slots.has(role) && entry) place(role, entry);
      }
      for (const role of roles)
        if (!slots.has(role) && primary) place(role, primary);
      meta.claudeDesktopModelRoutes = Object.fromEntries(
        roles.map((role) => {
          const routeId = CLAUDE_DESKTOP_ROLE_ROUTE_IDS[role];
          const entry = slots.get(role);
          const spec = defaults.find(
            (item) => roleFromId(item.routeId) === role,
          );
          return [
            routeId,
            {
              model: entry?.id ?? spec?.routeId ?? routeId,
              labelOverride: entry ? (entry.name ?? entry.id) : undefined,
              // Explicit source declarations win. Unknown upstream models must not acquire a fictitious 1M window.
              supports1m: entry
                ? (entry.supports1m ?? (entry.contextWindow ?? 0) >= 1_000_000)
                : (spec?.supports1m ?? false),
            },
          ];
        }),
      );
      const selected = [...slots.entries()];
      const omitted = models.filter(
        (entry) =>
          !selected.some(
            ([role, chosen]) =>
              chosen.id === entry.id &&
              (!entry.role || role === entry.role) &&
              (!entry.name || (chosen.name ?? chosen.id) === entry.name),
          ),
      );
      if (omitted.length)
        warnings.push(
          tr("warnings.desktopUnmapped", { models: describeModels(omitted) }),
        );
      if (!models.length) warnings.push(tr("warnings.desktopDefaults"));
      warnings.push(tr("warnings.desktopProxy"));
      break;
    }
    case "codex": {
      const template = getCodexCustomTemplate();
      const toml = parseToml(text(config.config) || template.config) as Record<
        string,
        any
      >;
      // A new custom identity must not inherit the built-in OpenAI or a managed-account route.
      const original = record(
        record(toml.model_providers)[text(toml.model_provider)],
      );
      toml.model_provider = "custom";
      toml.model_providers = {
        custom: {
          ...original,
          name: fields.name,
          base_url: baseUrl,
          wire_api: "responses",
          requires_openai_auth: true,
        },
      };
      delete toml.model_providers.custom.env_key;
      delete toml.model_providers.custom.experimental_bearer_token;
      if (primary) toml.model = primary.id;
      config.auth = { OPENAI_API_KEY: fields.apiKey };
      config.config = stringifyToml(toml);
      if (models.length)
        config.modelCatalog = {
          models: uniqueModels.map((entry) => ({
            model: entry.id,
            displayName: entry.name ?? entry.id,
            ...(entry.contextWindow || entry.supports1m
              ? { contextWindow: entry.contextWindow ?? 1_000_000 }
              : {}),
          })),
        };
      break;
    }
    case "gemini":
    case "agy": {
      nativeProtocol("gemini_native", ["gemini_native"]);
      config.env = {
        ...record(config.env),
        GOOGLE_GEMINI_BASE_URL: baseUrl,
        GEMINI_API_KEY: fields.apiKey,
      };
      if (primary) config.env.GEMINI_MODEL = primary.id;
      if (targetApp === "agy" && primary)
        config.config = { model: { name: primary.id } };
      singleModelNotice();
      fullUrlNotice();
      if (fields.requestOverrides || fields.customUserAgent)
        warnings.push(tr("warnings.advancedDropped"));
      break;
    }
    case "grokbuild": {
      config.config = buildGrokBuildConfig({
        ...parseGrokBuildConfig(text(config.config)),
        name: fields.name,
        baseUrl,
        apiKey: fields.apiKey,
        ...(primary
          ? {
              model: primary.id,
              upstreamModel: primary.id,
              ...(primary.contextWindow
                ? { contextWindow: primary.contextWindow }
                : {}),
            }
          : {}),
      });
      singleModelNotice();
      break;
    }
    case "opencode":
    case "mcode":
    case "dsh": {
      const piFamily = targetApp !== "opencode";
      const supported: Format[] = piFamily
        ? ["anthropic", "openai_chat", "openai_responses"]
        : ["anthropic", "openai_chat", "openai_responses", "gemini_native"];
      const protocol = nativeProtocol(
        piFamily ? "anthropic" : "openai_chat",
        supported,
      );
      // Preset compat flags are protocol-specific; keep them only while the preset's protocol is kept.
      const keepCompat = protocol === presetFields?.apiFormat;
      const targetModels = record(config.models);
      if (!keepCompat) delete config.compat;
      if (targetApp === "opencode")
        config.npm = {
          anthropic: "@ai-sdk/anthropic",
          openai_chat: "@ai-sdk/openai-compatible",
          openai_responses: "@ai-sdk/openai",
          gemini_native: "@ai-sdk/google",
        }[protocol];
      else config.api = piProtocol[protocol];
      config.options = {
        ...record(config.options),
        baseURL: baseUrl,
        apiKey: fields.apiKey,
      };
      applyHeaders(config.options);
      if (models.length)
        config.models = Object.fromEntries(
          uniqueModels.map((entry) => {
            const { compat, ...defaults } = record(targetModels[entry.id]);
            const limit = {
              ...record(defaults.limit),
              ...(entry.contextWindow || entry.supports1m
                ? { context: entry.contextWindow ?? 1_000_000 }
                : {}),
              ...(entry.maxTokens ? { output: entry.maxTokens } : {}),
            };
            return [
              entry.id,
              {
                ...defaults,
                ...(keepCompat && compat ? { compat } : {}),
                name: entry.name ?? defaults.name ?? entry.id,
                ...(Object.keys(limit).length ? { limit } : {}),
              },
            ];
          }),
        );
      break;
    }
    case "openclaw":
    case "pi":
    case "omp": {
      const protocol = nativeProtocol("openai_chat", [
        "anthropic",
        "openai_chat",
        "openai_responses",
        "gemini_native",
      ]);
      config.baseUrl = baseUrl;
      config.apiKey = fields.apiKey;
      config.api = piProtocol[protocol];
      if (targetApp === "pi") config.name = fields.name;
      applyHeaders(config);
      const keepCompat = protocol === presetFields?.apiFormat;
      if (!keepCompat) delete config.compat;
      if (models.length) {
        const targetModels = new Map<string, Record<string, any>>(
          (Array.isArray(config.models) ? config.models : []).map((entry) => [
            text(record(entry).id),
            record(entry),
          ]),
        );
        config.models = uniqueModels.map((entry) => {
          const { compat, ...defaults } = targetModels.get(entry.id) ?? {};
          return {
            ...defaults,
            ...(keepCompat && compat ? { compat } : {}),
            id: entry.id,
            name: entry.name ?? defaults.name ?? entry.id,
            ...(entry.contextWindow || entry.supports1m
              ? { contextWindow: entry.contextWindow ?? 1_000_000 }
              : {}),
            ...(entry.maxTokens ? { maxTokens: entry.maxTokens } : {}),
          };
        });
        if (targetApp === "pi" || targetApp === "omp") {
          for (const entry of config.models) {
            const missing = [
              !positive(entry.contextWindow) && "contextWindow",
              !positive(entry.maxTokens) && "maxTokens",
            ].filter(Boolean);
            if (missing.length)
              warnings.push(
                tr("warnings.missingParams", {
                  model: entry.id,
                  fields: missing.join(", "),
                }),
              );
          }
        }
      }
      break;
    }
    case "hermes": {
      const protocol = nativeProtocol("openai_chat", [
        "anthropic",
        "openai_chat",
        "openai_responses",
      ]);
      config.name = fields.name;
      config.base_url = baseUrl;
      config.api_key = fields.apiKey;
      config.api_mode = {
        anthropic: "anthropic_messages",
        openai_chat: "chat_completions",
        openai_responses: "codex_responses",
        gemini_native: "chat_completions",
      }[protocol];
      if (models.length)
        config.models = uniqueModels.map((entry) => ({
          id: entry.id,
          name: entry.name,
          ...(entry.contextWindow || entry.supports1m
            ? { context_length: entry.contextWindow ?? 1_000_000 }
            : {}),
        }));
      fullUrlNotice();
      if (fields.requestOverrides || fields.customUserAgent)
        warnings.push(tr("warnings.advancedDropped"));
      break;
    }
    case "kimi": {
      const protocol = nativeProtocol("openai_chat", [
        "anthropic",
        "openai_chat",
        "openai_responses",
        "gemini_native",
      ]);
      config.provider = {
        type: {
          anthropic: "anthropic",
          openai_chat: "openai",
          openai_responses: "openai_responses",
          gemini_native: "google-genai",
        }[protocol],
        base_url: baseUrl,
        api_key: fields.apiKey,
      };
      applyHeaders(config.provider, "custom_headers");
      const targetModels = new Map(
        Object.values(record(config.models)).map((value) => {
          const row = record(value);
          return [row.model, row] as const;
        }),
      );
      let usedPlaceholder = false;
      if (models.length)
        config.models = Object.fromEntries(
          uniqueModels.map((entry) => {
            const defaults = targetModels.get(entry.id) ?? {};
            const context =
              entry.contextWindow ??
              (entry.supports1m
                ? 1_000_000
                : positive(defaults.max_context_size));
            if (!context) usedPlaceholder = true;
            const { provider: _provider, ...modelDefaults } = defaults;
            return [
              entry.id,
              {
                ...modelDefaults,
                model: entry.id,
                display_name: entry.name ?? defaults.display_name,
                max_context_size: context ?? 262144,
              },
            ];
          }),
        );
      if (usedPlaceholder)
        warnings.push(tr("warnings.kimiContext", { size: 262144 }));
      break;
    }
  }
  if (
    !models.length &&
    ![
      "claude",
      "claude-desktop",
      "codex",
      "gemini",
      "agy",
      "grokbuild",
    ].includes(targetApp)
  ) {
    warnings.push(tr("warnings.noModels"));
  }
  return {
    provider: {
      id: "",
      name: fields.name,
      websiteUrl: fields.websiteUrl,
      notes: fields.notes,
      icon: fields.icon,
      iconColor: fields.iconColor,
      category: "custom",
      settingsConfig: config,
      ...(Object.keys(meta).length ? { meta } : {}),
    },
    warnings,
  };
}
