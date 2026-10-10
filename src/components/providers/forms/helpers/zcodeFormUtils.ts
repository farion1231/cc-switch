import type { ZCodeModel, ZCodeProviderConfig } from "@/types";

// ── Default configs ──────────────────────────────────────────────────

export const ZCODE_DEFAULT_KIND = "anthropic";
export const ZCODE_DEFAULT_CONFIG = JSON.stringify(
  {
    name: "",
    kind: ZCODE_DEFAULT_KIND,
    enabled: true,
    source: "custom",
    options: {
      baseURL: "",
      apiKey: "",
    },
    models: {},
  },
  null,
  2,
);

export const ZCODE_KNOWN_OPTION_KEYS = ["baseURL", "apiKey"] as const;

// Contains ":", which is not valid in an HTTP field name, so it cannot
// collide with a legitimate custom option from an existing configuration.
export const ZCODE_EXTRA_OPTION_DRAFT_PREFIX = "draft-option:";

// ── Pure functions ───────────────────────────────────────────────────

export function isKnownZcodeOptionKey(key: string): boolean {
  return ZCODE_KNOWN_OPTION_KEYS.includes(
    key as (typeof ZCODE_KNOWN_OPTION_KEYS)[number],
  );
}

export function parseZcodeConfig(
  settingsConfig?: Record<string, unknown>,
): ZCodeProviderConfig {
  const normalize = (
    parsed: Partial<ZCodeProviderConfig>,
  ): ZCodeProviderConfig => ({
    name: parsed.name,
    kind: parsed.kind || ZCODE_DEFAULT_KIND,
    enabled: parsed.enabled,
    source: parsed.source,
    options:
      parsed.options && typeof parsed.options === "object"
        ? (parsed.options as ZCodeProviderConfig["options"])
        : {},
    models:
      parsed.models && typeof parsed.models === "object"
        ? (parsed.models as Record<string, ZCodeModel>)
        : {},
  });

  try {
    const parsed = JSON.parse(
      settingsConfig ? JSON.stringify(settingsConfig) : ZCODE_DEFAULT_CONFIG,
    ) as Partial<ZCodeProviderConfig>;
    return normalize(parsed);
  } catch {
    return {
      kind: ZCODE_DEFAULT_KIND,
      options: {},
      models: {},
    };
  }
}

export function toZcodeExtraOptions(
  options: ZCodeProviderConfig["options"],
): Record<string, string> {
  const extra: Record<string, string> = {};
  for (const [k, v] of Object.entries(options || {})) {
    if (!isKnownZcodeOptionKey(k)) {
      extra[k] = typeof v === "string" ? v : JSON.stringify(v);
    }
  }
  return extra;
}
