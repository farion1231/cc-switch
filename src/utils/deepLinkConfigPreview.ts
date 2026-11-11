import { parse as parseToml, stringify as stringifyToml } from "smol-toml";
import type { DeepLinkImportRequest } from "@/lib/api/deeplink";
import { decodeBase64Utf8 } from "@/lib/utils/base64";
import { isSensitiveConfigKey, maskSensitiveValue } from "@/utils/deeplinkRisk";

export interface ParsedDeepLinkConfig {
  type: "claude" | "codex" | "gemini" | "grokbuild" | "agy" | "copilot";
  env?: Record<string, string>;
  model?: string;
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
  request: Pick<
    DeepLinkImportRequest,
    "app" | "config" | "configFormat" | "apiKey" | "endpoint" | "model"
  >,
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
    if (request.app === "agy") {
      const env = parsed.env ?? parsed;
      if (!env || typeof env !== "object" || Array.isArray(env)) return null;
      const values = env as Record<string, unknown>;
      const config = parsed.config as
        | { model?: { name?: unknown } }
        | undefined;
      const model = config?.model?.name ?? values.GEMINI_MODEL;
      return {
        type: "agy",
        env: Object.fromEntries(
          ["GEMINI_API_KEY", "GOOGLE_GEMINI_BASE_URL", "GEMINI_BASE_URL"]
            .filter((key) => typeof values[key] === "string")
            .map((key) => [key, values[key] as string]),
        ),
        model: typeof model === "string" ? model : undefined,
      };
    }
    if (request.app === "copilot") {
      const native = parsed.provider !== undefined;
      const node = native ? parsed.provider : (parsed.env ?? parsed);
      if (!node || typeof node !== "object" || Array.isArray(node)) return null;
      const values = node as Record<string, unknown>;
      const field = (key: string) =>
        typeof values[key] === "string" ? values[key].trim() : undefined;
      const endpoint = native ? "baseUrl" : "COPILOT_PROVIDER_BASE_URL";
      const apiKey = native ? "apiKey" : "COPILOT_PROVIDER_API_KEY";
      const bearer = native ? "bearerToken" : "COPILOT_PROVIDER_BEARER_TOKEN";
      const env: Record<string, string> = {};
      const url = request.endpoint ?? field(endpoint);
      if (url) env[endpoint] = url;
      const credentialKey =
        request.apiKey === undefined && field(bearer) ? bearer : apiKey;
      const credential = request.apiKey ?? field(credentialKey);
      if (credential) env[credentialKey] = credential;

      const ids = new Set<string>();
      if (request.model?.trim()) ids.add(request.model.trim());
      if (native && Array.isArray(parsed.models)) {
        for (const model of parsed.models) {
          if (typeof model?.id === "string" && model.id.trim())
            ids.add(model.id.trim());
        }
      } else if (!native && !ids.size) {
        const legacy = field("COPILOT_MODEL");
        if (legacy) ids.add(legacy);
      }
      return { type: "copilot", env, model: [...ids].join(", ") || undefined };
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
