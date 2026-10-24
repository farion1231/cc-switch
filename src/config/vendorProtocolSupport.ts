/**
 * 厂商官方支持的协议集合（按下拉可选协议过滤）
 *
 * 调研口径：各厂商官方文档/手册（2026-10 核实）：
 * - Kimi（月之暗面，含 Coding Plan）：Anthropic + OpenAI 双协议
 *   https://www.kimi.com/code/docs/en（api.kimi.com/coding/ 为 Anthropic、/coding/v1 为 OpenAI）
 * - DeepSeek：Anthropic 兼容 + OpenAI 兼容
 *   https://api-docs.deepseek.com/guides/anthropic_api
 * - 智谱 GLM：/api/anthropic（Anthropic）+ /api/paas/v4（OpenAI 兼容）
 *   https://docs.bigmodel.cn/cn/guide/develop/others
 * - SiliconFlow：OpenAI 兼容 v1 + Anthropic 兼容（Claude Code 接入）
 *   https://www.siliconflow.com/blog/deepseek-v4-pro-claude-code-siliconflow
 * - StepFun Step Plan：step_plan（Anthropic）+ step_plan/v1（OpenAI 兼容）
 *   https://platform.stepfun.com/docs/zh/step-plan/quick-start
 * - MiniMax：Anthropic 兼容（M3 系）+ OpenAI 兼容 v1
 *   https://platform.minimax.io/docs/api-reference/text-anthropic-api
 * - Xiaomi MiMo（含 Token Plan）：Anthropic + OpenAI
 *   https://mimo.mi.com/docs/quick-start/first-api-call
 * - CherryIN（聚合网关）：Anthropic + OpenAI + Gemini
 *   https://docs.cherryin.ai/zh/docs/newapi/openai-compatible-usage
 * - Anthropic 官方：仅 Anthropic Messages
 *
 * 未收录的厂商/自定义网关不设限（全部协议可选）——网关类通常可跨格式转换。
 *
 * openai_responses 官方确认支持（补研 2026-10）：
 * - Kimi/Moonshot：官方 Code 文档声明 "Chat Completions and Responses protocols"
 *   https://www.kimi.com/code/docs/en（api.kimi.ai / api.moonshot.ai 的 /v1/responses）
 * - 智谱 GLM：官方 Response API 兼容文档
 *   https://docs.bigmodel.cn/cn/guide/develop/responses/introduction
 * - 小米 MiMo：官方 Responses 接口文档
 *   https://mimo.mi.com/docs/zh-CN/api/chat/responses
 * SiliconFlow/DeepSeek/StepFun/MiniMax 官方文档未见 Responses 端点，不列（宁缺毋滥）。
 */

export type VendorApiFormat =
  | "anthropic"
  | "openai_chat"
  | "openai_responses"
  | "gemini_native";

/** 键：预设 family（优先）或预设名（无 family 的兜底） */
const VENDOR_PROTOCOL_SUPPORT: Record<string, VendorApiFormat[]> = {
  kimi: ["anthropic", "openai_chat", "openai_responses"],
  zhipu: ["anthropic", "openai_chat", "openai_responses"],
  siliconflow: ["anthropic", "openai_chat"],
  stepfun: ["anthropic", "openai_chat"],
  minimax: ["anthropic", "openai_chat"],
  "xiaomi-mimo": ["anthropic", "openai_chat", "openai_responses"],
  // 无 family 的按名称兜底
  DeepSeek: ["anthropic", "openai_chat"],
  CherryIN: ["anthropic", "openai_chat", "gemini_native"],
  "Claude Official": ["anthropic"],
};

export const ALL_API_FORMATS: VendorApiFormat[] = [
  "anthropic",
  "openai_chat",
  "openai_responses",
  "gemini_native",
];

/**
 * 查厂商官方支持的协议集合；未收录返回 null（不设限，全部可选）。
 */
export function vendorSupportedFormats(
  family: string | undefined,
  name: string,
): VendorApiFormat[] | null {
  if (family && VENDOR_PROTOCOL_SUPPORT[family]) {
    return VENDOR_PROTOCOL_SUPPORT[family];
  }
  return VENDOR_PROTOCOL_SUPPORT[name] ?? null;
}
