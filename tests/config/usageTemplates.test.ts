import { describe, expect, it } from "vitest";
import {
  buildMiniMaxUsageTemplate,
  detectMiniMaxUsageConfig,
  MINIMAX_CN_USAGE_URL,
  MINIMAX_GLOBAL_USAGE_URL,
} from "@/config/usageTemplates/minimax";
import {
  buildZhipuUsageTemplate,
  detectZhipuUsageConfig,
  ZHIPU_CN_USAGE_URL,
  ZHIPU_GLOBAL_USAGE_URL,
} from "@/config/usageTemplates/zhipu";
import type { Provider } from "@/types";

const createProvider = (overrides: Partial<Provider> = {}): Provider => ({
  id: "provider-1",
  name: "Test Provider",
  settingsConfig: {},
  ...overrides,
});

describe("usage template detection", () => {
  it("detects MiniMax China from website URL", () => {
    const provider = createProvider({
      websiteUrl: "https://www.minimaxi.com",
    });

    expect(detectMiniMaxUsageConfig(provider, "claude")).toEqual({
      template: "minimax",
      baseUrl: MINIMAX_CN_USAGE_URL,
    });
  });

  it("detects MiniMax Global from runtime base URL", () => {
    const provider = createProvider({
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://api.minimax.io/v1",
        },
      },
    });

    expect(detectMiniMaxUsageConfig(provider, "claude")).toEqual({
      template: "minimax",
      baseUrl: MINIMAX_GLOBAL_USAGE_URL,
    });
  });

  it("does not detect MiniMax from display name alone", () => {
    const provider = createProvider({
      name: "MiniMax",
      icon: "minimax",
    });

    expect(detectMiniMaxUsageConfig(provider, "claude")).toBeNull();
  });

  it("detects Zhipu China from website URL", () => {
    const provider = createProvider({
      websiteUrl: "https://open.bigmodel.cn",
    });

    expect(detectZhipuUsageConfig(provider, "claude")).toEqual({
      template: "zhipu",
      baseUrl: ZHIPU_CN_USAGE_URL,
    });
  });

  it("detects Zhipu Global from runtime base URL", () => {
    const provider = createProvider({
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://api.z.ai/v1",
        },
      },
    });

    expect(detectZhipuUsageConfig(provider, "claude")).toEqual({
      template: "zhipu",
      baseUrl: ZHIPU_GLOBAL_USAGE_URL,
    });
  });

  it("does not detect Zhipu from display name alone", () => {
    const provider = createProvider({
      name: "Zhipu",
      icon: "zhipu",
    });

    expect(detectZhipuUsageConfig(provider, "claude")).toBeNull();
  });

  it("formats MiniMax reset time as relative countdown text", () => {
    const template = buildMiniMaxUsageTemplate({
      hours5Quota: "5小时额度",
      weeklyQuota: "每周额度",
      countUnit: "次",
      countdownHourUnit: "小时",
      countdownMinuteUnit: "分钟",
      resetInTemplate: "{{time}}后重置",
    });

    const config = new Function(`return ${template}`)() as {
      extractor: (response: unknown) => Array<{ extra?: string }>;
    };

    const result = config.extractor({
      base_resp: { status_code: 0 },
      model_remains: [
        {
          model_name: "MiniMax-M1",
          current_interval_total_count: 100,
          current_interval_usage_count: 20,
          current_weekly_total_count: 200,
          current_weekly_usage_count: 80,
          remains_time: 17160000,
        },
      ],
    });

    expect(result[0]?.extra).toBe("4 小时 46 分钟后重置");
  });

  it("maps Zhipu quotas by returned order instead of reset time", () => {
    const template = buildZhipuUsageTemplate({
      hours5Quota: "5小时额度",
      weeklyQuota: "每周额度",
      mcpMonthly: "MCP每月",
      mcpMonthlyUnit: "次",
      queryFailed: "查询失败",
      resetSuffix: "重置",
    });

    const config = new Function(`return ${template}`)() as {
      extractor: (response: unknown) => Array<{
        planName?: string;
        remaining?: number;
      }>;
    };

    const result = config.extractor({
      success: true,
      data: {
        level: "pro",
        limits: [
          {
            type: "TOKENS_LIMIT",
            percentage: 10,
            nextResetTime: 200,
          },
          {
            type: "TOKENS_LIMIT",
            percentage: 70,
            nextResetTime: 100,
          },
        ],
      },
    });

    expect(result[0]?.planName).toBe("PRO · 5小时额度");
    expect(result[0]?.remaining).toBe(90);
    expect(result[1]?.planName).toBe("PRO · 每周额度");
    expect(result[1]?.remaining).toBe(30);
  });
});
