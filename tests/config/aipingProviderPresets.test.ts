import { describe, expect, it } from "vitest";
import { providerPresets } from "@/config/claudeProviderPresets";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";
import { piProviderPresets } from "@/config/piProviderPresets";
import { ompProviderPresets } from "@/config/ompProviderPresets";
import { mcodeProviderPresets } from "@/config/mcodeProviderPresets";
import { dshProviderPresets } from "@/config/dshProviderPresets";
import { kimiProviderPresets } from "@/config/kimiProviderPresets";
import { workbuddyProviderPresets } from "@/config/workbuddyProviderPresets";

const AI_PING_MODELS = [
  "DeepSeek-V4.1-Flash-1017",
  "GLM-5.3-Flash",
  "Kimi-K3.1-Pro",
  "MiniMax-M3.1-Flash",
];

describe("AI Ping presets", () => {
  it("offers a native Anthropic preset for Claude Code", () => {
    const preset = providerPresets.find((item) => item.name === "AI Ping")!;
    expect(preset.category).toBe("aggregator");
    expect(preset.icon).toBe("aiping");
    expect(preset.apiFormat).toBe("anthropic");
    expect(preset.settingsConfig).toMatchObject({
      env: {
        ANTHROPIC_BASE_URL: "https://aiping.cn/api/v1/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "DeepSeek-V4.1-Flash-1017",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "DeepSeek-V4.1-Flash-1017",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "Kimi-K3.1-Pro",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "GLM-5.3-Flash",
        ANTHROPIC_SMALL_FAST_MODEL: "MiniMax-M3.1-Flash",
      },
    });
    expect(preset.endpointCandidates).toEqual([
      "https://aiping.cn/api/v1/anthropic",
    ]);
  });

  it("maps the four Claude Desktop roles to AI Ping models", () => {
    const preset = claudeDesktopProviderPresets.find(
      (item) => item.name === "AI Ping",
    )!;
    expect(preset.baseUrl).toBe("https://aiping.cn/api/v1/anthropic");
    expect(preset.mode).toBe("proxy");
    expect(preset.apiFormat).toBe("anthropic");
    expect(preset.modelRoutes).toEqual([
      {
        routeId: "claude-sonnet-5",
        upstreamModel: "DeepSeek-V4.1-Flash-1017",
        supports1m: false,
      },
      {
        routeId: "claude-haiku-4-5",
        upstreamModel: "GLM-5.3-Flash",
        supports1m: false,
      },
      {
        routeId: "claude-opus-5",
        upstreamModel: "Kimi-K3.1-Pro",
        supports1m: false,
      },
      {
        routeId: "claude-fable-5",
        upstreamModel: "MiniMax-M3.1-Flash",
        supports1m: false,
      },
    ]);
  });

  it("feeds Codex through the local chat router", () => {
    const preset = codexProviderPresets.find(
      (item) => item.name === "AI Ping",
    )!;
    expect(preset.apiFormat).toBe("openai_chat");
    expect(preset.modelCatalog?.map((model) => model.model)).toEqual(
      AI_PING_MODELS,
    );
    expect(preset.config).toContain('base_url = "https://aiping.cn/api/v1"');
    expect(preset.config).toContain('model = "DeepSeek-V4.1-Flash-1017"');
  });

  it("offers the same model catalog to the OpenAI-compatible clients", () => {
    const opencode = opencodeProviderPresets.find(
      (item) => item.name === "AI Ping",
    )!;
    expect(Object.keys(opencode.settingsConfig.models)).toEqual(AI_PING_MODELS);

    const openclaw = openclawProviderPresets.find(
      (item) => item.name === "AI Ping",
    )!;
    expect(openclaw.settingsConfig.models?.map((model) => model.id)).toEqual(
      AI_PING_MODELS,
    );

    const hermes = hermesProviderPresets.find(
      (item) => item.name === "AI Ping",
    )!;
    expect(hermes.settingsConfig.models?.map((model) => model.id)).toEqual(
      AI_PING_MODELS,
    );
    expect(hermes.settingsConfig.base_url).toBe("https://aiping.cn/api/v1");
    expect(hermes.settingsConfig.api_mode).toBe("chat_completions");
  });

  it("propagates the Pi preset to every Pi-derived client", () => {
    const pi = piProviderPresets.find((item) => item.name === "AI Ping")!;
    expect(pi.providerKey).toBe("cc-switch-aiping");
    expect(pi.settingsConfig.models.map((model) => model.id)).toEqual(
      AI_PING_MODELS,
    );
    for (const [app, presets] of Object.entries({
      omp: ompProviderPresets,
      mcode: mcodeProviderPresets,
      dsh: dshProviderPresets,
      kimi: kimiProviderPresets,
      workbuddy: workbuddyProviderPresets,
    })) {
      const preset = presets.find((item) => item.name === "AI Ping");
      expect(preset, `${app} should include AI Ping`).toBeDefined();
    }
  });
});
