import { describe, expect, it } from "vitest";
import {
  CODING_PLAN_PROVIDERS,
  detectCodingPlanProvider,
} from "@/config/codingPlanProviders";

describe("detectCodingPlanProvider", () => {
  it("recognizes Volcengine Agentplan /api/plan baseUrl (issue #4808)", () => {
    // 火山方舟 Agent Plan 的正确 baseUrl 形如 ark.cn-beijing.volces.com/api/plan[/v3]，
    // 旧实现只匹配 /api/coding，导致用量查询把账号识别成未知供应商。
    expect(
      detectCodingPlanProvider(
        "https://ark.cn-beijing.volces.com/api/plan",
      ),
    ).toBe("volcengine");
    expect(
      detectCodingPlanProvider(
        "https://ark.cn-beijing.volces.com/api/plan/v3",
      ),
    ).toBe("volcengine");
  });

  it("still recognizes Volcengine Coding Plan /api/coding baseUrl", () => {
    // 旧 /api/coding 入口保留兼容，老用户无需迁移。
    expect(
      detectCodingPlanProvider(
        "https://ark.cn-beijing.volces.com/api/coding",
      ),
    ).toBe("volcengine");
    expect(
      detectCodingPlanProvider(
        "https://ark.cn-beijing.volces.com/api/coding/v3",
      ),
    ).toBe("volcengine");
  });

  it("matches Volcengine paths case-insensitively", () => {
    // 与 Rust 端 `detect_provider` 大小写不敏感约定一致。
    expect(
      detectCodingPlanProvider(
        "HTTPS://ARK.CN-BEIJING.VOLCES.COM/API/PLAN",
      ),
    ).toBe("volcengine");
  });

  it("does not match Volcengine pay-as-you-go paths (/api/v3, /api/compatible)", () => {
    // DouBaoSeed 按量付费走 /api/v3 与 /api/compatible，无套餐额度。
    expect(
      detectCodingPlanProvider("https://ark.cn-beijing.volces.com/api/v3"),
    ).toBeNull();
    expect(
      detectCodingPlanProvider(
        "https://ark.cn-beijing.volces.com/api/compatible",
      ),
    ).toBeNull();
  });

  it("returns null for unrelated URLs", () => {
    expect(detectCodingPlanProvider("")).toBeNull();
    expect(detectCodingPlanProvider(undefined)).toBeNull();
    expect(detectCodingPlanProvider(null)).toBeNull();
    expect(detectCodingPlanProvider("https://example.com/api/plan")).toBeNull();
  });

  it("keeps the Volcengine entry in the provider table", () => {
    // 防止被无意改名 / 删行时静默回归。
    const volcengine = CODING_PLAN_PROVIDERS.find((cp) => cp.id === "volcengine");
    expect(volcengine).toBeDefined();
    expect(volcengine?.label).toBe("火山方舟 (Volcengine)");
  });
});