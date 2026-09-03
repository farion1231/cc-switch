import { parse as parseToml, stringify as stringifyToml } from "smol-toml";
import type { DeepLinkImportRequest } from "@/lib/api/deeplink";
import { decodeBase64Utf8 } from "@/lib/utils/base64";
import { isSensitiveConfigKey, maskSensitiveValue } from "@/utils/deeplinkRisk";

export interface ParsedDeepLinkConfig {
  type: "claude" | "codex" | "gemini" | "grokbuild";
  env?: Record<string, string>;
  auth?: Record<string, string>;
  tomlConfig?: string;
}

const maskStructuredSecrets = (
  value: unknown,
  key = "",
  inheritedSensitive = false,
): unknown => {
  const sensitive = inheritedSensitive || isSensitiveConfigKey(key);
  if (typeof value === "string") {
    return sensitive ? maskSensitiveValue(value) : value;
  }
  if (Array.isArray(value)) {
    return value.map((item) => maskStructuredSecrets(item, key, sensitive));
  }
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(
        ([childKey, childValue]) => [
          childKey,
          maskStructuredSecrets(childValue, childKey, sensitive),
        ],
      ),
    );
  }
  return value;
};

const sanitizeTomlForPreview = (configToml: string): string => {
  const parsed = parseToml(configToml) as Record<string, unknown>;
  return `${stringifyToml(maskStructuredSecrets(parsed) as Record<string, unknown>).trim()}\n`;
};

export function parseDeepLinkConfigPreview(
  request: Pick<DeepLinkImportRequest, "app" | "config" | "configFormat">,
): ParsedDeepLinkConfig | null {
  if (!request.config) return null;

  try {
    const decoded = decodeBase64Utf8(request.config);
    const format = request.configFormat?.trim().toLowerCase();

    if (request.app === "grokbuild" && format === "toml") {
      return {
        type: "grokbuild",
        tomlConfig: sanitizeTomlForPreview(decoded),
      };
    }

    const parsed = JSON.parse(decoded) as Record<string, unknown>;
    if (request.app === "claude") {
      return {
        type: "claude",
        env: (parsed.env as Record<string, string>) || {},
      };
    }
    if (request.app === "codex") {
      const config = typeof parsed.config === "string" ? parsed.config : "";
      return {
        type: "codex",
        auth: (parsed.auth as Record<string, string>) || {},
        tomlConfig: config ? sanitizeTomlForPreview(config) : "",
      };
    }
    if (request.app === "gemini") {
      return {
        type: "gemini",
        env: parsed as Record<string, string>,
      };
    }
    if (request.app === "grokbuild") {
      const config =
        typeof parsed.config === "string"
          ? parsed.config
          : stringifyToml(parsed);
      return {
        type: "grokbuild",
        tomlConfig: sanitizeTomlForPreview(config),
      };
    }
    return null;
  } catch (error) {
    console.error("Failed to parse deep link config preview:", error);
    return null;
  }
}
