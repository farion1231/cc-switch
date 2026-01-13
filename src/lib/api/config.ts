// 配置相关 API
import { invoke } from "@tauri-apps/api/core";

export type AppType = "claude" | "codex" | "gemini";

/**
 * 获取 Claude 通用配置片段（已废弃，使用 getCommonConfigSnippet）
 * @returns 通用配置片段（JSON 字符串），如果不存在则返回 null
 * @deprecated 使用 getCommonConfigSnippet('claude') 替代
 */
export async function getClaudeCommonConfigSnippet(): Promise<string | null> {
  return invoke<string | null>("get_claude_common_config_snippet");
}

/**
 * 设置 Claude 通用配置片段（已废弃，使用 setCommonConfigSnippet）
 * @param snippet - 通用配置片段（JSON 字符串）
 * @throws 如果 JSON 格式无效
 * @deprecated 使用 setCommonConfigSnippet('claude', snippet) 替代
 */
export async function setClaudeCommonConfigSnippet(
  snippet: string,
): Promise<void> {
  return invoke("set_claude_common_config_snippet", { snippet });
}

/**
 * 获取通用配置片段（统一接口）
 * @param appType - 应用类型（claude/codex/gemini）
 * @returns 通用配置片段（原始字符串），如果不存在则返回 null
 */
export async function getCommonConfigSnippet(
  appType: AppType,
): Promise<string | null> {
  return invoke<string | null>("get_common_config_snippet", { appType });
}

/**
 * 设置通用配置片段（统一接口）
 * @param appType - 应用类型（claude/codex/gemini）
 * @param snippet - 通用配置片段（原始字符串）
 * @throws 如果格式无效（Claude/Gemini 验证 JSON，Codex 暂不验证）
 */
export async function setCommonConfigSnippet(
  appType: AppType,
  snippet: string,
): Promise<void> {
  return invoke("set_common_config_snippet", { appType, snippet });
}

/**
 * 提取通用配置片段
 *
 * 默认读取当前激活供应商的配置；若传入 `options.settingsConfig`，则从编辑器当前内容提取。
 * 会自动排除差异化字段（API Key、模型配置、端点等），返回可复用的通用配置片段。
 *
 * @param appType - 应用类型（claude/codex/gemini）
 * @param options - 可选：提取来源
 * @returns 提取的通用配置片段（JSON/TOML 字符串）
 */
export type ExtractCommonConfigSnippetOptions = {
  settingsConfig?: string;
};

export async function extractCommonConfigSnippet(
  appType: AppType,
  options?: ExtractCommonConfigSnippetOptions,
): Promise<string> {
  const args: Record<string, unknown> = { appType };
  const settingsConfig = options?.settingsConfig;

  if (typeof settingsConfig === "string" && settingsConfig.trim()) {
    args.settingsConfig = settingsConfig;
  }

  return invoke<string>("extract_common_config_snippet", args);
}
