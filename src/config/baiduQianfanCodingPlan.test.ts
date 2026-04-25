import { describe, expect, it } from "vitest";
import {
  BAIDU_QIANFAN_CODING_PLAN,
  BAIDU_QIANFAN_CODING_PLAN_MODELS,
} from "./baiduQianfanCodingPlan";

describe("Baidu Qianfan Coding Plan config", () => {
  it("uses official coding plan endpoints", () => {
    expect(BAIDU_QIANFAN_CODING_PLAN.anthropicBaseUrl).toBe(
      "https://qianfan.baidubce.com/anthropic/coding",
    );
    expect(BAIDU_QIANFAN_CODING_PLAN.openaiBaseUrl).toBe(
      "https://qianfan.baidubce.com/v2/coding",
    );
  });

  it("uses the dynamic Coding Plan model as the default", () => {
    expect(BAIDU_QIANFAN_CODING_PLAN.defaultModel).toBe("qianfan-code-latest");
    expect(BAIDU_QIANFAN_CODING_PLAN_MODELS).toHaveProperty(
      "qianfan-code-latest",
    );
  });

  it("contains the coding plan model set exposed by Qianfan docs", () => {
    expect(Object.keys(BAIDU_QIANFAN_CODING_PLAN_MODELS).sort()).toEqual([
      "deepseek-v3.2",
      "ernie-4.5-turbo-20260402",
      "glm-5",
      "kimi-k2.5",
      "minimax-m2.5",
      "qianfan-code-latest",
    ]);
  });
});
