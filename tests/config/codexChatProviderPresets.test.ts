import { describe, expect, it } from "vitest";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import {
  extractCodexBaseUrl,
  extractCodexModelName,
  extractCodexWireApi,
} from "@/utils/providerConfigUtils";

const expectedChatPresets = new Map<
  string,
  { baseUrl: string; contextWindows: Record<string, number> }
>([
  // 火山 Agent Plan / Coding Plan 与 BytePlus 国际站（coding/v3）均已切
  // 原生 Responses，见下方 native 清单
  [
    "Zhipu GLM",
    {
      baseUrl: "https://open.bigmodel.cn/api/coding/paas/v4",
      contextWindows: { "glm-5.2": 200000 },
    },
  ],
  [
    "Zhipu GLM en",
    {
      baseUrl: "https://api.z.ai/api/coding/paas/v4",
      contextWindows: { "glm-5.2": 200000 },
    },
  ],
  [
    "Baidu Qianfan Coding Plan",
    {
      baseUrl: "https://qianfan.baidubce.com/v2/coding",
      contextWindows: { "qianfan-code-latest": 131072 },
    },
  ],
  [
    "Baidu Qianfan Token Plan",
    {
      baseUrl: "https://qianfan.baidubce.com/v2/tokenplan/personal",
      contextWindows: {
        "deepseek-v4-pro": 1048576,
        "deepseek-v4-flash": 1048576,
        "deepseek-v4-flash-0731": 1048576,
        "glm-5.2": 1048576,
        "glm-5.1": 198000,
        "kimi-k2.6": 262144,
      },
    },
  ],
  [
    "Kimi",
    {
      baseUrl: "https://api.moonshot.cn/v1",
      contextWindows: { "kimi-k2.7-code": 262144, "kimi-k3": 1048576 },
    },
  ],
  [
    "Kimi For Coding",
    {
      baseUrl: "https://api.kimi.com/coding/v1",
      contextWindows: {
        "kimi-for-coding": 262144,
        "kimi-for-coding-highspeed": 262144,
        k3: 1048576,
        "k3-256k": 262144,
      },
    },
  ],
  [
    "StepFun",
    {
      baseUrl: "https://api.stepfun.com/step_plan/v1",
      contextWindows: {
        "step-3.7-flash": 262144,
        "step-3.5-flash-2603": 262144,
        "step-3.5-flash": 262144,
      },
    },
  ],
  [
    "StepFun en",
    {
      baseUrl: "https://api.stepfun.ai/step_plan/v1",
      contextWindows: {
        "step-3.7-flash": 262144,
        "step-3.5-flash-2603": 262144,
        "step-3.5-flash": 262144,
      },
    },
  ],
  [
    "ModelScope",
    {
      baseUrl: "https://api-inference.modelscope.cn/v1",
      contextWindows: { "ZhipuAI/GLM-5.2": 200000 },
    },
  ],
  [
    "BaiLing",
    {
      baseUrl: "https://api.tbox.cn/api/llm/v1",
      contextWindows: { "Ling-2.6-1T": 262144 },
    },
  ],
  [
    "SiliconFlow",
    {
      baseUrl: "https://api.siliconflow.cn/v1",
      contextWindows: { "Pro/MiniMaxAI/MiniMax-M2.5": 196608 },
    },
  ],
  [
    "SiliconFlow en",
    {
      baseUrl: "https://api.siliconflow.com/v1",
      contextWindows: { "MiniMaxAI/MiniMax-M3": 1048576 },
    },
  ],
  [
    "Novita AI",
    {
      baseUrl: "https://api.novita.ai/openai/v1",
      contextWindows: { "zai-org/glm-5.1": 202800 },
    },
  ],
  [
    "Nvidia",
    {
      baseUrl: "https://integrate.api.nvidia.com/v1",
      contextWindows: { "moonshotai/kimi-k2.5": 262144 },
    },
  ],
]);

describe("Codex Chat provider presets", () => {
  it("enables session-based prompt cache routing for Kimi Coding", () => {
    const preset = codexProviderPresets.find(
      (item) => item.name === "Kimi For Coding",
    );

    expect(preset?.promptCacheRouting).toBe("enabled");
  });

  it("marks migrated Chat Completions presets for local routing", () => {
    for (const [name, expected] of expectedChatPresets) {
      const preset = codexProviderPresets.find((item) => item.name === name);

      expect(preset, `${name} preset`).toBeDefined();
      expect(preset?.apiFormat).toBe("openai_chat");
      expect(extractCodexBaseUrl(preset?.config)).toBe(expected.baseUrl);
      expect(extractCodexWireApi(preset?.config)).toBe("responses");
      expect(preset?.endpointCandidates).toContain(expected.baseUrl);
      expect(preset?.modelCatalog?.length).toBeGreaterThan(0);
      expect(extractCodexModelName(preset?.config)).toBe(
        preset?.modelCatalog?.[0]?.model,
      );
      expect(
        Object.fromEntries(
          (preset?.modelCatalog ?? []).map((model) => [
            model.model,
            model.contextWindow,
          ]),
        ),
      ).toEqual(expected.contextWindows);
    }
  });

  it("uses native Responses API for migrated CN providers without local route mapping", () => {
    const nativeResponsesPresets = new Map<
      string,
      { contextWindows: Record<string, number> }
    >([
      // 官方 Codex 文档确认 Agent Plan /api/plan/v3 与 Coding Plan
      // /api/coding/v3 均支持 Responses API；BytePlus 国际站 coding/v3
      // 同（docs.byteplus.com/en/docs/ModelArk/2556056，2026-08-15 核实）
      ["火山 Agent Plan", { contextWindows: { "ark-code-latest": 256000 } }],
      ["火山 Coding Plan", { contextWindows: { "ark-code-latest": 256000 } }],
      ["BytePlus", { contextWindows: { "ark-code-latest": 256000 } }],
      [
        "DouBaoSeed",
        { contextWindows: { "doubao-seed-2-1-pro-260628": 262144 } },
      ],
      ["Bailian", { contextWindows: { "qwen3-coder-plus": 1048576 } }],
      // 腾讯 TokenHub 官方 Codex 文档确认 hy3 原生 Responses（2026-07-14）
      [
        "Tencent Hunyuan",
        { contextWindows: { hy3: 256000, "hy3-preview": 256000 } },
      ],
      // DeepSeek 官方 Codex 文档确认 deepseek-v4-flash 原生 Responses；
      // catalog 由后端按 deepseek.com host 镜像官方 models.json 生成
      [
        "DeepSeek",
        {
          contextWindows: {
            "deepseek-v4-flash": 1048576,
            "deepseek-v4-pro": 1048576,
          },
        },
      ],
      ["Longcat", { contextWindows: { "LongCat-2.0": 1048576 } }],
      ["MiniMax", { contextWindows: { "MiniMax-M3": 1000000 } }],
      ["MiniMax en", { contextWindows: { "MiniMax-M3": 1000000 } }],
      [
        "Xiaomi MiMo",
        {
          contextWindows: {
            "mimo-v2.5-pro": 1048576,
            "mimo-v2.5": 1048576,
          },
        },
      ],
      [
        "Xiaomi MiMo Token Plan (China)",
        {
          contextWindows: {
            "mimo-v2.5-pro": 1048576,
            "mimo-v2.5": 1048576,
          },
        },
      ],
    ]);

    for (const [name, expected] of nativeResponsesPresets) {
      const preset = codexProviderPresets.find((item) => item.name === name);

      expect(preset, `${name} preset`).toBeDefined();
      expect(preset?.apiFormat).toBe("openai_responses");
      // 原生 Responses 预设现在带 modelCatalog：cc-switch 直连时据此生成
      // ~/.codex 的 model-catalogs.json（shell_command 编辑、不发 freeform
      // apply_patch）。带 catalog 不再强制开“本地路由映射”——前端已按
      // apiFormat 解耦（openai_responses 默认不开接管）。
      expect((preset?.modelCatalog ?? []).length).toBeGreaterThan(0);
      expect(
        Object.fromEntries(
          (preset?.modelCatalog ?? []).map((model) => [
            model.model,
            model.contextWindow,
          ]),
        ),
      ).toEqual(expected.contextWindows);
      // 原生（直连）不走 Chat 转换，因此不需要 codexChatReasoning。
      expect(preset?.codexChatReasoning).toBeUndefined();
    }
  });

  it("ships per-model reasoningLevels for OpenCode Go mirroring models.dev", () => {
    // Zen 网关的合法 effort 档位是逐模型的（models.dev reasoning_options，
    // 2026-08）：统一并集映射会把 Codex 默认的 medium 发给只声明 high|max 的
    // glm-5.2（默认路径），此测试锁住逐模型表，防回退。
    const preset = codexProviderPresets.find((item) => item.name === "OpenCode Go");

    expect(preset, "OpenCode Go preset").toBeDefined();
    expect(preset?.codexChatReasoning?.effortValueMode).toBe("zen");
    expect(
      Object.fromEntries(
        (preset?.modelCatalog ?? []).map((model) => [
          model.model,
          model.reasoningLevels ?? null,
        ]),
      ),
    ).toEqual({
      "glm-5.2": ["high", "max"],
      "glm-5.1": null, // toggle 型，无 effort 声明
      "kimi-k2.7-code": null,
      "deepseek-v4-pro": ["high", "max"],
      "deepseek-v4-flash": ["low", "high", "max"],
      "mimo-v2.5-pro": null,
    });
  });
});
