import { parse as parseToml, stringify as stringifyToml } from "smol-toml";

const SECRET_KEY_PATTERN =
  /(api[_-]?key|token|secret|password|authorization|credential|auth)/i;
const MASK = "********";

const maskValue = (value: unknown, keyHint = ""): unknown => {
  if (Array.isArray(value)) {
    return value.map((item) => maskValue(item, keyHint));
  }

  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(([key, item]) => [
        key,
        maskValue(item, key),
      ]),
    );
  }

  if (typeof value === "string" && SECRET_KEY_PATTERN.test(keyHint)) {
    return value.trim() ? MASK : value;
  }

  return value;
};

/** Codex keeps config.toml as one string; credentials inside it (gateway
 * `experimental_bearer_token`, `http_headers.Authorization`, …) are masked
 * key by key. TOML that doesn't parse is hidden whole rather than shown raw. */
const maskToml = (toml: string): string => {
  try {
    const parsed = parseToml(toml) as Record<string, unknown>;
    return `${stringifyToml(maskValue(parsed) as Record<string, unknown>).trim()}\n`;
  } catch {
    return toml.trim() ? MASK : toml;
  }
};

/** Remote provider settings for the preview, with every credential masked. */
export const maskRemoteSettingsConfig = (
  config: Record<string, unknown>,
): unknown => {
  const masked = maskValue(config) as Record<string, unknown>;
  if (typeof config.config === "string") {
    masked.config = maskToml(config.config);
  }
  return masked;
};
