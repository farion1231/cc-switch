import { describe, expect, it } from "vitest";
import { cursorProviderPresets } from "@/config/cursorProviderPresets";

const CURSOR_BASE_PRESET_NAMES = [
  "Cursor Official",
  "Anthropic",
  "OpenAI",
  "OpenAI Compatible",
];

// 独立维护的第三方 OpenAI 兼容供应商清单（name -> { baseUrl, model }）。
// 与 codexProviderPresets 无数据联动；Cursor 需要显式 API Key，故 OAuth 类
// （xAI OAuth）和占位符类（Azure OpenAI）不在清单中。
const EXPECTED_THIRD_PARTY_PRESETS = new Map<
  string,
  { baseUrl: string; model: string }
>([
  ["Kimi", { baseUrl: "https://api.moonshot.cn/v1", model: "kimi-k2.7-code" }],
  [
    "Kimi For Coding",
    { baseUrl: "https://api.kimi.com/coding/v1", model: "kimi-for-coding" },
  ],
  ["PackyCode", { baseUrl: "https://www.packyapi.com/v1", model: "gpt-5.5" }],
  ["ZetaAPI", { baseUrl: "https://api.zetaapi.ai/v1", model: "gpt-5.5" }],
  ["APINebula", { baseUrl: "https://apinebula.com/v1", model: "gpt-5.5" }],
  [
    "AICodeMirror",
    {
      baseUrl: "https://api.aicodemirror.com/api/codex/backend-api/codex",
      model: "gpt-5.5",
    },
  ],
  ["PatewayAI", { baseUrl: "https://api.pateway.ai/v1", model: "gpt-5.5" }],
  ["FennoAI", { baseUrl: "https://api.fenno.ai", model: "gpt-5.5" }],
  ["RunAPI", { baseUrl: "https://runapi.co/v1", model: "gpt-5.5" }],
  ["Unity2.ai", { baseUrl: "https://api.unity2.ai", model: "gpt-5.5" }],
  [
    "Shengsuanyun",
    {
      baseUrl: "https://router.shengsuanyun.com/api/v1",
      model: "openai/gpt-5.5",
    },
  ],
  ["AIGoCode", { baseUrl: "https://api.aigocode.com", model: "gpt-5.5" }],
  ["SubRouter", { baseUrl: "https://subrouter.ai/v1", model: "gpt-5.5" }],
  ["APIKEY.FUN", { baseUrl: "https://api.apikey.fun/v1", model: "gpt-5.5" }],
  ["Code0", { baseUrl: "https://code0.ai/v1", model: "gpt-5.5" }],
  [
    "TeamoRouter",
    { baseUrl: "https://api.teamorouter.com/v1", model: "gpt-5.5" },
  ],
  ["ClaudeCN", { baseUrl: "https://claudecn.top/v1", model: "gpt-5.5" }],
  [
    "火山Agentplan",
    {
      baseUrl: "https://ark.cn-beijing.volces.com/api/coding/v3",
      model: "ark-code-latest",
    },
  ],
  [
    "BytePlus",
    {
      baseUrl: "https://ark.ap-southeast.bytepluses.com/api/coding/v3",
      model: "ark-code-latest",
    },
  ],
  [
    "DouBaoSeed",
    {
      baseUrl: "https://ark.cn-beijing.volces.com/api/v3",
      model: "doubao-seed-2-1-pro-260628",
    },
  ],
  [
    "SiliconFlow",
    {
      baseUrl: "https://api.siliconflow.cn/v1",
      model: "Pro/MiniMaxAI/MiniMax-M2.7",
    },
  ],
  [
    "SiliconFlow en",
    {
      baseUrl: "https://api.siliconflow.com/v1",
      model: "MiniMaxAI/MiniMax-M2.7",
    },
  ],
  ["NekoCode", { baseUrl: "https://nekocode.ai/v1", model: "gpt-5.5" }],
  [
    "AtlasCloud",
    { baseUrl: "https://api.atlascloud.ai/v1", model: "zai-org/glm-5.1" },
  ],
  ["Compshare", { baseUrl: "https://api.modelverse.cn/v1", model: "gpt-5.5" }],
  [
    "Compshare Coding Plan",
    { baseUrl: "https://cp.compshare.cn/v1", model: "gpt-5.5" },
  ],
  ["CCSub", { baseUrl: "https://www.ccsub.net/v1", model: "gpt-5.5" }],
  [
    "SSSAiCode",
    { baseUrl: "https://node-hk.sssaicodeapi.com/api/v1", model: "gpt-5.5" },
  ],
  ["Micu", { baseUrl: "https://www.micuapi.ai/v1", model: "gpt-5.5" }],
  ["RightCode", { baseUrl: "https://right.codes/codex/v1", model: "gpt-5.5" }],
  ["ETok.ai", { baseUrl: "https://api.etok.ai/v1", model: "gpt-5.5" }],
  ["Cubence", { baseUrl: "https://api.cubence.com/v1", model: "gpt-5.5" }],
  [
    "CrazyRouter",
    { baseUrl: "https://cn.crazyrouter.com/v1", model: "gpt-5.5" },
  ],
  ["DMXAPI", { baseUrl: "https://www.dmxapi.cn/v1", model: "gpt-5.5" }],
  [
    "Qiniu",
    { baseUrl: "https://api.qnaigc.com/bypass/openai/v1", model: "gpt-5.5" },
  ],
  [
    "SudoCode.chat",
    { baseUrl: "https://api.sudocode.chat/v1", model: "gpt-5.6-sol" },
  ],
  ["SudoCode.us", { baseUrl: "https://sudocode.us/v1", model: "gpt-5.5" }],
  ["Amux", { baseUrl: "https://api.amux.ai/v1", model: "gpt-5.5" }],
  [
    "DeepSeek",
    { baseUrl: "https://api.deepseek.com", model: "deepseek-v4-flash" },
  ],
  [
    "Zhipu GLM",
    {
      baseUrl: "https://open.bigmodel.cn/api/coding/paas/v4",
      model: "glm-5.2",
    },
  ],
  [
    "Zhipu GLM en",
    { baseUrl: "https://api.z.ai/api/coding/paas/v4", model: "glm-5.2" },
  ],
  [
    "Baidu Qianfan Coding Plan",
    {
      baseUrl: "https://qianfan.baidubce.com/v2/coding",
      model: "qianfan-code-latest",
    },
  ],
  [
    "Bailian",
    {
      baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
      model: "qwen3-coder-plus",
    },
  ],
  [
    "StepFun",
    {
      baseUrl: "https://api.stepfun.com/step_plan/v1",
      model: "step-3.7-flash",
    },
  ],
  [
    "StepFun en",
    { baseUrl: "https://api.stepfun.ai/step_plan/v1", model: "step-3.7-flash" },
  ],
  [
    "ModelScope",
    {
      baseUrl: "https://api-inference.modelscope.cn/v1",
      model: "ZhipuAI/GLM-5.1",
    },
  ],
  [
    "Longcat",
    { baseUrl: "https://api.longcat.chat/openai/v1", model: "LongCat-2.0" },
  ],
  ["MiniMax", { baseUrl: "https://api.minimaxi.com/v1", model: "MiniMax-M3" }],
  ["MiniMax en", { baseUrl: "https://api.minimax.io/v1", model: "MiniMax-M3" }],
  [
    "BaiLing",
    { baseUrl: "https://api.tbox.cn/api/llm/v1", model: "Ling-2.6-1T" },
  ],
  [
    "Xiaomi MiMo",
    { baseUrl: "https://api.xiaomimimo.com/v1", model: "mimo-v2.5-pro" },
  ],
  [
    "Xiaomi MiMo Token Plan (China)",
    {
      baseUrl: "https://token-plan-cn.xiaomimimo.com/v1",
      model: "mimo-v2.5-pro",
    },
  ],
  [
    "Novita AI",
    { baseUrl: "https://api.novita.ai/openai/v1", model: "zai-org/glm-5.1" },
  ],
  ["xAI (Grok)", { baseUrl: "https://api.x.ai/v1", model: "grok-4.5" }],
  [
    "Nvidia",
    {
      baseUrl: "https://integrate.api.nvidia.com/v1",
      model: "moonshotai/kimi-k2.5",
    },
  ],
  [
    "OpenCode Go",
    { baseUrl: "https://opencode.ai/zen/go/v1", model: "glm-5.2" },
  ],
  ["AiHubMix", { baseUrl: "https://aihubmix.com/v1", model: "gpt-5.5" }],
  [
    "CherryIN",
    { baseUrl: "https://open.cherryin.net/v1", model: "openai/gpt-5.5" },
  ],
  [
    "RelaxyCode",
    { baseUrl: "https://www.relaxycode.com/v1", model: "gpt-5.5" },
  ],
  ["E-FlowCode", { baseUrl: "https://e-flowcode.cc/v1", model: "gpt-5.5" }],
  ["PIPELLM", { baseUrl: "https://cc-api.pipellm.ai/v1", model: "gpt-5.5" }],
  ["OpenRouter", { baseUrl: "https://openrouter.ai/api/v1", model: "gpt-5.5" }],
  [
    "TheRouter",
    { baseUrl: "https://api.therouter.ai/v1", model: "openai/gpt-5.3-codex" },
  ],
]);

const UNSUPPORTED_PRESET_NAMES = [
  "OpenAI Official", // 无 baseUrl，Cursor 侧无意义
  "Azure OpenAI", // 占位 baseUrl + api-version query 参数，需要单独处理
  "xAI (Grok) OAuth", // requiresOAuth，Cursor 公网路由需要显式 API Key
];

describe("Cursor provider presets (independently maintained)", () => {
  it("keeps the 4 built-in Cursor presets", () => {
    for (const name of CURSOR_BASE_PRESET_NAMES) {
      expect(
        cursorProviderPresets.some((p) => p.name === name),
        `${name} should still exist`,
      ).toBe(true);
    }
  });

  it("includes every maintained OpenAI-compatible third-party preset", () => {
    const cursorNames = new Set(cursorProviderPresets.map((p) => p.name));

    for (const name of EXPECTED_THIRD_PARTY_PRESETS.keys()) {
      expect(cursorNames.has(name), `preset "${name}" should exist`).toBe(true);
    }
  });

  it("maps baseUrl and model correctly for every third-party preset", () => {
    for (const [name, expected] of EXPECTED_THIRD_PARTY_PRESETS) {
      const preset = cursorProviderPresets.find((p) => p.name === name);
      expect(preset, `${name} preset`).toBeDefined();
      expect(preset?.settingsConfig.baseUrl, `${name} baseUrl`).toBe(
        expected.baseUrl,
      );
      expect(preset?.settingsConfig.model, `${name} model`).toBe(
        expected.model,
      );
      expect(preset?.settingsConfig.apiKey, `${name} apiKey`).toBe("");
    }
  });

  it("excludes OAuth / placeholder presets", () => {
    const cursorNames = new Set(cursorProviderPresets.map((p) => p.name));
    for (const name of UNSUPPORTED_PRESET_NAMES) {
      expect(cursorNames.has(name), `${name} should be excluded`).toBe(false);
    }
  });

  it("keeps the preset list free of unexpected duplicates", () => {
    const names = cursorProviderPresets.map((p) => p.name);
    const duplicates = names.filter(
      (name, index) => names.indexOf(name) !== index,
    );
    expect(duplicates).toEqual([]);
  });

  it("carries a modelCatalog for cataloged providers", () => {
    const catalogedNames = [
      "Kimi",
      "DeepSeek",
      "StepFun",
      "MiniMax",
      "Xiaomi MiMo",
      "SiliconFlow",
      "DouBaoSeed",
      "Bailian",
    ];
    for (const name of catalogedNames) {
      const preset = cursorProviderPresets.find((p) => p.name === name);
      expect(preset, `${name} preset`).toBeDefined();
      expect(
        preset?.modelCatalog?.length,
        `${name} modelCatalog`,
      ).toBeGreaterThan(0);
      expect(
        preset?.modelCatalog?.[0]?.model,
        `${name} first catalog model`,
      ).toBe(preset?.settingsConfig.model);
    }
  });
});
