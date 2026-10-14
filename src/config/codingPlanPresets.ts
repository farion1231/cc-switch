/**
 * Coding Plan 套餐并发上限预设（会话粘性负载均衡的一键填充起点）。
 *
 * 数值来源（设计决策 5，2026-10-07 确认），sourceKey 指向 i18n tooltip：
 * - Kimi 五档：用户指定；其中 Max=8 与官网定价页口径（Agent concurrent
 *   tasks Max=4）不一致，tooltip 双标注，以用户指定为准。
 * - GLM：控制台实测（glm-5.3=30、glm-5.3-flash=50）。
 * - MiMo：官网无公开数字，留空（不限），运行时靠 429/403 冷却自适应。
 *
 * 三家官方均为动态调整机制，预设仅作填充起点，可手改。
 * Kimi 档位名以"第 N 档/Max"中性命名（官方档位名未在设计中确认）。
 */

export interface CodingPlanConcurrencyPreset {
  id: string;
  /** i18n label key（套餐档位名） */
  labelKey: string;
  /** 账号默认并发上限；undefined = 留空不限 */
  maxConcurrentRequests?: number;
  /** 按模型覆盖（key = outbound model 名） */
  maxConcurrentByModel?: Record<string, number>;
  /** 来源标注 i18n key（tooltip） */
  sourceKey: string;
}

export const CODING_PLAN_CONCURRENCY_PRESETS: readonly CodingPlanConcurrencyPreset[] =
  [
    {
      id: "kimi_tier1",
      labelKey: "codingPlanPresets.kimiTier1",
      maxConcurrentRequests: 1,
      sourceKey: "codingPlanPresets.sources.userSpecified",
    },
    {
      id: "kimi_tier2",
      labelKey: "codingPlanPresets.kimiTier2",
      maxConcurrentRequests: 1,
      sourceKey: "codingPlanPresets.sources.userSpecified",
    },
    {
      id: "kimi_tier3",
      labelKey: "codingPlanPresets.kimiTier3",
      maxConcurrentRequests: 2,
      sourceKey: "codingPlanPresets.sources.userSpecified",
    },
    {
      id: "kimi_tier4",
      labelKey: "codingPlanPresets.kimiTier4",
      maxConcurrentRequests: 2,
      sourceKey: "codingPlanPresets.sources.userSpecified",
    },
    {
      id: "kimi_max",
      labelKey: "codingPlanPresets.kimiMax",
      maxConcurrentRequests: 8,
      sourceKey: "codingPlanPresets.sources.kimiMax",
    },
    {
      id: "glm_max",
      labelKey: "codingPlanPresets.glmMax",
      maxConcurrentRequests: 30,
      maxConcurrentByModel: { "glm-5.3-flash": 50 },
      sourceKey: "codingPlanPresets.sources.glmConsole",
    },
    {
      id: "mimo",
      labelKey: "codingPlanPresets.mimo",
      sourceKey: "codingPlanPresets.sources.mimoUnspecified",
    },
  ] as const;
