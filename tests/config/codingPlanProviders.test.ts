import { describe, expect, it } from "vitest";
import {
  CODING_PLAN_PROVIDERS,
  detectCodingPlanProvider,
  injectCodingPlanUsageScript,
  isStaleJsScriptForCodingPlan,
} from "@/config/codingPlanProviders";

/**
 * Build a minimal provider-like object that satisfies injectCodingPlanUsageScript's
 * structural constraints (settingsConfig.env + optional meta). App-specific
 * configuration layouts (Codex auth + TOML, Hermes / OpenClaw / OpenCode
 * flattened options) do not affect the routing decision here because
 * `detectCodingPlanProvider` only inspects the resolved baseUrl — which we
 * inline via the ANTHROPIC_BASE_URL field for a single canonical input. For
 * non-Claude apps we still exercise the function with the same provider
 * shape to verify the no-app-restriction invariant (issue #4808 follow-up).
 */
function makeProvider(env: Record<string, string> = {}) {
  return {
    name: "volc-test",
    settingsConfig: { env },
  } as unknown as Parameters<typeof injectCodingPlanUsageScript>[1];
}

describe("detectCodingPlanProvider", () => {
  it("recognizes Volcengine Coding Plan /api/coding baseUrl", () => {
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

  it("recognizes Volcengine Agent Plan /api/plan baseUrl", () => {
    // Agent Plan（/api/plan）与 Coding Plan 同源火山方舟账号，
    // 走同一份 AK/SK 控制面签名流程（GetAFPUsage → GetCodingPlanUsage）。
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

  it("matches Volcengine paths case-insensitively", () => {
    // 与 Rust 端 `detect_provider` 大小写不敏感约定一致。
    expect(
      detectCodingPlanProvider(
        "HTTPS://ARK.CN-BEIJING.VOLCES.COM/API/PLAN",
      ),
    ).toBe("volcengine");
    expect(
      detectCodingPlanProvider(
        "HTTPS://ARK.CN-BEIJING.VOLCES.COM/API/CODING",
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
    expect(detectCodingPlanProvider("https://example.com/api/coding")).toBeNull();
  });

  it("keeps the Volcengine entry in the provider table", () => {
    // 防止被无意改名 / 删行时静默回归。
    const volcengine = CODING_PLAN_PROVIDERS.find((cp) => cp.id === "volcengine");
    expect(volcengine).toBeDefined();
    expect(volcengine?.label).toBe("火山方舟 (Volcengine)");
  });
});

describe("injectCodingPlanUsageScript", () => {
  // 修复回归点：旧实现把 appId 锁死在 "claude"，导致 Codex / Hermes / OpenCode
  // / OpenClaw / Claude Desktop 上的 Coding Plan 供应商拿不到 token_plan
  // 自动注入。Rust 端 `commands/provider.rs` 的 token_plan 分支通过
  // `resolve_native_credentials(app_type, …)` 已经按 app 取字段，前端
  // 不应再加 app 维度门控。
  const APPS = [
    "claude",
    "codex",
    "hermes",
    "opencode",
    "openclaw",
    "claude-desktop",
  ] as const;

  it.each(APPS)(
    "auto-injects token_plan usage_script for volcengine /api/coding on %s",
    (appId) => {
      const provider = makeProvider({
        ANTHROPIC_BASE_URL: "https://ark.cn-beijing.volces.com/api/coding",
      });
      const out = injectCodingPlanUsageScript(appId, provider);
      expect(out.meta?.usage_script?.templateType).toBe("token_plan");
      expect(out.meta?.usage_script?.codingPlanProvider).toBe("volcengine");
      expect(out.meta?.usage_script?.enabled).toBe(true);
    },
  );

  it.each(APPS)(
    "auto-injects token_plan usage_script for volcengine /api/plan on %s",
    (appId) => {
      // Agent Plan（/api/plan）走同一份 AK/SK 控制面签名，应同样触发自动注入。
      const provider = makeProvider({
        ANTHROPIC_BASE_URL: "https://ark.cn-beijing.volces.com/api/plan",
      });
      const out = injectCodingPlanUsageScript(appId, provider);
      expect(out.meta?.usage_script?.templateType).toBe("token_plan");
      expect(out.meta?.usage_script?.codingPlanProvider).toBe("volcengine");
      expect(out.meta?.usage_script?.enabled).toBe(true);
    },
  );

  it("preserves a previously-saved usage_script (does not overwrite)", () => {
    // 不覆盖用户/UsageScriptModal 已有配置——避免把 AK/SK 已填好的火山脚本重置。
    const provider = {
      ...makeProvider({
        ANTHROPIC_BASE_URL: "https://ark.cn-beijing.volces.com/api/coding",
      }),
      meta: {
        usage_script: {
          enabled: true,
          language: "javascript",
          code: "// existing user script",
          templateType: "general",
        },
      },
    } as unknown as Parameters<typeof injectCodingPlanUsageScript>[1];
    const out = injectCodingPlanUsageScript("claude", provider);
    expect(out.meta?.usage_script?.templateType).toBe("general");
    expect(out.meta?.usage_script?.code).toBe("// existing user script");
  });

  it("does not inject when baseUrl does not match any coding-plan vendor", () => {
    const provider = makeProvider({
      ANTHROPIC_BASE_URL: "https://api.openai.com/v1",
    });
    const out = injectCodingPlanUsageScript("claude", provider);
    expect(out.meta?.usage_script).toBeUndefined();
  });
});

describe("isStaleJsScriptForCodingPlan", () => {
  // Modal stale-script 弹层与 useState 的 selectedTemplate 自动路由共用此判定。
  // 涵盖三组输入：saved templateType、baseUrl、是否触发建议。

  it("flags a 'general' template paired with a Volcengine /api/coding baseUrl", () => {
    expect(
      isStaleJsScriptForCodingPlan(
        "general",
        "https://ark.cn-beijing.volces.com/api/coding",
      ),
    ).toBe(true);
  });

  it("also flags 'custom' and 'newapi' templates for the same /api/coding baseUrl", () => {
    expect(
      isStaleJsScriptForCodingPlan(
        "custom",
        "https://ark.cn-beijing.volces.com/api/coding",
      ),
    ).toBe(true);
    expect(
      isStaleJsScriptForCodingPlan(
        "newapi",
        "https://ark.cn-beijing.volces.com/api/coding",
      ),
    ).toBe(true);
  });

  it("flags a JS template when baseUrl is /api/plan (Agent Plan)", () => {
    // Agent Plan 与 Coding Plan 同源，走同一条 token_plan 路径。
    expect(
      isStaleJsScriptForCodingPlan(
        "general",
        "https://ark.cn-beijing.volces.com/api/plan",
      ),
    ).toBe(true);
    expect(
      isStaleJsScriptForCodingPlan(
        "custom",
        "https://ark.cn-beijing.volces.com/api/plan",
      ),
    ).toBe(true);
    expect(
      isStaleJsScriptForCodingPlan(
        "newapi",
        "https://ark.cn-beijing.volces.com/api/plan",
      ),
    ).toBe(true);
  });

  it("does not flag a JS template when baseUrl does not match a coding-plan vendor", () => {
    expect(isStaleJsScriptForCodingPlan("general", "https://api.openai.com/v1")).toBe(
      false,
    );
  });

  it("does not flag a native (token_plan / balance / official_subscription) template", () => {
    expect(
      isStaleJsScriptForCodingPlan(
        "token_plan",
        "https://ark.cn-beijing.volces.com/api/coding",
      ),
    ).toBe(false);
    expect(
      isStaleJsScriptForCodingPlan(
        "balance",
        "https://ark.cn-beijing.volces.com/api/coding",
      ),
    ).toBe(false);
  });

  it("treats undefined / empty inputs as non-stale", () => {
    expect(isStaleJsScriptForCodingPlan(undefined, undefined)).toBe(false);
    expect(
      isStaleJsScriptForCodingPlan(
        null,
        "https://ark.cn-beijing.volces.com/api/coding",
      ),
    ).toBe(false);
    expect(isStaleJsScriptForCodingPlan("general", undefined)).toBe(false);
  });
});