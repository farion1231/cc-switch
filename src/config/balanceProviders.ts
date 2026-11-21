/**
 * 官方余额查询供应商的 base_url 路由表，与后端
 * `src-tauri/src/services/balance.rs::detect_provider` 保持一致（按 host 标签边界匹配）。
 * 两边对同一组地址的识别结果由 tests/config/backendConstants.test.ts 校验。
 */
import { urlHostPattern } from "@/utils/urlHostPattern";

export const BALANCE_PROVIDERS = [
  {
    id: "deepseek",
    label: "DeepSeek",
    pattern: urlHostPattern(String.raw`api\.deepseek\.com`),
  },
  {
    id: "stepfun",
    label: "StepFun",
    pattern: urlHostPattern(String.raw`api\.stepfun\.(?:ai|com)`),
  },
  {
    id: "siliconflow",
    label: "SiliconFlow",
    pattern: urlHostPattern(String.raw`api\.siliconflow\.(?:cn|com)`),
  },
  {
    id: "openrouter",
    label: "OpenRouter",
    pattern: urlHostPattern(String.raw`openrouter\.ai`),
  },
  {
    id: "novita",
    label: "Novita AI",
    pattern: urlHostPattern(String.raw`api\.novita\.ai`),
  },
] as const;

export type BalanceProviderEntry = (typeof BALANCE_PROVIDERS)[number];

/** 根据 Base URL 认出官方余额查询供应商；认不出返回 undefined */
export function detectBalanceProvider(
  baseUrl: string | undefined,
): BalanceProviderEntry | undefined {
  if (!baseUrl) return undefined;
  return BALANCE_PROVIDERS.find((bp) => bp.pattern.test(baseUrl));
}
