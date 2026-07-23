import { parse as parseToml, stringify as stringifyToml } from "smol-toml";
import type { DeepLinkImportRequest } from "@/lib/api/deeplink";

export interface ParsedDeepLinkConfig {
  type: "claude" | "codex" | "gemini" | "grokbuild";
  env?: Record<string, string>;
  auth?: Record<string, string>;
  tomlConfig?: string;
}

const isSensitiveKey = (key: string) =>
  ["TOKEN", "KEY", "SECRET", "PASSWORD"].some((marker) =>
    key.toUpperCase().includes(marker),
  );

const maskSensitiveValue = (value: string) =>
  value.length > 4 ? `${value.slice(0, 4)}${"*".repeat(12)}` : "****";

const maskStructuredSecrets = (value: unknown, key = ""): unknown => {
  if (typeof value === "string") {
    return isSensitiveKey(key) ? maskSensitiveValue(value) : value;
  }
  if (Array.isArray(value)) {
    return value.map((item) => maskStructuredSecrets(item, key));
  }
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(
        ([childKey, childValue]) => [
          childKey,
          maskStructuredSecrets(childValue, childKey),
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

const decodeBase64Utf8 = (encoded: string): string => {
  const binary = atob(encoded);
  const bytes = Uint8Array.from(
    binary,
    (character) => character.codePointAt(0) ?? 0,
  );
  return new TextDecoder().decode(bytes);
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
