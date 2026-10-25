import type { ProviderPreset } from "@/config/claudeProviderPresets";
import type { ExtraAuthHeader } from "./types";

interface FingerprintReplacement {
  from: string;
  to: string;
}

const DEFAULT_FINGERPRINT_REPLACEMENTS: FingerprintReplacement[] = [
  { from: "Claude Code", to: "CodeAgent" },
  { from: "Claude Opus", to: "CodeAgent Opus" },
  { from: "Claude Sonnet", to: "CodeAgent Sonnet" },
  { from: "Claude Haiku", to: "CodeAgent Haiku" },
  { from: "Co-Authored-By: Claude", to: "Co-Authored-By: CodeAgent" },
  { from: "Claude", to: "CodeAgent" },
  { from: "noreply@anthropic.com", to: "noreply@aiprovider.io" },
  { from: "Anthropic", to: "AI Provider" },
  { from: "anthropic.com", to: "aiprovider.io" },
  { from: "claude.com", to: "aiprovider.io" },
  { from: ".claude/", to: "/codeagent/" },
];

const HUAWEI_CODEAGENT_2_AUTH_HEADERS: ExtraAuthHeader[] = [
  { name: "x-auth-token", tokenSource: "huawei_codeagent_2" },
  { name: "Authorization", tokenSource: "huawei_codeagent_2", valuePrefix: "Bearer " },
  { name: "X-snap-traceid", tokenSource: "uuid" },
  { name: "Agent-Type", value: "AgentCenter" },
  { name: "app-id", value: "com.huawei.devmind.codebot.apibot" },
  { name: "x-innercc-request-kind", value: "chat" },
  { name: "x-error-propagation", value: "true" },
  { name: "Model-Id", tokenSource: "request_model" },
];

export const huaweiCodeAgent2Preset: ProviderPreset = {
  name: "Huawei CodeAgent 2.0",
  websiteUrl: "https://rnd-idea-api.huawei.com",
  category: "cloud_provider",
  providerType: "huawei_codeagent_2",
  appTarget: "claude",
  requiresOAuth: true,
  settingsConfig: {
    env: {
      ANTHROPIC_BASE_URL:
        "https://snapengine.cida.cce.prod-szv-g.dragon.tools.huawei.com/api/v2",
      ANTHROPIC_API_KEY: "",
      ANTHROPIC_MODEL: "claude-sonnet-4.6[1M]",
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-haiku-4.5",
      ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet-4.6[1M]",
      ANTHROPIC_DEFAULT_OPUS_MODEL: "claude-opus-4[1M]",
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

export { DEFAULT_FINGERPRINT_REPLACEMENTS, HUAWEI_CODEAGENT_2_AUTH_HEADERS };
