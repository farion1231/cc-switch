// 配置相关 API
import { invoke } from "@tauri-apps/api/core";

export type AppType = "claude" | "codex" | "gemini" | "omo" | "omo_slim";

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
