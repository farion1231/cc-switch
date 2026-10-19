import { describe, it, expect } from "vitest";
import {
  detectCodingPlanProvider,
  extractBaseUrlForUsageDetection,
  injectCodingPlanUsageScript,
} from "./codingPlanProviders";

// codex 预设的 config 由 generateThirdPartyConfig 生成，这里取其等效形态
const OPENCODE_GO_CODEX_TOML = `model_provider = "custom"
model = "glm-5.2"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = "opencode_go"
base_url = "https://opencode.ai/zen/go/v1"
wire_api = "responses"
requires_openai_auth = true`;

describe("detectCodingPlanProvider (OpenCode Go)", () => {
  it("matches both base variants across apps", () => {
    // claude/claude-desktop 预设是 /zen/go，codex/opencode/pi 是 /zen/go/v1
    expect(detectCodingPlanProvider("https://opencode.ai/zen/go")).toBe(
      "opencode_go",
    );
    expect(detectCodingPlanProvider("https://opencode.ai/zen/go/v1")).toBe(
      "opencode_go",
    );
  });

  it("does not match OpenCode Zen (pay-as-you-go, no usage API)", () => {
    expect(detectCodingPlanProvider("https://opencode.ai/zen/v1")).toBeNull();
  });
});

describe("detectCodingPlanProvider (MiniMax)", () => {
  it.each([
    "https://api.minimax.cn/v1",
    "https://api.minimaxi.com/v1",
    "https://api.minimax.io/v1",
    "https://API.MINIMAX.CN/anthropic",
  ])("recognizes the MiniMax usage provider for %s", (baseUrl) => {
    expect(detectCodingPlanProvider(baseUrl)).toBe("minimax");
  });

  it.each([
    "https://api.minimax.cn.example.com/v1",
    "https://proxy.example.com/api.minimax.io/v1",
  ])("ignores look-alike MiniMax hosts such as %s", (baseUrl) => {
    expect(detectCodingPlanProvider(baseUrl)).toBeNull();
  });
});

describe("extractBaseUrlForUsageDetection", () => {
  it("reads env.ANTHROPIC_BASE_URL for claude and claude-desktop", () => {
    const config = {
      env: { ANTHROPIC_BASE_URL: "https://opencode.ai/zen/go" },
    };
    expect(extractBaseUrlForUsageDetection("claude", config)).toBe(
      "https://opencode.ai/zen/go",
    );
    expect(extractBaseUrlForUsageDetection("claude-desktop", config)).toBe(
      "https://opencode.ai/zen/go",
    );
  });

  it("reads base_url from the codex TOML config", () => {
    expect(
      extractBaseUrlForUsageDetection("codex", {
        auth: { OPENAI_API_KEY: "" },
        config: OPENCODE_GO_CODEX_TOML,
      }),
    ).toBe("https://opencode.ai/zen/go/v1");
  });

  it("reads options.baseURL for opencode and baseUrl for pi", () => {
    expect(
      extractBaseUrlForUsageDetection("opencode", {
        options: { baseURL: "https://opencode.ai/zen/go/v1" },
      }),
    ).toBe("https://opencode.ai/zen/go/v1");
    expect(
      extractBaseUrlForUsageDetection("pi", {
        baseUrl: "https://opencode.ai/zen/go/v1",
      }),
    ).toBe("https://opencode.ai/zen/go/v1");
  });

  it("reads the remaining apps' native base URL fields", () => {
    const kimi = "https://api.kimi.com/coding/v1";
    expect(
      extractBaseUrlForUsageDetection("gemini", {
        env: { GOOGLE_GEMINI_BASE_URL: kimi },
      }),
    ).toBe(kimi);
    expect(
      extractBaseUrlForUsageDetection("grokbuild", {
        config: `[models]\ndefault = "k"\n\n[model.k]\nmodel = "kimi-for-coding"\nbase_url = "${kimi}"\n`,
      }),
    ).toBe(kimi);
    expect(extractBaseUrlForUsageDetection("hermes", { base_url: kimi })).toBe(
      kimi,
    );
    expect(extractBaseUrlForUsageDetection("openclaw", { baseUrl: kimi })).toBe(
      kimi,
    );
    expect(
      extractBaseUrlForUsageDetection("mcode", { options: { baseURL: kimi } }),
    ).toBe(kimi);
  });

  it("returns null for unknown apps", () => {
    expect(
      extractBaseUrlForUsageDetection("unknown", { baseUrl: "https://x" }),
    ).toBeNull();
  });
});

type TestProvider = {
  settingsConfig?: Record<string, any>;
  meta?: Record<string, any>;
};

describe("injectCodingPlanUsageScript", () => {
  const inject = (appId: string, provider: TestProvider) =>
    injectCodingPlanUsageScript(appId, provider);
  const expectInjected = (provider: TestProvider) => {
    expect(provider.meta?.usage_script).toMatchObject({
      enabled: true,
      templateType: "token_plan",
      codingPlanProvider: "opencode_go",
    });
  };

  it("injects OpenCode Go for every app that ships its preset", () => {
    expectInjected(
      inject("claude", {
        settingsConfig: {
          env: { ANTHROPIC_BASE_URL: "https://opencode.ai/zen/go" },
        },
      }),
    );
    expectInjected(
      inject("claude-desktop", {
        settingsConfig: {
          env: { ANTHROPIC_BASE_URL: "https://opencode.ai/zen/go" },
        },
      }),
    );
    expectInjected(
      inject("codex", {
        settingsConfig: { config: OPENCODE_GO_CODEX_TOML },
      }),
    );
    expectInjected(
      inject("opencode", {
        settingsConfig: {
          options: { baseURL: "https://opencode.ai/zen/go/v1" },
        },
      }),
    );
    expectInjected(
      inject("pi", {
        settingsConfig: { baseUrl: "https://opencode.ai/zen/go/v1" },
      }),
    );
  });

  it("keeps the existing claude behavior for other coding plans", () => {
    const injected = inject("claude", {
      settingsConfig: {
        env: { ANTHROPIC_BASE_URL: "https://api.kimi.com/coding/v1" },
      },
    });
    expect(injected.meta?.usage_script?.codingPlanProvider).toBe("kimi");
  });

  it("injects every coding plan for non-claude apps", () => {
    expect(
      inject("opencode", {
        settingsConfig: {
          options: { baseURL: "https://open.bigmodel.cn/api/anthropic" },
        },
      }).meta?.usage_script,
    ).toMatchObject({ enabled: true, codingPlanProvider: "zhipu" });
    expect(
      inject("hermes", {
        settingsConfig: { base_url: "https://api.kimi.com/coding/v1" },
      }).meta?.usage_script,
    ).toMatchObject({ enabled: true, codingPlanProvider: "kimi" });
  });

  it("keeps ZenMux claude-only", () => {
    const settingsConfig = { options: { baseURL: "https://zenmux.ai/api/v1" } };
    expect(
      inject("opencode", { settingsConfig }).meta?.usage_script,
    ).toBeUndefined();
  });

  it("never overwrites an existing usage_script", () => {
    const provider: TestProvider = {
      settingsConfig: {
        env: { ANTHROPIC_BASE_URL: "https://opencode.ai/zen/go" },
      },
      meta: { usage_script: { enabled: false, templateType: "custom" } },
    };
    expect(inject("claude", provider).meta?.usage_script).toEqual({
      enabled: false,
      templateType: "custom",
    });
  });
});
