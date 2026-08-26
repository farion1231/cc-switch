import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";

describe("usage labels", () => {
  it("describes totals as model-processed tokens and local pricing estimates", () => {
    expect(zh.usage.realTotal).toBe("模型处理 Tokens");
    expect(en.usage.realTotal).toBe("Model Processed Tokens");
    expect(zh.usage.totalCost).toBe("本地定价估算");
    expect(en.usage.totalCost).toBe("Local Pricing Estimate");
    expect(zh.usage.cost).toBe("本地定价估算");
    expect(en.usage.cost).toBe("Local Pricing Estimate");
    expect(zh.usage.avgCost).toBe("平均本地定价估算");
    expect(en.usage.avgCost).toBe("Average Local Pricing Estimate");
  });
});
