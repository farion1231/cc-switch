import type { ProviderPreset } from "@/config/claudeProviderPresets";
import type { ExtraAuthHeader } from "./types";
import { DEFAULT_FINGERPRINT_REPLACEMENTS } from "./huaweiCodeAgent2Presets";

const CODEAGENT_3_AUTH_HEADERS: ExtraAuthHeader[] = [
  { name: "x-auth-token", tokenSource: "huawei_codeagent_2" },
  { name: "Authorization", tokenSource: "huawei_codeagent_2", valuePrefix: "Bearer " },
  { name: "X-snap-traceid", tokenSource: "uuid" },
  { name: "Agent-Type", value: "AgentCenter" },
  { name: "app-id", value: "com.huawei.devmind.codebot.apibot" },
  { name: "x-innercc-request-kind", value: "chat" },
  { name: "x-error-propagation", value: "true" },
  { name: "Model-Id", tokenSource: "request_model" },
];

export const huaweiCodeAgent3Preset: ProviderPreset = {
  name: "Huawei CodeAgent 3.0",
  websiteUrl: "https://rnd-idea-api.huawei.com",
  category: "cloud_provider",
  providerType: "huawei_codeagent_3",
  appTarget: "claude",
  requiresOAuth: true,
  settingsConfig: {
    env: {
      ANTHROPIC_BASE_URL:
        "https://snapengine.cida.cce.prod-szv-g.dragon.tools.huawei.com/api/v2",
      ANTHROPIC_API_KEY: "",
      ANTHROPIC_MODEL: "maas-glm-5.1-zhipu",
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "maas-glm-5.1-zhipu",
      ANTHROPIC_DEFAULT_SONNET_MODEL: "maas-glm-5.1-zhipu",
      ANTHROPIC_DEFAULT_OPUS_MODEL: "maas-glm-5.1-zhipu",
      CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: "1",
      NO_PROXY: "127.0.0.1,localhost",
    },
    api_format: "anthropic",
  },
  fingerprintReplacements: DEFAULT_FINGERPRINT_REPLACEMENTS,
  theme: {
    icon: "generic",
    backgroundColor: "#e63946",
    textColor: "#ffffff",
  },
};

export { CODEAGENT_3_AUTH_HEADERS };
