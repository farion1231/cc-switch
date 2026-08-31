import { describe, expect, it } from "vitest";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";
import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";
import {
  openclawProviderPresets,
  rebaseOpenClawSuggestedDefaults,
} from "@/config/openclawProviderPresets";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";
import {
  extractCodexBaseUrl,
  extractCodexModelName,
  extractCodexWireApi,
} from "@/utils/providerConfigUtils";

// 腾讯云 Token Plan（产品线 1300/1823 订阅套餐）国内 + 国际两站预设。
// 与 TokenHub 按量 API 市场（1823 线，Hunyuan 预设的 /v1 端点）是两条
// 产品线：Key 不互通，端点也不互作候选（官方明示不支持跨地域、跨站调用）。
const DOMESTIC_WEBSITE_URL = "https://cloud.tencent.com/product/tokenhub";
const INTL_WEBSITE_URL = "https://www.tencentcloud.com/products/tokenhub";
const DOMESTIC_TOKEN_PLAN_API_KEY_URL =
  "https://console.cloud.tencent.com/tokenhub/tokenplan";
const DOMESTIC_ENTERPRISE_API_KEY_URL =
  "https://console.cloud.tencent.com/tokenhub/tokenplan-e";
const INTL_TOKEN_PLAN_API_KEY_URL =
  "https://console.tencentcloud.com/tokenhub/tokenplan";
const INTL_ENTERPRISE_API_KEY_URL =
  "https://console.tencentcloud.com/tokenhub/tokenplan-e";

const DOMESTIC_PERSONAL_ANTHROPIC =
  "https://api.lkeap.cloud.tencent.com/plan/anthropic";
const DOMESTIC_PERSONAL_OPENAI = "https://api.lkeap.cloud.tencent.com/plan/v3";
const DOMESTIC_ENTERPRISE_ANTHROPIC =
  "https://tokenhub.tencentmaas.com/plan/anthropic";
const DOMESTIC_ENTERPRISE_OPENAI = "https://tokenhub.tencentmaas.com/plan/v3";
// 国内站企业套餐的新加坡地域端点（1823/130659、131173 双地域表；
// 与国际站 tencentcloudmaas.com 域是不同后端，仅国内站订阅 Key 可用）
const DOMESTIC_SINGAPORE_ANTHROPIC =
  "https://tokenhub-intl.tencentmaas.com/plan/anthropic";
const DOMESTIC_SINGAPORE_OPENAI =
  "https://tokenhub-intl.tencentmaas.com/plan/v3";
// 国际站文档（1300/81489、1300/81490）钦定的新加坡端点域
const INTL_ANTHROPIC =
  "https://tokenhub-intl.tencentcloudmaas.com/plan/anthropic";
const INTL_OPENAI = "https://tokenhub-intl.tencentcloudmaas.com/plan/v3";
// 国际站企业套餐的广州地域端点（1300/81489、81490 双地域表）
const INTL_GUANGZHOU_ANTHROPIC =
  "https://tokenhub.tencentcloudmaas.com/plan/anthropic";
const INTL_GUANGZHOU_OPENAI = "https://tokenhub.tencentcloudmaas.com/plan/v3";

const products = [
  {
    name: "Tencent Token Plan",
    site: "domestic" as const,
    apiKeyUrl: DOMESTIC_TOKEN_PLAN_API_KEY_URL,
    anthropicBaseUrl: DOMESTIC_PERSONAL_ANTHROPIC,
    anthropicCandidates: [DOMESTIC_PERSONAL_ANTHROPIC],
    openaiBaseUrl: DOMESTIC_PERSONAL_OPENAI,
    openaiCandidates: [DOMESTIC_PERSONAL_OPENAI],
    configProviderName: "tencent_token_plan",
    model: "tc-code-latest",
    // 通用 + Hy 两系列合并（1823/130060，2026-08-21 版）+ /models 实测
    // 收录的 minimax-m2.5；kimi-k2.5 官方标注 2026-08-31 下线不收
    catalogModels: [
      "tc-code-latest",
      "deepseek-v4-flash-202605",
      "deepseek-v4-pro-202606",
      "minimax-m2.7",
      "minimax-m2.5",
      "glm-5",
      "glm-5.1",
      "glm-5.2",
      "hy3",
      "hy3-preview",
    ],
  },
  {
    name: "Tencent Token Plan (Intl)",
    site: "intl" as const,
    apiKeyUrl: INTL_TOKEN_PLAN_API_KEY_URL,
    anthropicBaseUrl: INTL_ANTHROPIC,
    anthropicCandidates: [INTL_ANTHROPIC],
    openaiBaseUrl: INTL_OPENAI,
    openaiCandidates: [INTL_OPENAI],
    configProviderName: "tencent_token_plan_intl",
    model: "auto",
    // intl 1300/81315：Auto 调用 ID 是 auto（≠国内个人版 tc-code-latest）
    catalogModels: [
      "auto",
      "glm-5.2",
      "kimi-k2.6",
      "deepseek-v4-pro-202606",
      "deepseek-v4-flash-202605",
      "minimax-m3",
    ],
  },
  {
    name: "Tencent Token Plan Enterprise Pro",
    site: "domestic" as const,
    apiKeyUrl: DOMESTIC_ENTERPRISE_API_KEY_URL,
    anthropicBaseUrl: DOMESTIC_ENTERPRISE_ANTHROPIC,
    // 企业套餐双地域：广州默认 + 新加坡候选（需开通新加坡地域）
    anthropicCandidates: [
      DOMESTIC_ENTERPRISE_ANTHROPIC,
      DOMESTIC_SINGAPORE_ANTHROPIC,
    ],
    openaiBaseUrl: DOMESTIC_ENTERPRISE_OPENAI,
    openaiCandidates: [DOMESTIC_ENTERPRISE_OPENAI, DOMESTIC_SINGAPORE_OPENAI],
    configProviderName: "tencent_token_plan_enterprise_pro",
    model: "auto",
    // 1823/130659 广州地域（2026-08-25 版）；kimi-k2.5 官方标注
    // 2026-08-31 下线不收；minimax-m2.5 型号列表已除名但真 Key 实测可用
    catalogModels: [
      "auto",
      "glm-5.3",
      "glm-5.2",
      "glm-5",
      "glm-5.1",
      "glm-5-turbo",
      "kimi-k2.7-code",
      "kimi-k2.7-code-highspeed",
      "kimi-k2.6",
      "minimax-m2.7",
      "minimax-m3",
      "minimax-m2.5",
      "deepseek-v4-flash",
      "deepseek-v4-pro",
      "deepseek-v4-flash-0731",
      "deepseek-v4-pro-0813",
      "deepseek-v4-flash-202605",
      "deepseek-v4-pro-202606",
    ],
  },
  {
    name: "Tencent Token Plan Enterprise Pro (Intl)",
    site: "intl" as const,
    apiKeyUrl: INTL_ENTERPRISE_API_KEY_URL,
    anthropicBaseUrl: INTL_ANTHROPIC,
    // 企业套餐双地域：新加坡默认 + 广州候选（需开通广州地域）
    anthropicCandidates: [INTL_ANTHROPIC, INTL_GUANGZHOU_ANTHROPIC],
    openaiBaseUrl: INTL_OPENAI,
    openaiCandidates: [INTL_OPENAI, INTL_GUANGZHOU_OPENAI],
    configProviderName: "tencent_token_plan_enterprise_pro_intl",
    model: "auto",
    // intl 1300/81489 新加坡地域：广州地域的子集
    catalogModels: [
      "auto",
      "glm-5.3",
      "glm-5.2",
      "minimax-m3",
      "kimi-k2.7-code",
      "kimi-k2.7-code-highspeed",
      "deepseek-v4-flash",
      "deepseek-v4-pro",
      "deepseek-v4-flash-0731",
      "deepseek-v4-pro-0813",
      "deepseek-v4-flash-202605",
      "deepseek-v4-pro-202606",
    ],
  },
  {
    name: "Tencent Token Plan Enterprise Lite",
    site: "domestic" as const,
    apiKeyUrl: DOMESTIC_ENTERPRISE_API_KEY_URL,
    anthropicBaseUrl: DOMESTIC_ENTERPRISE_ANTHROPIC,
    anthropicCandidates: [
      DOMESTIC_ENTERPRISE_ANTHROPIC,
      DOMESTIC_SINGAPORE_ANTHROPIC,
    ],
    openaiBaseUrl: DOMESTIC_ENTERPRISE_OPENAI,
    openaiCandidates: [DOMESTIC_ENTERPRISE_OPENAI, DOMESTIC_SINGAPORE_OPENAI],
    configProviderName: "tencent_token_plan_enterprise_lite",
    model: "auto",
    catalogModels: ["auto"],
  },
  {
    name: "Tencent Token Plan Enterprise Lite (Intl)",
    site: "intl" as const,
    apiKeyUrl: INTL_ENTERPRISE_API_KEY_URL,
    anthropicBaseUrl: INTL_ANTHROPIC,
    anthropicCandidates: [INTL_ANTHROPIC, INTL_GUANGZHOU_ANTHROPIC],
    openaiBaseUrl: INTL_OPENAI,
    openaiCandidates: [INTL_OPENAI, INTL_GUANGZHOU_OPENAI],
    configProviderName: "tencent_token_plan_enterprise_lite_intl",
    model: "auto",
    catalogModels: ["auto"],
  },
] as const;

describe("Tencent Token Plan provider presets", () => {
  for (const product of products) {
    it(`uses its documented Anthropic endpoint for ${product.name} in Claude`, () => {
      const preset = providerPresets.find((item) => item.name === product.name);
      expect(preset).toBeDefined();
      const env = (
        preset!.settingsConfig as {
          env: Record<string, string>;
        }
      ).env;

      expect(preset).toMatchObject({
        websiteUrl:
          product.site === "domestic" ? DOMESTIC_WEBSITE_URL : INTL_WEBSITE_URL,
        apiKeyUrl: product.apiKeyUrl,
        category: "cn_official",
        endpointCandidates: product.anthropicCandidates,
        icon: "tencent",
      });
      expect(env).toMatchObject({
        ANTHROPIC_BASE_URL: product.anthropicBaseUrl,
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: product.model,
        ANTHROPIC_DEFAULT_HAIKU_MODEL: product.model,
        ANTHROPIC_DEFAULT_SONNET_MODEL: product.model,
        ANTHROPIC_DEFAULT_OPUS_MODEL: product.model,
      });
    });

    it(`uses its documented Anthropic endpoint for ${product.name} in Claude Desktop`, () => {
      const preset = claudeDesktopProviderPresets.find(
        (item) => item.name === product.name,
      );

      expect(preset).toBeDefined();
      expect(preset).toMatchObject({
        websiteUrl:
          product.site === "domestic" ? DOMESTIC_WEBSITE_URL : INTL_WEBSITE_URL,
        apiKeyUrl: product.apiKeyUrl,
        category: "cn_official",
        baseUrl: product.anthropicBaseUrl,
        mode: "proxy",
        apiFormat: "anthropic",
        endpointCandidates: product.anthropicCandidates,
        icon: "tencent",
      });
      expect(preset?.modelRoutes).toEqual([
        expect.objectContaining({
          routeId: "claude-sonnet-5",
          upstreamModel: product.model,
        }),
      ]);
    });

    it(`uses Chat Completions through local routing for ${product.name} in Codex`, () => {
      const preset = codexProviderPresets.find(
        (item) => item.name === product.name,
      );

      expect(preset).toBeDefined();
      expect(preset).toMatchObject({
        websiteUrl:
          product.site === "domestic" ? DOMESTIC_WEBSITE_URL : INTL_WEBSITE_URL,
        apiKeyUrl: product.apiKeyUrl,
        category: "cn_official",
        apiFormat: "openai_chat",
        endpointCandidates: product.openaiCandidates,
        auth: { OPENAI_API_KEY: "" },
        icon: "tencent",
      });
      expect(extractCodexBaseUrl(preset?.config)).toBe(product.openaiBaseUrl);
      expect(extractCodexModelName(preset?.config)).toBe(product.model);
      expect(extractCodexWireApi(preset?.config)).toBe("responses");
      expect(preset?.config).toContain(
        `name = "${product.configProviderName}"`,
      );
      expect(preset?.modelCatalog?.map((entry) => entry.model)).toEqual(
        product.catalogModels,
      );
      // thinking 参数与 reasoning_effort 在 /plan 端点真 Key 实测生效/容忍
      // （2026-08-31）；档位值域由各模型 reasoningLevels 限定为实测安全集
      expect(preset?.codexChatReasoning).toEqual({
        supportsThinking: true,
        supportsEffort: true,
        thinkingParam: "thinking",
        effortParam: "reasoning_effort",
        outputFormat: "reasoning_content",
      });
    });

    it(`uses the OpenAI-compatible endpoint for ${product.name} in OpenCode`, () => {
      const preset = opencodeProviderPresets.find(
        (item) => item.name === product.name,
      );

      expect(preset).toBeDefined();
      expect(preset?.websiteUrl).toBe(
        product.site === "domestic" ? DOMESTIC_WEBSITE_URL : INTL_WEBSITE_URL,
      );
      expect(preset?.apiKeyUrl).toBe(product.apiKeyUrl);
      expect(preset?.category).toBe("cn_official");
      expect(preset?.icon).toBe("tencent");
      expect(preset?.settingsConfig.npm).toBe("@ai-sdk/openai-compatible");
      expect(
        (preset?.settingsConfig.options as { baseURL: string }).baseURL,
      ).toBe(product.openaiBaseUrl);
      expect(Object.keys(preset?.settingsConfig.models ?? {})).toEqual(
        product.catalogModels,
      );
    });

    it(`uses the OpenAI-compatible endpoint for ${product.name} in Hermes`, () => {
      const preset = hermesProviderPresets.find(
        (item) => item.name === product.name,
      );

      expect(preset).toBeDefined();
      expect(preset?.websiteUrl).toBe(
        product.site === "domestic" ? DOMESTIC_WEBSITE_URL : INTL_WEBSITE_URL,
      );
      expect(preset?.apiKeyUrl).toBe(product.apiKeyUrl);
      expect(preset?.category).toBe("cn_official");
      expect(preset?.icon).toBe("tencent");
      expect(preset?.settingsConfig.base_url).toBe(product.openaiBaseUrl);
      expect(preset?.settingsConfig.api_mode).toBe("chat_completions");
      expect(
        (preset?.settingsConfig.models ?? []).map((model) => model.id),
      ).toEqual(product.catalogModels);
      expect(preset?.suggestedDefaults?.model).toEqual({
        default: product.model,
        provider: product.configProviderName,
      });
    });

    it(`uses the OpenAI-compatible endpoint for ${product.name} in OpenClaw`, () => {
      const preset = openclawProviderPresets.find(
        (item) => item.name === product.name,
      );

      expect(preset).toBeDefined();
      expect(preset?.websiteUrl).toBe(
        product.site === "domestic" ? DOMESTIC_WEBSITE_URL : INTL_WEBSITE_URL,
      );
      expect(preset?.apiKeyUrl).toBe(product.apiKeyUrl);
      expect(preset?.category).toBe("cn_official");
      expect(preset?.icon).toBe("tencent");
      expect(preset?.settingsConfig.baseUrl).toBe(product.openaiBaseUrl);
      expect(preset?.settingsConfig.api).toBe("openai-completions");
      expect(
        (preset?.settingsConfig.models ?? []).map((model) => model.id),
      ).toEqual(product.catalogModels);
      // 五字段照官方 OpenClaw 接入页（1823/130062、1300/81503）：订阅套餐
      // cost 全零；超出接入页的模型按平台列表补 maxTokens（注释标边界）
      for (const model of preset?.settingsConfig.models ?? []) {
        expect(model.cost).toEqual({
          input: 0,
          output: 0,
          cacheRead: 0,
          cacheWrite: 0,
        });
        expect(model.contextWindow).toBeGreaterThan(0);
        expect(model.maxTokens).toBeGreaterThan(0);
        expect(model.reasoning).toBeDefined();
        expect(model.input).toEqual(["text"]);
      }
    });
  }

  it("keeps post-paid Hunyuan (TokenHub marketplace line) separate from Token Plan products", () => {
    const preset = codexProviderPresets.find(
      (item) => item.name === "Tencent Hunyuan",
    );

    expect(preset).toMatchObject({
      apiFormat: "openai_responses",
      endpointCandidates: [
        "https://tokenhub.tencentmaas.com/v1",
        "https://tokenhub.tencentmaas.cn/v1",
      ],
    });
  });

  it("keeps domestic and intl endpoints isolated from each other", () => {
    // 官方明示不支持跨站调用：两站预设绝不携带对方的 tencentcloudmaas /
    // tencentmaas(含 lkeap) 域族。同站的跨地域候选（国内新加坡
    // tokenhub-intl.tencentmaas.com、国际广州 tokenhub.tencentcloudmaas.com）
    // 属同一站点，不算泄漏——所以按域名族判站点而非按 intl 前缀判
    for (const product of products) {
      const preset = codexProviderPresets.find(
        (item) => item.name === product.name,
      );
      const candidates = preset?.endpointCandidates ?? [];
      const hasIntlSite = candidates.some((url) =>
        url.includes("tencentcloudmaas.com"),
      );
      const hasDomesticSite = candidates.some(
        (url) =>
          url.includes("tencentmaas.com") ||
          url.includes("api.lkeap.cloud.tencent.com"),
      );
      if (product.site === "intl") {
        expect(hasIntlSite).toBe(true);
        expect(hasDomesticSite).toBe(false);
      } else {
        expect(hasDomesticSite).toBe(true);
        expect(hasIntlSite).toBe(false);
      }
    }
  });

  it("keeps enterprise pro and lite separated", () => {
    const proPreset = codexProviderPresets.find(
      (item) => item.name === "Tencent Token Plan Enterprise Pro",
    );
    const litePreset = codexProviderPresets.find(
      (item) => item.name === "Tencent Token Plan Enterprise Lite",
    );

    expect(proPreset).toBeDefined();
    expect(litePreset).toBeDefined();
    expect(proPreset?.modelCatalog?.map((entry) => entry.model)).toContain(
      "glm-5.3",
    );
    expect(litePreset?.modelCatalog?.map((entry) => entry.model)).toEqual([
      "auto",
    ]);
  });

  it("gives intl personal plan auto routing id instead of domestic tc-code-latest", () => {
    const domestic = codexProviderPresets.find(
      (item) => item.name === "Tencent Token Plan",
    );
    const intl = codexProviderPresets.find(
      (item) => item.name === "Tencent Token Plan (Intl)",
    );

    expect(domestic?.modelCatalog?.map((e) => e.model)).toContain(
      "tc-code-latest",
    );
    expect(intl?.modelCatalog?.map((e) => e.model)).not.toContain(
      "tc-code-latest",
    );
    expect(intl?.modelCatalog?.map((e) => e.model)).toContain("auto");
  });

  it("declares real-key-verified reasoning levels per model", () => {
    // 全部真 Key 实测（2026-08-31）：
    // - glm-5.3 始终思考且档位严格枚举 low/high/max（medium/xhigh 会 400）
    // - kimi-k2.7-code(-highspeed) 仅接受 thinking:enabled
    // - minimax-m2.5/m2.7 与国内 auto 关思考被静默忽略 → 不列 none
    // - 其余模型 thinking 开关真实生效 → 两态 none/high
    const epro = codexProviderPresets.find(
      (item) => item.name === "Tencent Token Plan Enterprise Pro",
    );
    const levels = (name: string) =>
      epro?.modelCatalog?.find((m) => m.model === name)?.reasoningLevels;

    expect(levels("glm-5.3")).toEqual(["low", "high", "max"]);
    expect(levels("kimi-k2.7-code")).toEqual(["high"]);
    expect(levels("kimi-k2.7-code-highspeed")).toEqual(["high"]);
    expect(levels("minimax-m2.7")).toEqual(["high"]);
    expect(levels("minimax-m2.5")).toEqual(["high"]);
    expect(levels("auto")).toEqual(["high"]); // 国内 auto 忽略关思考
    expect(levels("glm-5.2")).toEqual(["none", "high"]);
    expect(levels("kimi-k2.6")).toEqual(["none", "high"]);
    expect(levels("minimax-m3")).toEqual(["none", "high"]);
    expect(levels("deepseek-v4-pro-202606")).toEqual(["none", "high"]);

    // 国际站 auto 尊重关思考（与国内 auto 行为不同）
    const intlEpro = codexProviderPresets.find(
      (item) => item.name === "Tencent Token Plan Enterprise Pro (Intl)",
    );
    expect(
      intlEpro?.modelCatalog?.find((m) => m.model === "auto")?.reasoningLevels,
    ).toEqual(["none", "high"]);

    // 个人版：tc-code-latest 两态；minimax 双款关不掉
    const personal = codexProviderPresets.find(
      (item) => item.name === "Tencent Token Plan",
    );
    expect(
      personal?.modelCatalog?.find((m) => m.model === "tc-code-latest")
        ?.reasoningLevels,
    ).toEqual(["none", "high"]);
    expect(
      personal?.modelCatalog?.find((m) => m.model === "minimax-m2.7")
        ?.reasoningLevels,
    ).toEqual(["high"]);
  });

  it("overrides modelsUrl only on the domestic personal plan preset", () => {
    // /plan/v3/models 仅国内个人版端点可用（真 Key 实测 2026-08-31），
    // 企业/国际端点 /models 404，不覆写
    const personal = providerPresets.find(
      (item) => item.name === "Tencent Token Plan",
    );
    expect(personal?.modelsUrl).toBe(
      "https://api.lkeap.cloud.tencent.com/plan/v3/models",
    );
    for (const other of [
      "Tencent Token Plan (Intl)",
      "Tencent Token Plan Enterprise Pro",
      "Tencent Token Plan Enterprise Pro (Intl)",
      "Tencent Token Plan Enterprise Lite",
      "Tencent Token Plan Enterprise Lite (Intl)",
    ]) {
      const preset = providerPresets.find((item) => item.name === other);
      expect(preset?.modelsUrl).toBeUndefined();
    }
  });

  it("declares the official 196608 context window on every Auto routing row", () => {
    // Auto 是路由别名，平台模型列表无其窗口；唯一官方口径=OpenClaw 接入页
    //（1823/130062、1300/81503）的 196608。不声明则后端回落 config.toml
    // 默认 128K，六个预设的默认模型全部窗口折半
    for (const product of products) {
      const preset = codexProviderPresets.find(
        (item) => item.name === product.name,
      );
      const row = preset?.modelCatalog?.find(
        (entry) => entry.model === product.model,
      );
      expect(row, `${product.name} default model row`).toBeDefined();
      expect(row?.contextWindow).toBe(196608);
    }
  });

  it("keeps glm-5.3 off the max-effort default", () => {
    // glm-5.3 严格枚举 low/high/max 不含模板默认 medium → 回落最高档 max
    //（最慢最耗额度）；显式 high 兜底（真 Key 实测无厂商默认值证据）
    for (const name of [
      "Tencent Token Plan Enterprise Pro",
      "Tencent Token Plan Enterprise Pro (Intl)",
    ]) {
      const preset = codexProviderPresets.find((item) => item.name === name);
      const row = preset?.modelCatalog?.find(
        (entry) => entry.model === "glm-5.3",
      );
      expect(row?.reasoningLevels).toEqual(["low", "high", "max"]);
      expect(row?.defaultReasoningLevel).toBe("high");
    }
  });

  it("rebases OpenClaw defaults to the submitted provider key", () => {
    const preset = openclawProviderPresets.find(
      (item) => item.name === "Tencent Token Plan",
    );
    expect(preset?.suggestedDefaults).toBeDefined();

    const rebased = rebaseOpenClawSuggestedDefaults(
      preset!.suggestedDefaults!,
      "my-tencent",
    );
    expect(rebased.model?.primary).toBe("my-tencent/tc-code-latest");
    expect(rebased.modelCatalog).toHaveProperty("my-tencent/tc-code-latest");
  });
});
