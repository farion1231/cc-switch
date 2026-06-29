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
  [
    "火山Agentplan",
    {
      baseUrl: "https://ark.cn-beijing.volces.com/api/coding/v3",
      contextWindows: { "ark-code-latest": 256000 },
    },
  ],
  [
    "BytePlus",
    {
      baseUrl: "https://ark.ap-southeast.bytepluses.com/api/coding/v3",
      contextWindows: { "ark-code-latest": 256000 },
    },
  ],
  [
    "DeepSeek",
    {
      baseUrl: "https://api.deepseek.com",
      contextWindows: {
        "deepseek-v4-flash": 1000000,
        "deepseek-v4-pro": 1000000,
      },
    },
  ],
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
    "Kimi",
    {
      baseUrl: "https://api.moonshot.cn/v1",
      contextWindows: { "kimi-k2.7-code": 262144 },
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
      contextWindows: { "ZhipuAI/GLM-5.1": 200000 },
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
      contextWindows: { "Pro/MiniMaxAI/MiniMax-M2.7": 200000 },
    },
  ],
  [
    "SiliconFlow en",
    {
      baseUrl: "https://api.siliconflow.com/v1",
      contextWindows: { "MiniMaxAI/MiniMax-M2.7": 200000 },
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
    const nativeResponsesPresets = [
      "DouBaoSeed",
      "Bailian",
      "Longcat",
      "MiniMax",
      "MiniMax en",
      "Xiaomi MiMo",
      "Xiaomi MiMo Token Plan (China)",
    ];

    for (const name of nativeResponsesPresets) {
      const preset = codexProviderPresets.find((item) => item.name === name);

      expect(preset, `${name} preset`).toBeDefined();
      expect(preset?.apiFormat).toBe("openai_responses");
      // 原生 Responses 预设必须不带 modelCatalog，否则“本地路由映射”开关会默认勾选
      expect(preset?.modelCatalog ?? []).toHaveLength(0);
      expect(preset?.codexChatReasoning).toBeUndefined();
    }
  });
});
