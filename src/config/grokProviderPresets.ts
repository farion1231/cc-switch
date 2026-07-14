import {
  generateThirdPartyAuth,
  type CodexProviderPreset,
} from "./codexProviderPresets";

const quote = (value: string) => JSON.stringify(value);

export function generateGrokProfileConfig(
  baseUrl: string,
  model = "grok-4.5",
  backend: "responses" | "chat_completions" = "responses",
): string {
  return `[endpoints]
models_base_url = ${quote(baseUrl)}

[models]
default = ${quote(model)}
web_search = ${quote(model)}

[subagents]
default_model = ${quote(model)}

[model.${quote(model)}]
model = ${quote(model)}
base_url = ${quote(baseUrl)}
api_backend = ${quote(backend)}
supports_backend_search = true
context_window = 500000
`;
}

/** Grok Build Profile 预设；TOML 结构与 Grok Build 原生配置一致。 */
export const grokProviderPresets: CodexProviderPreset[] = [
  {
    name: "Grok 官方账号 (OAuth)",
    websiteUrl: "https://grok.com",
    auth: {},
    config: "",
    isOfficial: true,
    category: "official",
    icon: "xai",
  },
  {
    name: "xAI API (Responses)",
    websiteUrl: "https://x.ai/api",
    apiKeyUrl: "https://console.x.ai",
    auth: generateThirdPartyAuth(""),
    config: generateGrokProfileConfig(
      "https://api.x.ai/v1",
      "grok-4.5",
      "responses",
    ),
    endpointCandidates: ["https://api.x.ai/v1"],
    apiFormat: "openai_responses",
    category: "third_party",
    icon: "xai",
  },
  {
    name: "OpenAI 兼容接口",
    websiteUrl: "",
    auth: generateThirdPartyAuth(""),
    config: generateGrokProfileConfig(
      "https://api.example.com/v1",
      "grok-model",
      "chat_completions",
    ),
    apiFormat: "openai_chat",
    category: "third_party",
    icon: "xai",
  },
];

export function getGrokCustomTemplate() {
  return {
    auth: generateThirdPartyAuth(""),
    config: generateGrokProfileConfig(
      "https://api.x.ai/v1",
      "grok-4.5",
      "responses",
    ),
  };
}
