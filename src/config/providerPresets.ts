/**
 * 预设供应商配置模板
 */
import { ProviderCategory } from "../types";

export interface TemplateValueConfig {
  label: string;
  placeholder: string;
  defaultValue?: string;
  editorValue: string;
}

export interface ProviderPreset {
  name: string;
  websiteUrl: string;
  // 新增：第三方/聚合等可单独配置获取 API Key 的链接
  apiKeyUrl?: string;
  settingsConfig: object;
  isOfficial?: boolean; // 标识是否为官方预设
  category?: ProviderCategory; // 新增：分类
  // 新增：模板变量定义，用于动态替换配置中的值
  templateValues?: Record<string, TemplateValueConfig>; // editorValue 存储编辑器中的实时输入值
  // 新增：请求地址候选列表（用于地址管理/测速）
  endpointCandidates?: string[];
}

export const providerPresets: ProviderPreset[] = [
  {
    name: "Claude Official",
    websiteUrl: "https://www.anthropic.com/claude-code",
    settingsConfig: {
      env: {},
    },
    isOfficial: true, // 明确标识为官方预设
    category: "official",
  },
  {
    name: "DeepSeek",
    websiteUrl: "https://platform.deepseek.com",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.deepseek.com/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "DeepSeek-V3.2-Exp",
        ANTHROPIC_SMALL_FAST_MODEL: "DeepSeek-V3.2-Exp",
      },
    },
    category: "cn_official",
  },
  {
    name: "Zhipu GLM",
    websiteUrl: "https://open.bigmodel.cn",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://open.bigmodel.cn/api/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        // 兼容旧键名，保持前端读取一致
        ANTHROPIC_MODEL: "GLM-4.6",
        ANTHROPIC_SMALL_FAST_MODEL: "glm-4.5-air",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "glm-4.5-air",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-4.6",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "glm-4.6",
      },
    },
    category: "cn_official",
  },
  {
    name: "Qwen Coder",
    websiteUrl: "https://bailian.console.aliyun.com",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://dashscope.aliyuncs.com/api/v2/apps/claude-code-proxy",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "qwen3-max",
        ANTHROPIC_SMALL_FAST_MODEL: "qwen3-max",
      },
    },
    category: "cn_official",
  },
  {
    name: "Kimi k2",
    websiteUrl: "https://platform.moonshot.cn/console",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.moonshot.cn/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "kimi-k2-turbo-preview",
        ANTHROPIC_SMALL_FAST_MODEL: "kimi-k2-turbo-preview",
      },
    },
    category: "cn_official",
  },
  {
    name: "ModelScope",
    websiteUrl: "https://modelscope.cn",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api-inference.modelscope.cn",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "ZhipuAI/GLM-4.6",
        ANTHROPIC_SMALL_FAST_MODEL: "ZhipuAI/GLM-4.6",
      },
    },
    category: "aggregator",
  },
  {
    name: "KAT-Coder",
    websiteUrl: "https://console.streamlake.ai/wanqing/",
    apiKeyUrl: "https://console.streamlake.ai/console/wanqing/api-key",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://vanchin.streamlake.ai/api/gateway/v1/endpoints/${ENDPOINT_ID}/claude-code-proxy",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "KAT-Coder",
        ANTHROPIC_SMALL_FAST_MODEL: "KAT-Coder",
      },
    },
    category: "cn_official",
    templateValues: {
      ENDPOINT_ID: {
        label: "Vanchin Endpoint ID",
        placeholder: "ep-xxx-xxx",
        defaultValue: "",
        editorValue: "",
      },
    },
  },
  {
    name: "Longcat",
    websiteUrl: "https://longcat.chat/platform",
    apiKeyUrl: "https://longcat.chat/platform/api_keys",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.longcat.chat/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "LongCat-Flash-Chat",
        ANTHROPIC_SMALL_FAST_MODEL: "LongCat-Flash-Chat",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "LongCat-Flash-Chat",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "LongCat-Flash-Chat",
        CLAUDE_CODE_MAX_OUTPUT_TOKENS: "6000",
        CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: 1,
      },
    },
    category: "cn_official",
  },
  {
    name: "PackyCode",
    websiteUrl: "https://www.packycode.com",
    apiKeyUrl: "https://www.packycode.com/?aff=rlo54mgz",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.packycode.com",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    // 请求地址候选（用于地址管理/测速）
    endpointCandidates: [
      "https://api.packycode.com",
      "https://api-hk-cn2.packycode.com",
      "https://api-hk-g.packycode.com",
      "https://api-us-cn2.packycode.com",
      "https://api-cf-pro.packycode.com",
    ],
    category: "third_party",
  },
];
