/**
 * 通用配置格式适配器
 *
 * 提供 Claude (JSON), Codex (TOML), Gemini (ENV/JSON) 三种格式的适配器实现。
 */

import {
  computeFinalConfig,
  extractDifference,
  isPlainObject,
  isSubset,
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

// Re-export from utils for backward compatibility
export { hasContentByAppType } from "@/utils/commonConfigDetection";
import { hasGeminiContent } from "@/utils/commonConfigDetection";

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
    const result = claudeAdapter.parseSnippet(snippet);
    return (
      !result.error &&
      result.config !== null &&
      Object.keys(result.config).length > 0
    );
  },

  hasContent: (configStr: string, snippetStr: string): boolean => {
    try {
      if (!snippetStr.trim()) return false;
      const config = configStr ? JSON.parse(configStr) : {};
      const snippet = JSON.parse(snippetStr);
      if (!isPlainObject(snippet)) return false;
      return isSubset(config, snippet);
    } catch {
      return false;
    }
  },

  getApplyError: (snippet: string, t): string => {
    const result = claudeAdapter.parseSnippet(snippet);
    if (result.error) {
      return result.error;
    }
    if (result.config === null || Object.keys(result.config).length === 0) {
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
 *
 * 注意：如果 config 字段存在但不是 string，这表示 schema 异常
 * 此时返回空字符串并打印警告，避免静默处理错误数据
 */
function extractConfigToml(configInput: string): string {
  if (!configInput || !configInput.trim()) {
    return "";
  }

  // 尝试解析为 JSON（wrapper 格式）
  try {
    const parsed = JSON.parse(configInput);
    if (typeof parsed?.config === "string") {
      return parsed.config;
    }
    // 如果是 JSON 对象
    if (typeof parsed === "object" && parsed !== null) {
      // config 字段存在但不是 string，这是 schema 异常
      if ("config" in parsed && typeof parsed.config !== "string") {
        console.warn(
          `[extractConfigToml] config field is ${typeof parsed.config}, expected string`,
        );
      }
      // JSON 对象没有有效的 config 字段，返回空（无 TOML 可提取）
      return "";
    }
  } catch {
    // JSON 解析失败，说明是直接的 TOML 字符串
  }

  return configInput;
}

/**
 * 检测 Codex 配置是否是 JSON wrapper 格式
 * @returns 如果是 JSON wrapper 返回解析后的对象，否则返回 null
 */
function detectJsonWrapperFormat(
  codexConfig: string,
): { auth?: unknown; config?: unknown; [key: string]: unknown } | null {
  try {
    const parsed = JSON.parse(codexConfig);
    // 只有当解析结果是对象时才认为是 JSON wrapper
    if (
      typeof parsed === "object" &&
      parsed !== null &&
      !Array.isArray(parsed)
    ) {
      return parsed;
    }
  } catch {
    // 不是 JSON
  }
  return null;
}

/**
 * Codex 配置格式保留结果
 */
export interface PreserveCodexConfigResult {
  /** 格式化后的配置字符串 */
  config: string;
  /** 错误信息（如果有） */
  error?: string;
}

/**
 * 保留原始格式写回 Codex 配置
 *
 * 如果原始配置是 JSON wrapper 格式，则更新 config 字段并返回 JSON
 * 如果原始配置是纯 TOML 格式，则直接返回 TOML
 *
 * @param originalConfig - 原始配置字符串
 * @param updatedToml - 更新后的 TOML 内容
 * @returns 格式化后的配置字符串和可能的错误
 */
export function preserveCodexConfigFormat(
  originalConfig: string,
  updatedToml: string,
): PreserveCodexConfigResult {
  const jsonWrapper = detectJsonWrapperFormat(originalConfig);

  if (jsonWrapper) {
    // 是 JSON wrapper 格式
    if ("config" in jsonWrapper) {
      // 存在 config 字段
      if (typeof jsonWrapper.config !== "string") {
        // config 字段不是 string，这是意外的 schema
        // 返回原配置并报错，避免数据丢失
        return {
          config: originalConfig,
          error: `Codex config field is ${typeof jsonWrapper.config}, expected string. Cannot update safely.`,
        };
      }
      // 正常更新 config 字段
      jsonWrapper.config = updatedToml;
      return { config: JSON.stringify(jsonWrapper, null, 2) };
    } else {
      // 没有 config 字段，但是是 JSON 对象（可能包含 auth 等其他字段）
      // 添加 config 字段
      jsonWrapper.config = updatedToml;
      return { config: JSON.stringify(jsonWrapper, null, 2) };
    }
  }

  // 纯 TOML 格式
  return { config: updatedToml };
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

  hasContent: (configStr: string, snippetStr: string): boolean => {
    if (!snippetStr.trim()) return false;
    // 解析配置（可能是纯 TOML 或 JSON wrapper 格式）
    const configToml = extractConfigToml(configStr);
    const configParsed = safeParseToml(configToml);
    const snippetParsed = safeParseToml(snippetStr);
    if (configParsed.error || !configParsed.config) return false;
    if (snippetParsed.error || !snippetParsed.config) return false;
    // 使用 isSubset 检查 snippet 是否是 config 的子集
    return isSubset(configParsed.config, snippetParsed.config);
  },

  getApplyError: (snippet: string, t): string => {
    if (!hasTomlContent(snippet)) {
      return t("codexConfig.noCommonConfigToApply");
    }
    const error = validateTomlFormat(snippet);
    if (error) {
      return t("codexConfig.tomlFormatError", {
        defaultValue: "TOML 格式错误",
      });
    }
    return "";
  },

  parseInput: (input: string): string => {
    return extractConfigToml(input);
  },

  computeFinal: (custom: string, common: string, enabled: boolean): string => {
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
const GEMINI_DEFAULT_SNIPPET = "";

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

    parseSnippet: (snippet: string): ParseResult<Record<string, string>> => {
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

    hasContent: (configStr: string, snippetStr: string): boolean => {
      return hasGeminiContent(configStr, snippetStr).hasContent;
    },

    getApplyError: (snippet: string, t): string => {
      const result = parseGeminiCommonConfigSnippet(snippet, {
        strictForbiddenKeys: true,
      });

      if (result.errorInfo) {
        // Use structured error info for type-safe handling
        switch (result.errorInfo.code) {
          case GEMINI_CONFIG_ERROR_CODES.FORBIDDEN_KEYS:
            return t("geminiConfig.commonConfigInvalidKeys", {
              keys: result.errorInfo.keys?.join(", ") ?? "",
            });
          case GEMINI_CONFIG_ERROR_CODES.VALUE_NOT_STRING:
            return t("geminiConfig.commonConfigInvalidValues");
          default:
            return t("geminiConfig.invalidEnvFormat", {
              defaultValue: "配置格式错误",
            });
        }
      }

      // Fallback for legacy error string (backward compatibility)
      if (result.error) {
        return t("geminiConfig.invalidEnvFormat", {
          defaultValue: "配置格式错误",
        });
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
