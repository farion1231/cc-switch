/**
 * 通用配置内容检测工具
 *
 * 这些是纯函数，不依赖 React/hooks，可以安全地被 lib/api 层使用。
 * 解决层级反转问题：lib/api 不应依赖 hooks 层。
 */

import { isPlainObject, isSubset } from "@/utils/configMerge";
import { safeParseToml } from "@/utils/tomlConfigMerge";
import {
  parseGeminiCommonConfigSnippet,
  GEMINI_COMMON_ENV_FORBIDDEN_KEYS,
} from "@/utils/providerConfigUtils";

// ============================================================================
// 类型定义
// ============================================================================

export type CommonConfigAppType = "claude" | "codex" | "gemini";

/**
 * 内容检测结果
 */
export interface ContentDetectionResult {
  /** 是否包含通用配置内容 */
  hasContent: boolean;
  /** 检测失败原因（如果有） */
  parseError?: string;
}

// ============================================================================
// Claude 内容检测
// ============================================================================

/**
 * 检查 Claude 配置是否包含通用配置片段
 */
export function hasClaudeContent(
  configStr: string,
  snippetStr: string,
): ContentDetectionResult {
  try {
    if (!snippetStr.trim()) {
      return { hasContent: false };
    }
    const config = configStr ? JSON.parse(configStr) : {};
    const snippet = JSON.parse(snippetStr);
    if (!isPlainObject(snippet)) {
      return { hasContent: false, parseError: "snippet is not a plain object" };
    }
    return { hasContent: isSubset(config, snippet) };
  } catch (e) {
    return {
      hasContent: false,
      parseError: e instanceof Error ? e.message : String(e),
    };
  }
}

// ============================================================================
// Codex 内容检测
// ============================================================================

/**
 * 从 Codex 配置中提取 TOML 配置字符串
 * 支持两种格式：纯 TOML 或 JSON wrapper { config: "TOML" }
 */
export function extractCodexConfigToml(configInput: string): {
  toml: string;
  parseError?: string;
} {
  if (!configInput || !configInput.trim()) {
    return { toml: "" };
  }

  // 尝试解析为 JSON（wrapper 格式）
  try {
    const parsed = JSON.parse(configInput);
    if (typeof parsed?.config === "string") {
      return { toml: parsed.config };
    }
    // 是 JSON 对象但没有 string config 字段
    // 这可能是未知 schema，返回原字符串当 TOML 处理
    if (typeof parsed === "object" && parsed !== null) {
      // 如果有 config 但不是 string，返回错误
      if ("config" in parsed && typeof parsed.config !== "string") {
        return {
          toml: "",
          parseError: `config field exists but is ${typeof parsed.config}, expected string`,
        };
      }
      // 没有 config 字段的 JSON 对象，可能是纯 JSON 配置
      // 返回空，因为无法当 TOML 处理
      return { toml: "" };
    }
  } catch {
    // JSON 解析失败，说明是直接的 TOML 字符串
  }

  return { toml: configInput };
}

/**
 * 检查 Codex 配置是否包含通用配置片段
 */
export function hasCodexContent(
  configStr: string,
  snippetStr: string,
): ContentDetectionResult {
  if (!snippetStr.trim()) {
    return { hasContent: false };
  }

  // 解析配置（可能是纯 TOML 或 JSON wrapper 格式）
  const { toml: configToml, parseError: configError } =
    extractCodexConfigToml(configStr);
  if (configError) {
    return { hasContent: false, parseError: configError };
  }

  const configParsed = safeParseToml(configToml);
  const snippetParsed = safeParseToml(snippetStr);

  if (configParsed.error || !configParsed.config) {
    return {
      hasContent: false,
      parseError: configParsed.error || "failed to parse config TOML",
    };
  }
  if (snippetParsed.error || !snippetParsed.config) {
    return {
      hasContent: false,
      parseError: snippetParsed.error || "failed to parse snippet TOML",
    };
  }

  // 使用 isSubset 检查 snippet 是否是 config 的子集
  return { hasContent: isSubset(configParsed.config, snippetParsed.config) };
}

// ============================================================================
// Gemini 内容检测
// ============================================================================

/**
 * 检查 Gemini 配置是否包含通用配置片段
 */
export function hasGeminiContent(
  configStr: string,
  snippetStr: string,
): ContentDetectionResult {
  try {
    if (!snippetStr.trim()) {
      return { hasContent: false };
    }

    const config = configStr ? JSON.parse(configStr) : {};
    if (!isPlainObject(config)) {
      return { hasContent: false, parseError: "config is not a plain object" };
    }
    const envValue = (config as Record<string, unknown>).env;
    if (envValue !== undefined && !isPlainObject(envValue)) {
      return {
        hasContent: false,
        parseError: "env field is not a plain object",
      };
    }
    const env = (isPlainObject(envValue) ? envValue : {}) as Record<
      string,
      unknown
    >;

    const parseResult = parseGeminiCommonConfigSnippet(snippetStr, {
      strictForbiddenKeys: false,
    });
    if (parseResult.error) {
      return { hasContent: false, parseError: parseResult.error };
    }
    if (Object.keys(parseResult.env).length === 0) {
      return { hasContent: false };
    }

    const hasContent = Object.entries(parseResult.env).every(([key, value]) => {
      // 跳过禁用的键
      if (GEMINI_COMMON_ENV_FORBIDDEN_KEYS.includes(key as any)) {
        return true;
      }
      const current = env[key];
      return typeof current === "string" && current === value.trim();
    });

    return { hasContent };
  } catch (e) {
    return {
      hasContent: false,
      parseError: e instanceof Error ? e.message : String(e),
    };
  }
}

// ============================================================================
// 统一入口
// ============================================================================

/**
 * 检查配置是否包含通用配置片段（按 appType 分发）
 *
 * 这是给 lib/api/config.ts 使用的入口函数。
 * 返回结构化结果，包含检测结果和可能的错误信息。
 *
 * @param appType - 应用类型
 * @param configStr - 供应商的 settingsConfig 字符串
 * @param snippetStr - 通用配置片段字符串
 * @returns 检测结果
 */
export function detectContent(
  appType: CommonConfigAppType,
  configStr: string,
  snippetStr: string,
): ContentDetectionResult {
  switch (appType) {
    case "claude":
      return hasClaudeContent(configStr, snippetStr);
    case "codex":
      return hasCodexContent(configStr, snippetStr);
    case "gemini":
      return hasGeminiContent(configStr, snippetStr);
    default:
      return { hasContent: false };
  }
}

/**
 * 简化版：仅返回 boolean（向后兼容）
 *
 * 注意：此函数会吞掉解析错误，仅用于向后兼容。
 * 新代码应使用 detectContent 获取完整结果。
 */
export function hasContentByAppType(
  appType: CommonConfigAppType,
  configStr: string,
  snippetStr: string,
): boolean {
  return detectContent(appType, configStr, snippetStr).hasContent;
}
