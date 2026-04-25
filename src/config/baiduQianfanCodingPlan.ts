export const BAIDU_QIANFAN_CODING_PLAN_MODELS = {
  "qianfan-code-latest": { name: "Qianfan Code Latest" },
  "ernie-4.5-turbo-20260402": { name: "ERNIE 4.5 Turbo" },
  "kimi-k2.5": { name: "Kimi K2.5" },
  "deepseek-v3.2": { name: "DeepSeek V3.2" },
  "glm-5": { name: "GLM 5" },
  "minimax-m2.5": { name: "MiniMax M2.5" },
} as const;

export const BAIDU_QIANFAN_CODING_PLAN = {
  name: "Baidu Qianfan Coding Plan",
  websiteUrl: "https://cloud.baidu.com/product/qianfan_modelbuilder",
  apiKeyUrl:
    "https://console.bce.baidu.com/qianfan/ais/console/applicationConsole/application",
  anthropicBaseUrl: "https://qianfan.baidubce.com/anthropic/coding",
  openaiBaseUrl: "https://qianfan.baidubce.com/v2/coding",
  defaultModel: "qianfan-code-latest",
  category: "cn_official",
  icon: "baidu",
  iconColor: "#2932E1",
} as const;
