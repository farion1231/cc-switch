import { createUsageScript } from "@/types";
import { TEMPLATE_TYPES } from "@/config/constants";
import { extractCodexBaseUrl } from "@/utils/providerConfigUtils";

export interface CodingPlanProviderEntry {
  /** 与后端 QuotaTier 的 `codingPlanProvider` 取值对齐 */
  id: "kimi" | "zhipu" | "minimax" | "zenmux" | "volcengine";
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
  {
    id: "zenmux",
    label: "ZenMux",
    pattern: /zenmux\./i,
  },
  {
    // 火山方舟 Agent Plan / Coding Plan。base_url 形如
    // ark.cn-beijing.volces.com/api/{coding|plan}[/v3]；与后端 detect_provider 的
    // `volces.com/api/coding` 或 `volces.com/api/plan` 子串判断同效。
    // 两种路径需同时识别：用户配置 /api/coding（Coding Plan 入口）与
    // /api/plan（Agent Plan 入口，issue #4808）。
    // 后端走控制面 AK/SK SigV4：先探 GetAFPUsage，再 fallback GetCodingPlanUsage。
    id: "volcengine",
    label: "火山方舟 (Volcengine)",
    pattern: /volces\.com\/api\/(?:coding|plan)/i,
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
 * Issue #4808 回归点：判断 "saved usage_script 是 JS 模板但 baseUrl 实际是 Coding Plan 供应商"
 * 这一场景。UsageScriptModal 打开时命中即弹层建议切换到原生 token_plan，避免
 * 用户在 Test 按钮上踩到 reqwest 相对 URL 错误。
 */
export function isStaleJsScriptForCodingPlan(
  existingTemplateType: string | undefined | null,
  baseUrl: string | undefined | null,
): boolean {
  if (!existingTemplateType) return false;
  if (
    existingTemplateType === TEMPLATE_TYPES.TOKEN_PLAN ||
    existingTemplateType === TEMPLATE_TYPES.BALANCE ||
    existingTemplateType === TEMPLATE_TYPES.OFFICIAL_SUBSCRIPTION ||
    existingTemplateType === TEMPLATE_TYPES.GITHUB_COPILOT
  ) {
    return false;
  }
  return detectCodingPlanProvider(baseUrl) !== null;
}

/**
 * 从 provider/settingsConfig 中安全、鲁棒地提取 base_url
 */
export function extractBaseUrlFromProvider(
  appId: string,
  provider: any,
): string | undefined {
  const config = typeof provider?.settingsConfig === "string"
    ? (() => { try { return JSON.parse(provider.settingsConfig); } catch { return undefined; } })()
    : provider?.settingsConfig;
  let rawBaseUrl: string | undefined;

  if (config) {
    if (appId === "claude" || appId === "claude-desktop") {
      rawBaseUrl = config.env?.ANTHROPIC_BASE_URL || config.baseUrl || config.base_url;
    } else if (appId === "codex") {
      const configToml = config.config || "";
      rawBaseUrl = extractCodexBaseUrl(configToml) || config.env?.ANTHROPIC_BASE_URL;
    } else if (appId === "gemini") {
      rawBaseUrl = config.env?.GOOGLE_GEMINI_BASE_URL || config.env?.ANTHROPIC_BASE_URL;
    } else if (appId === "hermes") {
      rawBaseUrl = config.base_url || config.env?.ANTHROPIC_BASE_URL;
    } else if (appId === "openclaw") {
      rawBaseUrl = config.baseUrl || config.env?.ANTHROPIC_BASE_URL;
    } else if (appId === "opencode") {
      rawBaseUrl = config.options?.baseURL || config.env?.ANTHROPIC_BASE_URL;
    }
  }

  // 回退到顶层字段
  if (!rawBaseUrl) {
    rawBaseUrl = provider?.baseUrl || provider?.base_url;
  }

  return rawBaseUrl;
}

/**
 * 新建供应商时，若对应 app 的 base_url 命中 Coding Plan 路由表，
 * 自动把 `meta.usage_script` 标记为 token_plan 并启用。
 *
 * - 仅在 `meta.usage_script` 完全缺失时注入，不覆盖用户/UsageScriptModal 已有配置
 * - 适用于所有 app：使用 extractBaseUrlFromProvider 鲁棒地提取 base_url
 * - code 置空：Rust 端走专用 `coding_plan::get_coding_plan_quota`，不执行 JS 脚本
 * - 已绑定该供应商的既存 stale script（templateType 为 general/newapi/custom 等
 *   JS 模板）由 UsageScriptModal 打开时弹层提示升级，不由本函数默默改写
 */
export function injectCodingPlanUsageScript<
  T extends {
    settingsConfig?: Record<string, any>;
    meta?: Record<string, any>;
    baseUrl?: string;
    base_url?: string;
  },
>(appId: string, provider: T): T {
  if (provider.meta?.usage_script) return provider;

  const rawBaseUrl = extractBaseUrlFromProvider(appId, provider);
  const codingPlanProvider = detectCodingPlanProvider(
    typeof rawBaseUrl === "string" ? rawBaseUrl : null,
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
