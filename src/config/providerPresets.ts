/**
 * 预设供应商配置模板
 */
export interface ProviderPreset {
  name: string;
  websiteUrl: string;
  settingsConfig: object;
  isOfficial?: boolean; // 标识是否为官方预设
}

export const providerPresets: ProviderPreset[] = [
  {
    name: "Claude官方登录",
    websiteUrl: "https://www.anthropic.com/claude-code",
    settingsConfig: {
      env: {},
    },
    isOfficial: true, // 明确标识为官方预设
  },
  {
    name: "DeepSeek v3.1",
    websiteUrl: "https://platform.deepseek.com",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.deepseek.com/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "deepseek-chat",
        ANTHROPIC_SMALL_FAST_MODEL: "deepseek-chat",
      },
    },
  },
  {
    name: "智谱GLM",
    websiteUrl: "https://open.bigmodel.cn",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://open.bigmodel.cn/api/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
  },
  {
    name: "千问Qwen-Coder",
    websiteUrl: "https://bailian.console.aliyun.com",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://dashscope.aliyuncs.com/api/v2/apps/claude-code-proxy",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
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
  },
  {
    name: "魔搭",
    websiteUrl: "https://modelscope.cn",
    settingsConfig: {
      env: {
        ANTHROPIC_AUTH_TOKEN: "ms-your-api-key",
        ANTHROPIC_BASE_URL: "https://api-inference.modelscope.cn",
        ANTHROPIC_MODEL: "ZhipuAI/GLM-4.5",
        ANTHROPIC_SMALL_FAST_MODEL: "ZhipuAI/GLM-4.5",
      },
    },
  },
  {
    name: "PackyCode",
    websiteUrl: "https://www.packycode.com",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.packycode.com",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
  },
];
