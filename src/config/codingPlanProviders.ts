/**
 * Coding Plan 供应商的 base_url 路由表。
 *
 * 与后端 `src-tauri/src/services/coding_plan.rs::detect_provider` 保持一致：
 * 后端靠 `url.contains(...)` 做子串判断，前端这里用 RegExp 做同效匹配。
 * 新增供应商时改这一处即可（UsageScriptModal 下拉 + useProviderActions
 * 新建自动注入 + 托盘识别全部复用）。
 */
import { createUsageScript } from "@/types";
import { TEMPLATE_TYPES } from "@/config/constants";

export interface CodingPlanProviderEntry {
  /** 与后端 QuotaTier 的 `codingPlanProvider` 取值对齐 */
  id: "kimi" | "zhipu" | "minimax";
  /** UsageScriptModal 下拉显示用 */
  label: string;
  /** base_url 匹配规则 */
  pattern: RegExp;
}

export const CODING_PLAN_PROVIDERS: readonly CodingPlanProviderEntry[] = [
  { id: "kimi", label: "Kimi For Coding", pattern: /api\.kimi\.com\/coding/i },
  {
    id: "zhipu",
    label: "Zhipu GLM (智谱)",
    pattern: /bigmodel\.cn|api\.z\.ai/i,
  },
  {
    id: "minimax",
    label: "MiniMax",
    pattern: /api\.minimaxi?\.com|api\.minimax\.io/i,
  },
] as const;

/** 根据 Base URL 自动检测 Coding Plan 供应商；未命中返回 null */
export function detectCodingPlanProvider(
  baseUrl: string | undefined | null,
): CodingPlanProviderEntry["id"] | null {
  if (!baseUrl) return null;
  for (const cp of CODING_PLAN_PROVIDERS) {
    if (cp.pattern.test(baseUrl)) return cp.id;
  }
  return null;
}

/**
 * 新建 Claude 供应商时，若 `ANTHROPIC_BASE_URL` 命中 Coding Plan 路由表，
 * 自动把 `meta.usage_script` 标记为 token_plan 并启用。
 *
 * - 仅在 `meta.usage_script` 完全缺失时注入，不覆盖用户/UsageScriptModal 已有配置
 * - 仅对 Claude app 生效：后端 `commands/provider.rs` 的 token_plan 分支只处理 Claude
 *   supplier 的 `settings_config.env.ANTHROPIC_BASE_URL`
 * - code 置空：Rust 端走专用 `coding_plan::get_coding_plan_quota`，不执行 JS 脚本
 */
export function injectCodingPlanUsageScript<
  T extends {
    settingsConfig?: Record<string, any>;
    meta?: Record<string, any>;
  },
>(appId: string, provider: T): T {
  if (appId !== "claude") return provider;
  if (provider.meta?.usage_script) return provider;

  const baseUrl = provider.settingsConfig?.env?.ANTHROPIC_BASE_URL;
  const codingPlanProvider = detectCodingPlanProvider(
    typeof baseUrl === "string" ? baseUrl : null,
  );
  if (!codingPlanProvider) return provider;

  return {
    ...provider,
    meta: {
      ...(provider.meta ?? {}),
      usage_script: createUsageScript({
        enabled: true,
        templateType: TEMPLATE_TYPES.TOKEN_PLAN,
        codingPlanProvider,
      }),
    },
  };
}
