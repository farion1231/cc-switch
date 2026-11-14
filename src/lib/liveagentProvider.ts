import { z } from "zod";

export const LIVEAGENT_APIS = [
  "openai-completions",
  "openai-responses",
  "anthropic-messages",
  "google-generative-ai",
  "google-gemini",
] as const;

/**
 * Protocols shown in the form, with readable labels (same wording as the Pi
 * form). K-brain treats `google-gemini` as an alias of `google-generative-ai`,
 * so it is only shown when a provider already uses it.
 */
export const LIVEAGENT_API_OPTIONS = [
  { value: "openai-completions", label: "OpenAI Chat Completions" },
  { value: "openai-responses", label: "OpenAI Responses" },
  { value: "anthropic-messages", label: "Anthropic Messages" },
  { value: "google-generative-ai", label: "Google Generative AI" },
] as const;

/** Provider `type` values for which K-brain derives `api` when it is empty. */
const TYPED_PROVIDERS = ["claude_code", "gemini", "xai", "codex", "deepseek"];

const str = z.string().nullish();
const bool = z.boolean().nullish();
const strings = z.array(z.string()).nullish();
const count = z.number().int().nonnegative().nullish();

// K-brain decodes config.json with DisallowUnknownFields: one stray key stops
// LiveAgent from loading its configuration. These mirror config.Provider and
// config.PiModel (K-brain 7db2a37) and the backend whitelist.
const modelSchema = z
  .object({
    id: z.string().trim().min(1),
    name: str,
    displayName: str,
    ownedBy: str,
    limitsSource: str,
    outputModalities: strings,
    imageAdapter: str,
    inputModalities: strings,
    api: z.enum(LIVEAGENT_APIS).or(z.literal("")).nullish(),
    reasoning: bool,
    input: strings,
    contextWindow: count,
    maxTokens: count,
    samplingParams: z
      .object({
        temperature: z.number().nullish(),
        top_p: z.number().nullish(),
      })
      .strict()
      .nullish(),
  })
  .strict();

export const liveagentProviderSchema = z
  .object({
    name: str,
    type: str,
    baseUrl: z.string().refine((s) => {
      if (!s) return true;
      try {
        return ["http:", "https:"].includes(new URL(s).protocol);
      } catch {
        return false;
      }
    }),
    isFullUrl: bool,
    modelsUrl: str,
    api: z.enum(LIVEAGENT_APIS).or(z.literal("")).optional(),
    apiKey: z.string(),
    customHeaders: z
      .array(z.object({ key: z.string(), value: z.string() }).strict())
      .nullish(),
    activeModels: strings,
    modelOrder: strings,
    requestFormat: str,
    reasoning: str,
    promptCacheHintMode: str,
    nativeWebSearchEnabled: bool,
    useSystemProxy: bool,
    metadata: z.record(z.string(), z.unknown()).nullish(),
    promptCachingEnabled: bool,
    promptCacheRetention: str,
    retryPolicy: z.record(z.string(), z.unknown()).nullish(),
    usageQuery: z.record(z.string(), z.unknown()).nullish(),
    cacheSessionAffinity: bool,
    cacheControlFormat: str,
    cacheCapabilities: z
      .object({
        supportsPromptCacheKey: bool,
        supportsLongRetention: bool,
        sessionAffinityFormat: str,
        responsesCacheOptions: bool,
      })
      .strict()
      .nullish(),
    models: z.array(modelSchema),
  })
  .strict()
  .superRefine((config, ctx) => {
    if (!config.api && !TYPED_PROVIDERS.includes(config.type ?? "")) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        path: ["api"],
        message: "Choose an API format",
      });
    }
    const ids = new Set(config.models.map((model) => model.id));
    if (
      ids.size !== config.models.length ||
      config.activeModels?.some((id) => !ids.has(id))
    ) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: "Model IDs must be unique and active models must exist",
      });
    }
  });

export type LiveAgentProviderConfig = z.infer<typeof liveagentProviderSchema>;
export type LiveAgentModel = LiveAgentProviderConfig["models"][number];

/** New providers default to Chat Completions; existing ones keep what they have. */
export function initialLiveAgentConfig(
  settingsConfig?: Record<string, unknown>,
): LiveAgentProviderConfig {
  if (settingsConfig) {
    return {
      baseUrl: "",
      apiKey: "",
      models: [],
      ...settingsConfig,
    } as LiveAgentProviderConfig;
  }
  return { api: "openai-completions", baseUrl: "", apiKey: "", models: [] };
}

export function addLiveAgentModel(
  config: LiveAgentProviderConfig,
): LiveAgentProviderConfig {
  // A blank id never enters activeModels; it is added once the id is typed.
  return { ...config, models: [...config.models, { id: "" }] };
}

export function changeLiveAgentModel(
  config: LiveAgentProviderConfig,
  index: number,
  patch: Partial<LiveAgentModel>,
): LiveAgentProviderConfig {
  const previous = config.models[index];
  const models = config.models.map((model, i) =>
    i === index ? { ...model, ...patch } : model,
  );
  if (!config.activeModels || patch.id === undefined) {
    return { ...config, models };
  }
  const nextId = patch.id;
  let active = config.activeModels.filter((id) => id !== "");
  if (previous.id === "") {
    // A newly added model starts enabled, like models without an explicit list.
    active = [...active, nextId];
  } else {
    active = active.map((id) => (id === previous.id ? nextId : id));
  }
  return {
    ...config,
    models,
    activeModels: Array.from(new Set(active.filter((id) => id !== ""))),
  };
}

export function removeLiveAgentModel(
  config: LiveAgentProviderConfig,
  index: number,
): LiveAgentProviderConfig {
  const removed = config.models[index].id;
  const models = config.models.filter((_, i) => i !== index);
  // Another model may still carry the same id while the user is editing.
  const stillUsed = models.some((model) => model.id === removed);
  return {
    ...config,
    models,
    ...(config.activeModels && !stillUsed
      ? { activeModels: config.activeModels.filter((id) => id !== removed) }
      : {}),
  };
}

/** Pick-list result: drop unchecked models, append newly checked ones once. */
export function applyFetchedLiveAgentModels(
  config: LiveAgentProviderConfig,
  { add, remove }: { add: string[]; remove: string[] },
): LiveAgentProviderConfig {
  const removed = new Set(remove);
  const kept = config.models.filter((model) => !removed.has(model.id));
  const present = new Set(kept.map((model) => model.id));
  const added = [...new Set(add)].filter((id) => id && !present.has(id));
  return {
    ...config,
    models: [...kept, ...added.map((id) => ({ id }))],
    ...(config.activeModels
      ? {
          activeModels: [
            ...config.activeModels.filter((id) => !removed.has(id)),
            ...added,
          ],
        }
      : {}),
  };
}

/** Enabled models of a stored provider config, for "set as default" choices. */
export function enabledLiveAgentModels(
  settingsConfig: unknown,
): { id: string; name?: string }[] {
  const config = (settingsConfig ?? {}) as {
    models?: unknown;
    activeModels?: unknown;
  };
  const active = Array.isArray(config.activeModels)
    ? (config.activeModels as unknown[])
    : null;
  return (Array.isArray(config.models) ? config.models : [])
    .filter(
      (model): model is { id: string; name?: string } =>
        typeof model?.id === "string" &&
        model.id.trim() !== "" &&
        (!active || active.includes(model.id)),
    )
    .map((model) => ({
      id: model.id,
      name: typeof model.name === "string" ? model.name : undefined,
    }));
}

export function liveAgentModelEnabled(
  config: LiveAgentProviderConfig,
  id: string,
): boolean {
  return !config.activeModels || config.activeModels.includes(id);
}

export function setLiveAgentModelEnabled(
  config: LiveAgentProviderConfig,
  id: string,
  enabled: boolean,
): LiveAgentProviderConfig {
  const active = config.activeModels ?? config.models.map((m) => m.id);
  return {
    ...config,
    activeModels: enabled
      ? Array.from(new Set([...active, id]))
      : active.filter((value) => value !== id),
  };
}

/** API format for fetching `/models`; typed providers derive theirs from `type`. */
export function liveAgentFetchApiFormat(
  config: LiveAgentProviderConfig,
): string | undefined {
  if (config.api) return config.api;
  if (config.type === "claude_code") return "anthropic-messages";
  if (config.type === "gemini") return "google-generative-ai";
  return undefined;
}
