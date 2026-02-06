import type { ProviderCategory } from "@/types";

export interface QwenProviderPreset {
  name: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: {
    env: {
      OPENAI_API_KEY: string;
      OPENAI_BASE_URL: string;
      OPENAI_MODEL: string;
    };
  };
  category: ProviderCategory;
  isOfficial?: boolean;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  icon: string;
  iconColor?: string;
  endpointCandidates?: string[];
}

export const qwenProviderPresets: QwenProviderPreset[] = [
  {
    name: "Alibaba Cloud (DashScope)",
    websiteUrl: "https://www.alibabacloud.com/product/dashscope",
    apiKeyUrl: "https://dashscope.console.aliyun.com/apiKey",
    settingsConfig: {
      env: {
        OPENAI_API_KEY: "",
        OPENAI_BASE_URL: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        OPENAI_MODEL: "qwen3-coder-plus",
      },
    },
    category: "cn_official",
    icon: "qwen",
    iconColor: "#6B4CFF",
  },
  {
    name: "ModelScope",
    websiteUrl: "https://modelscope.cn",
    apiKeyUrl: "https://modelscope.cn/my/myaccesstoken",
    settingsConfig: {
      env: {
        OPENAI_API_KEY: "",
        OPENAI_BASE_URL: "https://api-inference.modelscope.cn/v1/",
        OPENAI_MODEL: "Qwen/Qwen3-Coder-480B-A35B-Instruct",
      },
    },
    category: "cn_official",
    icon: "modelscope-color",
  },
  {
    name: "OpenRouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    settingsConfig: {
      env: {
        OPENAI_API_KEY: "",
        OPENAI_BASE_URL: "https://openrouter.ai/api/v1",
        OPENAI_MODEL: "qwen/qwen3-coder",
      },
    },
    category: "aggregator",
    icon: "openrouter",
  },
];
