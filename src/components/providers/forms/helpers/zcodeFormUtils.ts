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

const ZCODE_KNOWN_OPTION_KEYS = ["baseURL", "apiKey"] as const;

// ── Pure functions ───────────────────────────────────────────────────

function isKnownZcodeOptionKey(key: string): boolean {
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

/** `options` 里表单不管的键（旧版表单写的超时等），原样带过每次编辑。 */
export function extractZcodeOptionExtras(
  options?: ZCodeProviderConfig["options"],
): Record<string, unknown> {
  const extra: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(options || {})) {
    if (!isKnownZcodeOptionKey(k)) extra[k] = v;
  }
  return extra;
}

/** Structured keys the form owns; everything else is round-tripped verbatim. */
const ZCODE_KNOWN_TOP_LEVEL_KEYS = [
  "name",
  "kind",
  "enabled",
  "source",
  "options",
  "models",
] as const;

/**
 * Top-level keys outside the structured whitelist (e.g. `priority`, `tags`
 * written by the backend). The form must carry these through edits untouched —
 * rebuilding only the whitelist would silently delete them from live config.
 */
export function extractZcodeConfigExtras(
  settingsConfig?: Record<string, unknown>,
): Record<string, unknown> {
  if (!settingsConfig || typeof settingsConfig !== "object") return {};
  const extras: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(settingsConfig)) {
    if (!(ZCODE_KNOWN_TOP_LEVEL_KEYS as readonly string[]).includes(k)) {
      extras[k] = v;
    }
  }
  return extras;
}
