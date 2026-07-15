import { piVendorPresets } from "@/config/piProviderPresets";
import type {
  PiApiKeyDraft,
  PiHeaderDraft,
  PiModelCost,
  PiModelDraft,
  PiProviderCompat,
  PiProviderDraft,
  PiProviderMode,
  PiProviderTemplate,
} from "@/types/pi";

/** Provider keys fully owned by the draft form. Everything else on an existing
 *  provider object is preserved by the backend's deep-merge upsert, and we
 *  collect it back into `advancedJson` here so edits round-trip without loss. */
const MANAGED_KEYS = new Set([
  "baseUrl",
  "api",
  "apiKey",
  "headers",
  "models",
  "compat",
]);

function parseApiKey(raw: unknown): PiApiKeyDraft {
  if (typeof raw !== "string" || raw === "") return { mode: "none", value: "" };
  if (raw.startsWith("$")) return { mode: "env", value: raw.slice(1) };
  if (raw.startsWith("!")) return { mode: "command", value: raw };
  return { mode: "literal", value: raw };
}

function parseCost(raw: unknown): PiModelCost | undefined {
  if (typeof raw !== "object" || raw === null) return undefined;
  const c = raw as Record<string, unknown>;
  const num = (k: string): number | undefined =>
    typeof c[k] === "number" ? (c[k] as number) : undefined;
  const input = num("input");
  const output = num("output");
  if (input === undefined || output === undefined) return undefined;
  return {
    input,
    output,
    cacheRead: num("cacheRead"),
    cacheWrite: num("cacheWrite"),
  };
}

function inferTemplate(api: string): PiProviderTemplate {
  switch (api) {
    case "anthropic-messages":
      return "anthropicCompatible";
    case "google-generative-ai":
      return "googleGenerativeAi";
    case "openai-responses":
      return "openAiResponses";
    default:
      return "openAiCompatible";
  }
}

export interface ProviderToDraftSeed {
  providerId: string;
  /** `mode`/`template` are NOT stored inside a per-provider JSON object, so
   *  when parsing the JSON editor's content the caller passes the current
   *  draft's values. When loading an existing provider, `mode` is inferred
   *  from the vendor preset list. */
  mode?: PiProviderMode;
  template?: PiProviderTemplate;
}

/**
 * Faithfully convert a `models.json` provider object back into a draft,
 * preserving every modeled field (cost, all 9 compat flags) and collecting any
 * non-managed keys into `advancedJson`. This is the single source of truth used
 * by "edit an existing provider" (and any JSON round-trip), so editing a
 * provider never silently drops data.
 */
export function providerToDraft(
  provider: Record<string, unknown>,
  seed: ProviderToDraftSeed,
): PiProviderDraft {
  // — models (including cost) —
  const models: PiModelDraft[] = [];
  if (Array.isArray(provider.models)) {
    for (const m of provider.models) {
      if (typeof m !== "object" || m === null) continue;
      const mo = m as Record<string, unknown>;
      models.push({
        id: String(mo.id ?? ""),
        name: typeof mo.name === "string" ? mo.name : null,
        nameTouched: typeof mo.name === "string",
        reasoning: typeof mo.reasoning === "boolean" ? mo.reasoning : undefined,
        input: Array.isArray(mo.input)
          ? (mo.input as unknown[]).filter(
              (x): x is string => typeof x === "string",
            )
          : undefined,
        contextWindow:
          typeof mo.contextWindow === "number" ? mo.contextWindow : undefined,
        maxTokens: typeof mo.maxTokens === "number" ? mo.maxTokens : undefined,
        cost: parseCost(mo.cost),
      });
    }
  }
  if (models.length === 0) {
    models.push({ id: "", name: "", nameTouched: false });
  }

  // — headers —
  const headers: PiHeaderDraft[] = [];
  const h = provider.headers;
  if (typeof h === "object" && h !== null && !Array.isArray(h)) {
    for (const [k, v] of Object.entries(h as Record<string, unknown>)) {
      headers.push({ key: k, value: String(v ?? "") });
    }
  }

  // — compat: all 9 flags, preserving boolean|undefined (NOT Boolean()) so an
  //   absent flag stays absent rather than being coerced to false. —
  let compat: PiProviderCompat | null = null;
  const c = provider.compat;
  if (typeof c === "object" && c !== null && !Array.isArray(c)) {
    const co = c as Record<string, unknown>;
    const b = (k: string): boolean | undefined =>
      typeof co[k] === "boolean" ? (co[k] as boolean) : undefined;
    const s = (k: string): string | undefined =>
      typeof co[k] === "string" ? (co[k] as string) : undefined;
    compat = {
      supportsDeveloperRole: b("supportsDeveloperRole"),
      supportsReasoningEffort: b("supportsReasoningEffort"),
      supportsUsageInStreaming: b("supportsUsageInStreaming"),
      maxTokensField: s("maxTokensField") as PiProviderCompat["maxTokensField"],
      thinkingFormat: s("thinkingFormat"),
      supportsEagerToolInputStreaming: b("supportsEagerToolInputStreaming"),
      supportsLongCacheRetention: b("supportsLongCacheRetention"),
      forceAdaptiveThinking: b("forceAdaptiveThinking"),
      allowEmptySignature: b("allowEmptySignature"),
    };
  }

  // — advancedJson: collect every non-managed key so it round-trips —
  let advancedJson: Record<string, unknown> | null = null;
  for (const k of Object.keys(provider)) {
    if (!MANAGED_KEYS.has(k)) {
      advancedJson = advancedJson ?? {};
      advancedJson[k] = provider[k];
    }
  }

  // — mode/template are not stored in the per-provider object —
  const api =
    typeof provider.api === "string" ? provider.api : "openai-completions";
  const isBuiltin = piVendorPresets.some(
    (v) => v.providerId === seed.providerId && v.isBuiltin,
  );
  const mode: PiProviderMode =
    seed.mode ?? (isBuiltin ? "builtinOverride" : "custom");
  const template: PiProviderTemplate = seed.template ?? inferTemplate(api);

  return {
    mode,
    providerId: seed.providerId,
    template,
    baseUrl: typeof provider.baseUrl === "string" ? provider.baseUrl : "",
    api,
    apiKey: parseApiKey(provider.apiKey),
    headers,
    models,
    compat,
    advancedJson,
  };
}
