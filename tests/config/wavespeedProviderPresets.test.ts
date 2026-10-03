import { describe, expect, it } from "vitest";
import { providerPresets } from "@/config/claudeProviderPresets";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";
import {
  opencodeProviderPresets,
  OPENCODE_PRESET_MODEL_VARIANTS,
} from "@/config/opencodeProviderPresets";

const WAVESPEED_BASE_URL = "https://llm.wavespeed.ai/v1";
const DEFAULT_MODEL = "google/gemini-2.5-flash";
const FALLBACK_MODEL = "google/gemini-2.5-flash-lite";
const VERIFIED_CHAT_MODELS = [
  DEFAULT_MODEL,
  FALLBACK_MODEL,
  "google/gemini-2.5-pro",
  "openai/gpt-5",
  "openai/gpt-5-mini",
  "openai/gpt-5-nano",
  "openai/gpt-4.1",
  "openai/gpt-4.1-mini",
  "openai/gpt-4o",
  "openai/gpt-4o-mini",
  "anthropic/claude-opus-4.1",
  "anthropic/claude-opus-4",
  "anthropic/claude-sonnet-4",
  "anthropic/claude-3.7-sonnet",
  "deepseek/deepseek-chat-v3.1",
  "deepseek/deepseek-chat",
  "deepseek/deepseek-r1",
  "qwen/qwen3-coder",
  "qwen/qwen3-max-thinking",
  "qwen/qwen3-235b-a22b-2507",
  "qwen/qwen3-235b-a22b",
  "qwen/qwen3-32b",
  "x-ai/grok-4",
  "x-ai/grok-3",
  "meta-llama/llama-4-maverick",
  "meta-llama/llama-4-scout",
  "meta-llama/llama-3.3-70b-instruct",
];

describe("WaveSpeed provider presets", () => {
  it("uses OpenAI Chat format for Claude Code", () => {
    const preset = providerPresets.find((item) => item.name === "WaveSpeed");
    const env = (preset?.settingsConfig as any)?.env ?? {};

    expect(preset).toBeDefined();
    expect(preset?.category).toBe("aggregator");
    expect(preset?.apiFormat).toBe("openai_chat");
    expect(preset?.endpointCandidates).toEqual([WAVESPEED_BASE_URL]);
    expect(env.ANTHROPIC_BASE_URL).toBe(WAVESPEED_BASE_URL);
    expect(env.ANTHROPIC_MODEL).toBe(DEFAULT_MODEL);
    expect(env.ANTHROPIC_DEFAULT_HAIKU_MODEL).toBe(FALLBACK_MODEL);
  });

  it("uses OpenAI-compatible config for OpenCode", () => {
    const preset = opencodeProviderPresets.find(
      (item) => item.name === "WaveSpeed",
    );

    expect(preset).toBeDefined();
    expect(preset?.category).toBe("aggregator");
    expect(preset?.settingsConfig.npm).toBe("@ai-sdk/openai-compatible");
    expect(preset?.settingsConfig.options?.baseURL).toBe(WAVESPEED_BASE_URL);
    expect(Object.keys(preset?.settingsConfig.models ?? {})).toEqual(
      expect.arrayContaining(VERIFIED_CHAT_MODELS),
    );

    const variants =
      OPENCODE_PRESET_MODEL_VARIANTS["@ai-sdk/openai-compatible"];
    expect(variants.map((model) => model.id)).toEqual(
      expect.arrayContaining(VERIFIED_CHAT_MODELS),
    );
    expect(variants.find((model) => model.id === DEFAULT_MODEL)).toMatchObject({
      contextLimit: 1048576,
      outputLimit: 65535,
    });
  });

  it("uses provider-prefixed defaults for OpenClaw", () => {
    const preset = openclawProviderPresets.find(
      (item) => item.name === "WaveSpeed",
    );
    const modelIds = (preset?.settingsConfig.models ?? []).map(
      (model) => model.id,
    );

    expect(preset).toBeDefined();
    expect(preset?.category).toBe("aggregator");
    expect(preset?.settingsConfig.baseUrl).toBe(WAVESPEED_BASE_URL);
    expect(preset?.settingsConfig.api).toBe("openai-completions");
    expect(modelIds).toEqual(expect.arrayContaining(VERIFIED_CHAT_MODELS));
    expect(preset?.suggestedDefaults?.model).toEqual({
      primary: `wavespeed/${DEFAULT_MODEL}`,
      fallbacks: [`wavespeed/${FALLBACK_MODEL}`],
    });
    expect(preset?.suggestedDefaults?.modelCatalog).toHaveProperty(
      `wavespeed/${DEFAULT_MODEL}`,
    );
  });

  it("uses chat completions config for Hermes", () => {
    const preset = hermesProviderPresets.find(
      (item) => item.name === "WaveSpeed",
    );

    expect(preset).toBeDefined();
    expect(preset?.category).toBe("aggregator");
    expect(preset?.settingsConfig.name).toBe("wavespeed");
    expect(preset?.settingsConfig.base_url).toBe(WAVESPEED_BASE_URL);
    expect(preset?.settingsConfig.api_mode).toBe("chat_completions");
    expect(
      (preset?.settingsConfig.models ?? []).map((model) => model.id),
    ).toEqual(expect.arrayContaining(VERIFIED_CHAT_MODELS));
    expect(preset?.suggestedDefaults?.model).toEqual({
      default: DEFAULT_MODEL,
      provider: "wavespeed",
    });
  });
});
