import type { OpenCodeModel, OpenCodeProviderConfig } from "@/types";
import { isPlainObject } from "@/lib/requestOverrides";

// ── Default configs ──────────────────────────────────────────────────

export const CLAUDE_DEFAULT_CONFIG = JSON.stringify({ env: {} }, null, 2);
export const CLAUDE_DESKTOP_DEFAULT_CONFIG = JSON.stringify(
  {
    env: {
      ANTHROPIC_BASE_URL: "",
      ANTHROPIC_AUTH_TOKEN: "",
    },
  },
  null,
  2,
);
export const CODEX_DEFAULT_CONFIG = JSON.stringify(
  { auth: {}, config: "" },
  null,
  2,
);
export const GEMINI_DEFAULT_CONFIG = JSON.stringify(
  {
    env: {
      GOOGLE_GEMINI_BASE_URL: "",
      GEMINI_API_KEY: "",
      GEMINI_MODEL: "gemini-3.6-flash",
    },
  },
  null,
  2,
);

export const OPENCODE_DEFAULT_NPM = "@ai-sdk/openai-compatible";
export const OPENCODE_DEFAULT_CONFIG = JSON.stringify(
  {
    npm: OPENCODE_DEFAULT_NPM,
    options: {
      baseURL: "",
      apiKey: "",
      setCacheKey: true,
    },
    models: {},
  },
  null,
  2,
);
export const OPENCODE_KNOWN_OPTION_KEYS = [
  "baseURL",
  "apiKey",
  "headers",
] as const;

// Contains ":", which is not valid in an HTTP field name, so it cannot
// collide with a legitimate custom header from an existing configuration.
export { REQUEST_HEADER_DRAFT_PREFIX as OPENCODE_HEADER_DRAFT_PREFIX } from "./requestHeaders";
export const OPENCODE_EXTRA_OPTION_DRAFT_PREFIX = "draft-option:";

export const OPENCLAW_DEFAULT_CONFIG = JSON.stringify(
  {
    baseUrl: "",
    apiKey: "",
    api: "openai-completions",
    models: [],
  },
  null,
  2,
);

// ── Pure functions ───────────────────────────────────────────────────

// Keys only a V1 declaration has, and keys only a native one has. Mirrors
// provider_format in src-tauri/src/opencode_config.rs.
const OPENCODE_LEGACY_ONLY_KEYS = ["npm", "options", "api"];
const OPENCODE_NATIVE_ONLY_KEYS = [
  "package",
  "settings",
  "headers",
  "body",
  "canonical",
];

function parseJson(json: string): unknown {
  try {
    return JSON.parse(json);
  } catch {
    return undefined;
  }
}

/** Keep native declarations out of the V1 structured editor. Source metadata
 * disambiguates built-in overrides containing only models (or an empty object).
 * A pasted full config holding `providers` counts as native too.
 */
export function isNativeOpencodeConfig(
  json: string,
  source?: "v1" | "v2",
): boolean {
  if (source) return source === "v2";
  const value = parseJson(json);
  if (!isPlainObject(value)) return false;
  if (OPENCODE_LEGACY_ONLY_KEYS.some((key) => key in value)) return false;
  return [...OPENCODE_NATIVE_ONLY_KEYS, "providers"].some(
    (key) => key in value,
  );
}

/** A declaration that does not rely on a built-in definition: it names a
 * package (`npm` for V1, `package` for native) and at least one model.
 */
export function hasOpencodeDefinition(
  declaration: unknown,
  packageKey: "npm" | "package",
): boolean {
  if (!isPlainObject(declaration)) return false;
  const pkg = declaration[packageKey];
  const { models } = declaration;
  return (
    typeof pkg === "string" &&
    pkg.trim() !== "" &&
    isPlainObject(models) &&
    Object.keys(models).length > 0
  );
}

/** The native form's check: a pasted full config holds the declaration under
 * `providers.<id>`, where the backend also takes it from.
 */
export function hasNativeOpencodeDefinition(
  json: string,
  providerId: string,
): boolean {
  const value = parseJson(json);
  const providers = isPlainObject(value) ? value.providers : undefined;
  const declaration = isPlainObject(providers)
    ? (providers[providerId] ?? value)
    : value;
  return hasOpencodeDefinition(declaration, "package");
}

export function isKnownOpencodeOptionKey(key: string): boolean {
  return OPENCODE_KNOWN_OPTION_KEYS.includes(
    key as (typeof OPENCODE_KNOWN_OPTION_KEYS)[number],
  );
}

export function parseOpencodeConfig(
  settingsConfig?: Record<string, unknown>,
): OpenCodeProviderConfig {
  const normalize = (
    parsed: Partial<OpenCodeProviderConfig>,
  ): OpenCodeProviderConfig => ({
    npm: parsed.npm ?? (settingsConfig ? "" : OPENCODE_DEFAULT_NPM),
    options:
      parsed.options && typeof parsed.options === "object"
        ? (parsed.options as OpenCodeProviderConfig["options"])
        : {},
    models:
      parsed.models && typeof parsed.models === "object"
        ? (parsed.models as Record<string, OpenCodeModel>)
        : {},
  });

  try {
    const parsed = JSON.parse(
      settingsConfig ? JSON.stringify(settingsConfig) : OPENCODE_DEFAULT_CONFIG,
    ) as Partial<OpenCodeProviderConfig>;
    return normalize(parsed);
  } catch {
    return {
      npm: OPENCODE_DEFAULT_NPM,
      options: {},
      models: {},
    };
  }
}

export function parseOpencodeConfigStrict(
  settingsConfig?: Record<string, unknown>,
): OpenCodeProviderConfig {
  const parsed = JSON.parse(
    settingsConfig ? JSON.stringify(settingsConfig) : OPENCODE_DEFAULT_CONFIG,
  ) as Partial<OpenCodeProviderConfig>;
  return {
    npm: parsed.npm ?? (settingsConfig ? "" : OPENCODE_DEFAULT_NPM),
    options:
      parsed.options && typeof parsed.options === "object"
        ? (parsed.options as OpenCodeProviderConfig["options"])
        : {},
    models:
      parsed.models && typeof parsed.models === "object"
        ? (parsed.models as Record<string, OpenCodeModel>)
        : {},
  };
}

export const OPENCODE_KNOWN_MODEL_KEYS = ["name", "limit", "options"] as const;

export function isKnownModelKey(key: string): boolean {
  return OPENCODE_KNOWN_MODEL_KEYS.includes(
    key as (typeof OPENCODE_KNOWN_MODEL_KEYS)[number],
  );
}

export function getModelExtraFields(
  model: OpenCodeModel,
): Record<string, string> {
  const extra: Record<string, string> = {};
  for (const [k, v] of Object.entries(model)) {
    if (!isKnownModelKey(k)) {
      extra[k] = typeof v === "string" ? v : JSON.stringify(v);
    }
  }
  return extra;
}

export function toOpencodeExtraOptions(
  options: OpenCodeProviderConfig["options"],
): Record<string, string> {
  const extra: Record<string, string> = {};
  for (const [k, v] of Object.entries(options || {})) {
    if (!isKnownOpencodeOptionKey(k)) {
      extra[k] = typeof v === "string" ? v : JSON.stringify(v);
    }
  }
  return extra;
}

export { buildOmoProfilePreview } from "@/types/omo";
