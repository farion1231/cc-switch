/**
 * 通用配置格式适配器
 *
 * 提供 Claude (JSON), Codex (TOML), Gemini (ENV/JSON) 三种格式的适配器实现。
 */

import { validateJsonConfig } from "@/utils/providerConfigUtils";
import {
  computeFinalConfig,
  extractDifference,
  isPlainObject,
} from "@/utils/configMerge";
import {
  computeFinalTomlConfig,
  extractTomlDifference,
  safeParseToml,
} from "@/utils/tomlConfigMerge";
import {
  parseGeminiCommonConfigSnippet,
  GEMINI_CONFIG_ERROR_CODES,
} from "@/utils/providerConfigUtils";
import type {
  CommonConfigAdapter,
  ParseResult,
  ExtractResult,
} from "./useCommonConfigBase";

// ============================================================================
// Claude Adapter (JSON)
// ============================================================================

const CLAUDE_LEGACY_STORAGE_KEY = "cc-switch:common-config-snippet";
const CLAUDE_DEFAULT_SNIPPET = `{
  "includeCoAuthoredBy": false
}`;

export const claudeAdapter: CommonConfigAdapter<
  Record<string, unknown>,
  string
> = {
  appKey: "claude",
  defaultSnippet: CLAUDE_DEFAULT_SNIPPET,
  legacyStorageKey: CLAUDE_LEGACY_STORAGE_KEY,

  parseSnippet: (snippet: string): ParseResult<Record<string, unknown>> => {
    const trimmed = snippet.trim();
    if (!trimmed) {
      return { config: {}, error: null };
    }

    try {
      const parsed = JSON.parse(trimmed);
      if (!isPlainObject(parsed)) {
        return { config: null, error: "JSON 格式错误：不是对象" };
      }
      return { config: parsed, error: null };
    } catch {
      return { config: null, error: "JSON 格式错误" };
    }
  },

  hasValidContent: (snippet: string): boolean => {
    try {
      const parsed = JSON.parse(snippet.trim());
      return isPlainObject(parsed) && Object.keys(parsed).length > 0;
    } catch {
      return false;
    }
  },

  getApplyError: (snippet: string, t): string => {
    if (!snippet.trim()) {
      return t("claudeConfig.noCommonConfigToApply");
    }
    const validationError = validateJsonConfig(snippet, "通用配置片段");
    if (validationError) {
      return validationError;
    }
    try {
      const parsed = JSON.parse(snippet) as Record<string, unknown>;
      if (Object.keys(parsed).length === 0) {
        return t("claudeConfig.noCommonConfigToApply");
      }
    } catch {
      return t("claudeConfig.noCommonConfigToApply");
    }
    return "";
  },

  parseInput: (input: string): Record<string, unknown> => {
    try {
      const parsed = JSON.parse(input || "{}");
      return isPlainObject(parsed) ? parsed : {};
    } catch {
      return {};
    }
  },

  computeFinal: (
    custom: Record<string, unknown>,
    common: Record<string, unknown>,
    enabled: boolean,
  ): string => {
    if (!enabled || Object.keys(common).length === 0) {
      return JSON.stringify(custom, null, 2);
    }
    const merged = computeFinalConfig(custom, common, true);
    return JSON.stringify(merged, null, 2);
  },

  extractDiff: (
    custom: Record<string, unknown>,
    common: Record<string, unknown>,
  ): ExtractResult<Record<string, unknown>> => {
    const result = extractDifference(custom, common);
    return {
      custom: result.customConfig,
      hasCommonKeys: result.hasCommonKeys,
    };
  },

  serializeOutput: (config: Record<string, unknown>): string => {
    return JSON.stringify(config, null, 2);
  },

  buildExtractRequest: (finalValue: string): { settingsConfig: string } => {
    return { settingsConfig: finalValue };
  },
};

// ============================================================================
// Codex Adapter (TOML)
// ============================================================================

const CODEX_LEGACY_STORAGE_KEY = "cc-switch:codex-common-config-snippet";
const CODEX_DEFAULT_SNIPPET = `# Common Codex config
# Add your common TOML configuration here`;

/** 检查 TOML 是否有实质内容（非空、非纯注释） */
function hasTomlContent(toml: string): boolean {
  const lines = toml.split("\n");
  return lines.some((line) => {
    const trimmed = line.trim();
    return trimmed && !trimmed.startsWith("#");
  });
}

/** 校验 TOML 格式 */
function validateTomlFormat(tomlText: string): string | null {
  if (!hasTomlContent(tomlText)) {
    return null; // 空或纯注释是合法的
  }
  const result = safeParseToml(tomlText);
  return result.error;
}

/**
 * 从 codexConfig 提取 config 字段（TOML 格式）
 * 支持两种格式：
 * 1. 直接的 TOML 字符串
 * 2. JSON 字符串 { auth: {...}, config: "TOML" }
 */
function extractConfigToml(configInput: string): string {
  if (!configInput || !configInput.trim()) {
    return "";
  }

  // 尝试解析为 JSON（旧格式）
  try {
    const parsed = JSON.parse(configInput);
    if (typeof parsed?.config === "string") {
      return parsed.config;
    }
    // 如果是 JSON 对象但没有 config 字段，返回空
    if (typeof parsed === "object" && parsed !== null) {
      return "";
    }
  } catch {
    // JSON 解析失败，说明是直接的 TOML 字符串
  }

  return configInput;
}

export const codexAdapter: CommonConfigAdapter<string, string> = {
  appKey: "codex",
  defaultSnippet: CODEX_DEFAULT_SNIPPET,
  legacyStorageKey: CODEX_LEGACY_STORAGE_KEY,

  parseSnippet: (snippet: string): ParseResult<string> => {
    const error = validateTomlFormat(snippet);
    if (error) {
      return { config: null, error };
    }
    return { config: snippet, error: null };
  },

  hasValidContent: (snippet: string): boolean => {
    return hasTomlContent(snippet) && !validateTomlFormat(snippet);
  },

  getApplyError: (snippet: string, t): string => {
    if (!hasTomlContent(snippet)) {
      return t("codexConfig.noCommonConfigToApply");
    }
    const error = validateTomlFormat(snippet);
    if (error) {
      return t("codexConfig.tomlFormatError", { defaultValue: "TOML 格式错误" });
    }
    return "";
  },

  parseInput: (input: string): string => {
    return extractConfigToml(input);
  },

  computeFinal: (
    custom: string,
    common: string,
    enabled: boolean,
  ): string => {
    if (!enabled || !hasTomlContent(common)) {
      return custom;
    }
    const result = computeFinalTomlConfig(custom, common, true);
    return result.error ? custom : result.finalConfig;
  },

  extractDiff: (custom: string, common: string): ExtractResult<string> => {
    const result = extractTomlDifference(custom, common);
    return {
      custom: result.customToml,
      hasCommonKeys: result.hasCommonKeys,
      error: result.error,
    };
  },

  serializeOutput: (config: string): string => {
    return config;
  },

  buildExtractRequest: (finalValue: string): { settingsConfig: string } => {
    return {
      settingsConfig: JSON.stringify({ config: finalValue ?? "" }),
    };
  },
};

// ============================================================================
// Gemini Adapter (ENV/JSON)
// ============================================================================

const GEMINI_LEGACY_STORAGE_KEY = "cc-switch:gemini-common-config-snippet";
const GEMINI_DEFAULT_SNIPPET = "{}";

export interface GeminiAdapterOptions {
  /** 字符串转对象 */
  envStringToObj: (envString: string) => Record<string, string>;
  /** 对象转字符串 */
  envObjToString: (envObj: Record<string, unknown>) => string;
}

/**
 * 创建 Gemini 适配器
 * 需要传入 env 转换函数，因为这些函数依赖于外部实现
 */
export function createGeminiAdapter(
  options: GeminiAdapterOptions,
): CommonConfigAdapter<Record<string, string>, Record<string, string>> {
  const { envStringToObj, envObjToString } = options;

  return {
    appKey: "gemini",
    defaultSnippet: GEMINI_DEFAULT_SNIPPET,
    legacyStorageKey: GEMINI_LEGACY_STORAGE_KEY,

    parseSnippet: (
      snippet: string,
    ): ParseResult<Record<string, string>> => {
      const result = parseGeminiCommonConfigSnippet(snippet, {
        strictForbiddenKeys: true,
      });

      if (result.error) {
        return { config: null, error: result.error };
      }

      return { config: result.env, error: null };
    },

    hasValidContent: (snippet: string): boolean => {
      const result = parseGeminiCommonConfigSnippet(snippet, {
        strictForbiddenKeys: true,
      });
      return !result.error && Object.keys(result.env).length > 0;
    },

    getApplyError: (snippet: string, t): string => {
      const result = parseGeminiCommonConfigSnippet(snippet, {
        strictForbiddenKeys: true,
      });

      if (result.error) {
        if (result.error.startsWith(GEMINI_CONFIG_ERROR_CODES.FORBIDDEN_KEYS)) {
          const keys = result.error.split(": ")[1] ?? result.error;
          return t("geminiConfig.commonConfigInvalidKeys", { keys });
        }
        if (
          result.error.startsWith(GEMINI_CONFIG_ERROR_CODES.VALUE_NOT_STRING)
        ) {
          return t("geminiConfig.commonConfigInvalidValues");
        }
        return t("geminiConfig.invalidJsonFormat");
      }

      if (Object.keys(result.env).length === 0) {
        return t("geminiConfig.noCommonConfigToApply");
      }

      return "";
    },

    parseInput: (input: string): Record<string, string> => {
      return envStringToObj(input);
    },

    computeFinal: (
      custom: Record<string, string>,
      common: Record<string, string>,
      enabled: boolean,
    ): Record<string, string> => {
      if (!enabled || Object.keys(common).length === 0) {
        return custom;
      }

      // 通用配置作为 base，自定义 env 覆盖
      const merged = computeFinalConfig(
        custom as Record<string, unknown>,
        common as Record<string, unknown>,
        true,
      );

      // 转换回 Record<string, string>
      const result: Record<string, string> = {};
      for (const [key, value] of Object.entries(merged)) {
        if (typeof value === "string") {
          result[key] = value;
        }
      }
      return result;
    },

    extractDiff: (
      custom: Record<string, string>,
      common: Record<string, string>,
    ): ExtractResult<Record<string, string>> => {
      const result = extractDifference(
        custom as Record<string, unknown>,
        common as Record<string, unknown>,
      );

      // 转换回 Record<string, string>
      const customResult: Record<string, string> = {};
      for (const [key, value] of Object.entries(result.customConfig)) {
        if (typeof value === "string") {
          customResult[key] = value;
        }
      }

      return {
        custom: customResult,
        hasCommonKeys: result.hasCommonKeys,
      };
    },

    serializeOutput: (config: Record<string, string>): string => {
      return envObjToString(config);
    },

    buildExtractRequest: (
      finalValue: Record<string, string>,
    ): { settingsConfig: string } => {
      return {
        settingsConfig: JSON.stringify({ env: finalValue }),
      };
    },
  };
}
