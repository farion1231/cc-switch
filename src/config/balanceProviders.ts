/**
 * 官方余额查询（BALANCE 模板）的供应商路由表。
 *
 * 与后端 `src-tauri/src/services/balance.rs::detect_provider` 保持一致：
 * 后端多数供应商靠 `url.contains(...)` 判断，88API 按 host 标签边界匹配，
 * 前端这里用 RegExp 做同效匹配。
 */
import { createUsageScript } from "@/types";
import { TEMPLATE_TYPES } from "@/config/constants";
import { extractBaseUrlForUsageDetection } from "@/config/codingPlanProviders";

export interface BalanceProviderEntry {
  id: string;
  /** UsageScriptModal 显示用 */
  label: string;
  /** base_url 匹配规则 */
  pattern: RegExp;
  /**
   * 新建供应商时是否自动启用余额查询。其余供应商同一地址可能还有别的查询方式，
   * 仍由用户自己选模板；88API 用推理 Key 就能查本站账户钱包，不需要额外凭据。
   */
  enabledByDefault?: boolean;
}

export const BALANCE_PROVIDERS: readonly BalanceProviderEntry[] = [
  { id: "deepseek", label: "DeepSeek", pattern: /api\.deepseek\.com/i },
  { id: "stepfun", label: "StepFun", pattern: /api\.stepfun\.(ai|com)/i },
  {
    id: "siliconflow",
    label: "SiliconFlow",
    pattern: /api\.siliconflow\.(cn|com)/i,
  },
  { id: "openrouter", label: "OpenRouter", pattern: /openrouter\.ai/i },
  { id: "novita", label: "Novita AI", pattern: /api\.novita\.ai/i },
  {
    // 只认 HTTPS 下的 88api.ai 及其子域名（同后端 api88_origin），不认
    // 88api.ai.example.com 这类伪造后缀或出现在路径里的域名
    id: "88api",
    label: "88API",
    pattern: /^https:\/\/(?:[^/?#@]*@)?(?:[\w-]+\.)*88api\.ai(?=[:/?#]|$)/i,
    enabledByDefault: true,
  },
];

/** 根据 Base URL 自动检测余额查询供应商；未命中返回 null */
export function detectBalanceProvider(
  baseUrl: string | undefined | null,
): BalanceProviderEntry | null {
  if (!baseUrl) return null;
  return BALANCE_PROVIDERS.find((bp) => bp.pattern.test(baseUrl)) ?? null;
}

/**
 * 新建供应商时，若 base_url 命中默认启用的余额供应商（目前只有 88API），
 * 自动把 `meta.usage_script` 设为 balance 模板并启用，从预设添加后卡片直接显示余额。
 *
 * - 仅在 `meta.usage_script` 完全缺失时注入，不覆盖用户或深链接已带的配置
 * - code 置空：Rust 端走 `balance::get_balance`，不执行 JS 脚本
 */
export function injectBalanceUsageScript<
  T extends {
    settingsConfig?: Record<string, any>;
    meta?: Record<string, any>;
  },
>(appId: string, provider: T): T {
  if (provider.meta?.usage_script) return provider;

  const baseUrl = extractBaseUrlForUsageDetection(
    appId,
    provider.settingsConfig,
  );
  if (!detectBalanceProvider(baseUrl)?.enabledByDefault) return provider;

  return {
    ...provider,
    meta: {
      ...(provider.meta ?? {}),
      usage_script: createUsageScript({
        enabled: true,
        templateType: TEMPLATE_TYPES.BALANCE,
      }),
    },
  };
}
