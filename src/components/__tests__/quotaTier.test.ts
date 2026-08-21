import { describe, expect, it } from "vitest";

import type { UsageData } from "@/types";
import { toQuotaTier } from "@/components/UsageFooter";

describe("toQuotaTier", () => {
  it("V2 套餐：extra 为裸 resets_at 字符串时，积分字段缺失", () => {
    const data: UsageData = {
      planName: "5小时",
      used: 30,
      extra: "2026-08-20T00:00:00Z",
    };
    const tier = toQuotaTier(data);
    expect(tier.usedCredits).toBeUndefined();
    expect(tier.maxCredits).toBeUndefined();
    expect(tier.utilization).toBe(30);
    expect(tier.resetsAt).toBe("2026-08-20T00:00:00Z");
  });

  it("V3 套餐：extra 为 JSON 时解析出 usedCredits/maxCredits", () => {
    const data: UsageData = {
      planName: "5小时",
      used: 30,
      extra: JSON.stringify({
        resetsAt: "2026-08-20T00:00:00Z",
        usedCredits: 300,
        maxCredits: 1000,
      }),
    };
    const tier = toQuotaTier(data);
    expect(tier.usedCredits).toBe(300);
    expect(tier.maxCredits).toBe(1000);
    expect(tier.resetsAt).toBe("2026-08-20T00:00:00Z");
    expect(tier.utilization).toBe(30);
  });

  it("V3 套餐：JSON 缺少积分字段时降级为 null（前端回退到百分比）", () => {
    const data: UsageData = {
      planName: "5小时",
      used: 10,
      extra: JSON.stringify({ resetsAt: "2026-08-20T00:00:00Z" }),
    };
    const tier = toQuotaTier(data);
    expect(tier.usedCredits).toBeNull();
    expect(tier.maxCredits).toBeNull();
  });

  it("ZenMux：JSON 携带 USD 字段时积分字段仍为 null", () => {
    const data: UsageData = {
      planName: "Pro",
      used: 50,
      extra: JSON.stringify({
        resetsAt: null,
        usedValueUsd: 1.23,
        maxValueUsd: 10.0,
        planLabel: "ZenMux·PRO",
      }),
    };
    const tier = toQuotaTier(data);
    expect(tier.usedValueUsd).toBe(1.23);
    expect(tier.maxValueUsd).toBe(10.0);
    expect(tier.usedCredits).toBeNull();
    expect(tier.maxCredits).toBeNull();
    expect(tier.planLabel).toBe("ZenMux·PRO");
  });

  it("extra 为非法 JSON 但以 { 开头时回退到裸字符串", () => {
    const data: UsageData = {
      planName: "5小时",
      used: 5,
      extra: "{not valid json",
    };
    const tier = toQuotaTier(data);
    expect(tier.resetsAt).toBe("{not valid json");
    expect(tier.usedCredits).toBeUndefined();
  });

  it("extra 为空时返回空 resetsAt", () => {
    const data: UsageData = { planName: "5小时", used: 0 };
    const tier = toQuotaTier(data);
    expect(tier.resetsAt).toBeNull();
    expect(tier.utilization).toBe(0);
  });
});
