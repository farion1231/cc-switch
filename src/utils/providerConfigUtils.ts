// 供应商配置处理工具函数

import type { TemplateValueConfig } from "../config/claudeProviderPresets";
import { normalizeQuotes } from "@/utils/textNormalization";
import { isPlainObject } from "@/utils/configMerge";

// Gemini 通用配置禁止的键（共享常量，供 hook 和同步逻辑复用）
export const GEMINI_COMMON_ENV_FORBIDDEN_KEYS = [
  "GOOGLE_GEMINI_BASE_URL",
  "GEMINI_API_KEY",
] as const;
export type GeminiForbiddenEnvKey =
  (typeof GEMINI_COMMON_ENV_FORBIDDEN_KEYS)[number];

const isSubset = (target: any, source: any): boolean => {
  if (isPlainObject(source)) {
    if (!isPlainObject(target)) return false;
    return Object.entries(source).every(([key, value]) =>
      isSubset(target[key], value),
    );
  }

  if (Array.isArray(source)) {
    if (!Array.isArray(target) || target.length !== source.length) return false;
    return source.every((item, index) => isSubset(target[index], item));
  }

  return target === source;
};

// 验证JSON配置格式
export const validateJsonConfig = (
  value: string,
  fieldName: string = "配置",
): string => {
  if (!value.trim()) {
    return "";
  }
  try {
    const parsed = JSON.parse(value);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return `${fieldName}必须是 JSON 对象`;
    }
    return "";
  } catch {
    return `${fieldName}JSON格式错误，请检查语法`;
  }
};

// 检查当前配置是否已包含通用配置片段
export const hasCommonConfigSnippet = (
  jsonString: string,
  snippetString: string,
): boolean => {
  try {
    if (!snippetString.trim()) return false;
    const config = jsonString ? JSON.parse(jsonString) : {};
    const snippet = JSON.parse(snippetString);
    if (!isPlainObject(snippet)) return false;
    return isSubset(config, snippet);
  } catch (err) {
    return false;
  }
};

// 检查 Gemini 配置是否已包含通用配置片段（env JSON）
export const hasGeminiCommonConfigSnippet = (
  jsonString: string,
  snippetString: string,
): boolean => {
  try {
    if (!snippetString.trim()) return false;
    const config = jsonString ? JSON.parse(jsonString) : {};
    if (!isPlainObject(config)) return false;
    const envValue = (config as Record<string, unknown>).env;
    if (envValue !== undefined && !isPlainObject(envValue)) return false;
    const env = (isPlainObject(envValue) ? envValue : {}) as Record<
      string,
      unknown
    >;

    const parsed = JSON.parse(snippetString);
    if (!isPlainObject(parsed)) return false;

    const entries = Object.entries(parsed).filter(
      (entry): entry is [string, string] => {
        const [key, value] = entry;
        if (
          GEMINI_COMMON_ENV_FORBIDDEN_KEYS.includes(
            key as GeminiForbiddenEnvKey,
          )
        ) {
          return false;
        }
        if (typeof value !== "string") return false;
        return value.trim().length > 0;
      },
    );

    if (entries.length === 0) return false;

    return entries.every(([key, value]) => {
      const current = env[key];
      return typeof current === "string" && current === value.trim();
    });
  } catch {
    return false;
  }
};

// 读取配置中的 API Key（支持 Claude, Codex, Gemini）
export const getApiKeyFromConfig = (
  jsonString: string,
  appType?: string,
): string => {
  try {
    const config = JSON.parse(jsonString);
    const env = config?.env;

    if (!env) return "";

    // Gemini API Key
    if (appType === "gemini") {
      const geminiKey = env.GEMINI_API_KEY;
      return typeof geminiKey === "string" ? geminiKey : "";
    }

    // Codex API Key
    if (appType === "codex") {
      const codexKey = env.CODEX_API_KEY;
      return typeof codexKey === "string" ? codexKey : "";
    }

    // Claude API Key (优先 ANTHROPIC_AUTH_TOKEN，其次 ANTHROPIC_API_KEY)
    const token = env.ANTHROPIC_AUTH_TOKEN;
    const apiKey = env.ANTHROPIC_API_KEY;
    const value =
      typeof token === "string"
        ? token
        : typeof apiKey === "string"
          ? apiKey
          : "";
    return value;
  } catch (err) {
    return "";
  }
};

// 模板变量替换
export const applyTemplateValues = (
  config: any,
  templateValues: Record<string, TemplateValueConfig> | undefined,
): any => {
  const resolvedValues = Object.fromEntries(
    Object.entries(templateValues ?? {}).map(([key, value]) => {
      const resolvedValue =
        value.editorValue !== undefined
          ? value.editorValue
          : (value.defaultValue ?? "");
      return [key, resolvedValue];
    }),
  );

  const replaceInString = (str: string): string => {
    return Object.entries(resolvedValues).reduce((acc, [key, value]) => {
      const placeholder = `\${${key}}`;
      if (!acc.includes(placeholder)) {
        return acc;
      }
      return acc.split(placeholder).join(value ?? "");
    }, str);
  };

  const traverse = (obj: any): any => {
    if (typeof obj === "string") {
      return replaceInString(obj);
    }
    if (Array.isArray(obj)) {
      return obj.map(traverse);
    }
    if (obj && typeof obj === "object") {
      const result: any = {};
      for (const [key, value] of Object.entries(obj)) {
        result[key] = traverse(value);
      }
      return result;
    }
    return obj;
  };

  return traverse(config);
};

// 判断配置中是否存在 API Key 字段
export const hasApiKeyField = (
  jsonString: string,
  appType?: string,
): boolean => {
  try {
    const config = JSON.parse(jsonString);
    const env = config?.env ?? {};

    if (appType === "gemini") {
      return Object.prototype.hasOwnProperty.call(env, "GEMINI_API_KEY");
    }

    if (appType === "codex") {
      return Object.prototype.hasOwnProperty.call(env, "CODEX_API_KEY");
    }

    return (
      Object.prototype.hasOwnProperty.call(env, "ANTHROPIC_AUTH_TOKEN") ||
      Object.prototype.hasOwnProperty.call(env, "ANTHROPIC_API_KEY")
    );
  } catch (err) {
    return false;
  }
};

// 写入/更新配置中的 API Key，默认不新增缺失字段
export const setApiKeyInConfig = (
  jsonString: string,
  apiKey: string,
  options: { createIfMissing?: boolean; appType?: string } = {},
): string => {
  const { createIfMissing = false, appType } = options;
  try {
    const config = JSON.parse(jsonString);
    if (!config.env) {
      if (!createIfMissing) return jsonString;
      config.env = {};
    }
    const env = config.env as Record<string, any>;

    // Gemini API Key
    if (appType === "gemini") {
      if ("GEMINI_API_KEY" in env) {
        env.GEMINI_API_KEY = apiKey;
      } else if (createIfMissing) {
        env.GEMINI_API_KEY = apiKey;
      } else {
        return jsonString;
      }
      return JSON.stringify(config, null, 2);
    }

    // Codex API Key
    if (appType === "codex") {
      if ("CODEX_API_KEY" in env) {
        env.CODEX_API_KEY = apiKey;
      } else if (createIfMissing) {
        env.CODEX_API_KEY = apiKey;
      } else {
        return jsonString;
      }
      return JSON.stringify(config, null, 2);
    }

    // Claude API Key (优先写入已存在的字段；若两者均不存在且允许创建，则默认创建 AUTH_TOKEN 字段)
    if ("ANTHROPIC_AUTH_TOKEN" in env) {
      env.ANTHROPIC_AUTH_TOKEN = apiKey;
    } else if ("ANTHROPIC_API_KEY" in env) {
      env.ANTHROPIC_API_KEY = apiKey;
    } else if (createIfMissing) {
      env.ANTHROPIC_AUTH_TOKEN = apiKey;
    } else {
      return jsonString;
    }
    return JSON.stringify(config, null, 2);
  } catch (err) {
    return jsonString;
  }
};

// 检查 TOML 配置是否已包含通用配置片段
export const hasTomlCommonConfigSnippet = (
  tomlString: string,
  snippetString: string,
): boolean => {
  if (!snippetString.trim()) return false;

  // 简单检查配置是否包含片段内容
  // 去除空白字符后比较，避免格式差异影响
  const normalizeWhitespace = (str: string) => str.replace(/\s+/g, " ").trim();

  return normalizeWhitespace(tomlString).includes(
    normalizeWhitespace(snippetString),
  );
};

// ========== Codex base_url utils ==========

// 从 Codex 的 TOML 配置文本中提取 base_url（支持单/双引号）
export const extractCodexBaseUrl = (
  configText: string | undefined | null,
): string | undefined => {
  try {
    const raw = typeof configText === "string" ? configText : "";
    // 归一化中文/全角引号，避免正则提取失败
    const text = normalizeQuotes(raw);
    if (!text) return undefined;
    const m = text.match(/base_url\s*=\s*(['"])([^'\"]+)\1/);
    return m && m[2] ? m[2] : undefined;
  } catch {
    return undefined;
  }
};

// 从 Provider 对象中提取 Codex base_url（当 settingsConfig.config 为 TOML 字符串时）
export const getCodexBaseUrl = (
  provider: { settingsConfig?: Record<string, any> } | undefined | null,
): string | undefined => {
  try {
    const text =
      typeof provider?.settingsConfig?.config === "string"
        ? (provider as any).settingsConfig.config
        : "";
    return extractCodexBaseUrl(text);
  } catch {
    return undefined;
  }
};

// 在 Codex 的 TOML 配置文本中写入或更新 base_url 字段
export const setCodexBaseUrl = (
  configText: string,
  baseUrl: string,
): string => {
  const trimmed = baseUrl.trim();
  if (!trimmed) {
    return configText;
  }
  // 归一化原文本中的引号（既能匹配，也能输出稳定格式）
  const normalizedText = normalizeQuotes(configText);

  const normalizedUrl = trimmed.replace(/\s+/g, "");
  const replacementLine = `base_url = "${normalizedUrl}"`;
  const pattern = /base_url\s*=\s*(["'])([^"']+)\1/;

  if (pattern.test(normalizedText)) {
    return normalizedText.replace(pattern, replacementLine);
  }

  const prefix =
    normalizedText && !normalizedText.endsWith("\n")
      ? `${normalizedText}\n`
      : normalizedText;
  return `${prefix}${replacementLine}\n`;
};

// ========== Codex model name utils ==========

// 从 Codex 的 TOML 配置文本中提取 model 字段（支持单/双引号）
export const extractCodexModelName = (
  configText: string | undefined | null,
): string | undefined => {
  try {
    const raw = typeof configText === "string" ? configText : "";
    // 归一化中文/全角引号，避免正则提取失败
    const text = normalizeQuotes(raw);
    if (!text) return undefined;

    // 匹配 model = "xxx" 或 model = 'xxx'
    const m = text.match(/^model\s*=\s*(['"])([^'"]+)\1/m);
    return m && m[2] ? m[2] : undefined;
  } catch {
    return undefined;
  }
};

// 在 Codex 的 TOML 配置文本中写入或更新 model 字段
export const setCodexModelName = (
  configText: string,
  modelName: string,
): string => {
  const trimmed = modelName.trim();
  if (!trimmed) {
    return configText;
  }

  // 归一化原文本中的引号（既能匹配，也能输出稳定格式）
  const normalizedText = normalizeQuotes(configText);

  const replacementLine = `model = "${trimmed}"`;
  const pattern = /^model\s*=\s*["']([^"']+)["']/m;

  if (pattern.test(normalizedText)) {
    return normalizedText.replace(pattern, replacementLine);
  }

  // 如果不存在 model 字段，尝试在 model_provider 之后插入
  // 如果 model_provider 也不存在，则插入到开头
  const providerPattern = /^model_provider\s*=\s*["'][^"']+["']/m;
  const match = normalizedText.match(providerPattern);

  if (match && match.index !== undefined) {
    // 在 model_provider 行之后插入
    const endOfLine = normalizedText.indexOf("\n", match.index);
    if (endOfLine !== -1) {
      return (
        normalizedText.slice(0, endOfLine + 1) +
        replacementLine +
        "\n" +
        normalizedText.slice(endOfLine + 1)
      );
    }
  }

  // 在文件开头插入
  const lines = normalizedText.split("\n");
  return `${replacementLine}\n${lines.join("\n")}`;
};

// ============================================================================
// Gemini Common Config Parsing Utilities
// ============================================================================

/**
 * Error codes for Gemini common config parsing.
 * These codes are used for consistent error handling and i18n mapping.
 */
export const GEMINI_CONFIG_ERROR_CODES = {
  NOT_OBJECT: "GEMINI_CONFIG_NOT_OBJECT",
  ENV_NOT_OBJECT: "GEMINI_CONFIG_ENV_NOT_OBJECT",
  VALUE_NOT_STRING: "GEMINI_CONFIG_VALUE_NOT_STRING",
  FORBIDDEN_KEYS: "GEMINI_CONFIG_FORBIDDEN_KEYS",
} as const;

/**
 * Result of parsing Gemini common config snippet
 */
export interface GeminiCommonConfigParseResult {
  /** Parsed env key-value pairs (empty if invalid) */
  env: Record<string, string>;
  /** Error message if parsing/validation failed (starts with error code) */
  error?: string;
  /** Warning message (non-fatal, config still usable) */
  warning?: string;
}

/**
 * Parse Gemini common config snippet with full validation.
 *
 * Supports three formats:
 * - ENV format: KEY=VALUE lines (one per line, # for comments)
 * - Flat JSON: {"KEY": "VALUE", ...}
 * - Wrapped JSON: {"env": {"KEY": "VALUE", ...}}
 *
 * Validation rules:
 * - Forbidden keys (GOOGLE_GEMINI_BASE_URL, GEMINI_API_KEY) are rejected
 * - Non-string values are rejected
 * - Empty string values are filtered out
 * - Arrays and non-plain objects are rejected
 *
 * @param snippet - The common config snippet string
 * @param options - Optional configuration
 * @returns Parse result with env, error, and warning
 */
export function parseGeminiCommonConfigSnippet(
  snippet: string,
  options?: {
    /** If true, reject forbidden keys with error; otherwise filter them with warning */
    strictForbiddenKeys?: boolean;
  },
): GeminiCommonConfigParseResult {
  const trimmed = snippet.trim();
  if (!trimmed) {
    return { env: {} };
  }

  const strictForbiddenKeys = options?.strictForbiddenKeys ?? true;
  let rawEnv: Record<string, unknown> = {};
  let isJson = false;

  // Try JSON first
  try {
    const parsed = JSON.parse(trimmed);

    // Must be a plain object (not array, null, etc.)
    if (!isPlainObject(parsed)) {
      return {
        env: {},
        error: `${GEMINI_CONFIG_ERROR_CODES.NOT_OBJECT}: must be a JSON object, not array or primitive`,
      };
    }

    isJson = true;

    // Check if wrapped format {"env": {...}}
    if ("env" in parsed) {
      const envField = parsed.env;
      if (!isPlainObject(envField)) {
        return {
          env: {},
          error: `${GEMINI_CONFIG_ERROR_CODES.ENV_NOT_OBJECT}: 'env' field must be a plain object`,
        };
      }
      rawEnv = envField as Record<string, unknown>;
    } else {
      // Flat format
      rawEnv = parsed as Record<string, unknown>;
    }
  } catch {
    // Not JSON, parse as ENV format (KEY=VALUE lines)
    isJson = false;
    for (const line of trimmed.split("\n")) {
      const lineTrimmed = line.trim();
      if (!lineTrimmed || lineTrimmed.startsWith("#")) continue;
      const equalIndex = lineTrimmed.indexOf("=");
      if (equalIndex > 0) {
        const key = lineTrimmed.substring(0, equalIndex).trim();
        // Strip surrounding quotes (single or double) from value
        // e.g., KEY="value" or KEY='value' -> value
        const rawValue = lineTrimmed.substring(equalIndex + 1).trim();
        const value = rawValue.replace(/^["'](.*)["']$/, "$1");
        if (key) {
          rawEnv[key] = value;
        }
      }
    }
  }

  // Validate and filter entries
  const env: Record<string, string> = {};
  const warnings: string[] = [];
  const forbiddenKeysFound: string[] = [];

  for (const [key, value] of Object.entries(rawEnv)) {
    // Check forbidden keys
    if (
      GEMINI_COMMON_ENV_FORBIDDEN_KEYS.includes(key as GeminiForbiddenEnvKey)
    ) {
      forbiddenKeysFound.push(key);
      continue;
    }

    // Must be string
    if (typeof value !== "string") {
      if (isJson) {
        return {
          env: {},
          error: `${GEMINI_CONFIG_ERROR_CODES.VALUE_NOT_STRING}: value for '${key}' must be a string, got ${typeof value}`,
        };
      }
      // For ENV format, skip non-strings silently (shouldn't happen)
      continue;
    }

    // Filter empty strings
    const trimmedValue = value.trim();
    if (!trimmedValue) {
      continue;
    }

    env[key] = trimmedValue;
  }

  // Handle forbidden keys
  if (forbiddenKeysFound.length > 0) {
    const msg = `${GEMINI_CONFIG_ERROR_CODES.FORBIDDEN_KEYS}: ${forbiddenKeysFound.join(", ")}`;
    if (strictForbiddenKeys) {
      return { env: {}, error: msg };
    }
    warnings.push(msg);
  }

  return {
    env,
    warning: warnings.length > 0 ? warnings.join("; ") : undefined,
  };
}

/**
 * Map Gemini common config warning to i18n-friendly message.
 *
 * @param warning - The raw warning string from parseGeminiCommonConfigSnippet
 * @param t - The i18n translation function
 * @returns Translated warning message
 */
export function mapGeminiWarningToI18n(
  warning: string,
  t: (
    key: string,
    options?: { keys?: string; defaultValue?: string },
  ) => string,
): string {
  if (warning.startsWith(GEMINI_CONFIG_ERROR_CODES.FORBIDDEN_KEYS)) {
    // Extract key list: "GEMINI_CONFIG_FORBIDDEN_KEYS: KEY1, KEY2" -> "KEY1, KEY2"
    const keys = warning.replace(
      `${GEMINI_CONFIG_ERROR_CODES.FORBIDDEN_KEYS}: `,
      "",
    );
    return t("geminiConfig.forbiddenKeysWarning", { keys });
  }
  // Other warnings: return as-is
  return warning;
}
