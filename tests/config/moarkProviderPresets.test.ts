import { describe, expect, it } from "vitest";

import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { mcodeProviderPresets } from "@/config/mcodeProviderPresets";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";
import { piProviderPresets } from "@/config/piProviderPresets";
import { getIcon, hasIcon } from "@/icons/extracted";

const WEBSITE_URL = "https://moark.com";
const API_KEY_URL = "https://moark.com/dashboard/tokens";
const DEFAULT_MODEL = "deepseek-v4-flash-0731";

describe("MoArk (模力方舟) provider presets", () => {
  it("uses the Anthropic-native root endpoint for Claude Code", () => {
    const preset = providerPresets.find((item) => item.name === "模力方舟");

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe(WEBSITE_URL);
    expect(preset?.apiKeyUrl).toBe(API_KEY_URL);
    expect(preset?.category).toBe("aggregator");
    expect(preset?.endpointCandidates).toEqual(["https://moark.com/anthropic"]);
    expect(preset?.icon).toBe("moark");

    const env = (preset?.settingsConfig as { env: Record<string, string> }).env;
    expect(env.ANTHROPIC_BASE_URL).toBe("https://moark.com/anthropic");
    expect(env.ANTHROPIC_AUTH_TOKEN).toBe("");
    expect(env.ANTHROPIC_MODEL).toBe(DEFAULT_MODEL);
    expect(env.ANTHROPIC_DEFAULT_HAIKU_MODEL).toBe(DEFAULT_MODEL);
    expect(env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe(DEFAULT_MODEL);
    expect(env.ANTHROPIC_DEFAULT_OPUS_MODEL).toBe(DEFAULT_MODEL);
  });

  it("uses the OpenAI-compatible Responses endpoint for Codex", () => {
    const preset = codexProviderPresets.find(
      (item) => item.name === "模力方舟",
    );

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe(WEBSITE_URL);
    expect(preset?.apiKeyUrl).toBe(API_KEY_URL);
    expect(preset?.category).toBe("aggregator");
    expect(preset?.endpointCandidates).toEqual(["https://moark.com/v1"]);
    expect(preset?.icon).toBe("moark");
    expect(preset?.auth).toEqual({ OPENAI_API_KEY: "" });
    expect(preset?.config).toContain('model = "deepseek-v4-flash-0731"');
    expect(preset?.config).toContain('base_url = "https://moark.com/v1"');
    expect(preset?.config).toContain('wire_api = "responses"');
  });

  it("declares the Codex model catalog so /model lists every MoArk model", () => {
    const preset = codexProviderPresets.find(
      (item) => item.name === "模力方舟",
    );
    const catalog = preset?.modelCatalog ?? [];

    // 缺 modelCatalog 时 Codex 退回通用元数据，/model 只会列出默认模型。
    expect(catalog.map((model) => model.model)).toEqual([
      DEFAULT_MODEL,
      "DeepSeek-V4-Pro",
      "GLM-5.3",
      "Kimi-K2.7-Code",
      "qwen3-coder-plus",
    ]);

    // 目录首行 = 默认模型，必须与 config.toml 的 model 一致。
    expect(preset?.config).toContain(`model = "${catalog[0]?.model}"`);

    for (const model of catalog) {
      expect(
        model.contextWindow,
        `${model.model} contextWindow`,
      ).toBeGreaterThan(0);
      expect(model.inputModalities, `${model.model} inputModalities`).toContain(
        "text",
      );
    }
    // Kimi K2.7 Code 与 OpenClaw 预设一致地支持图像输入。
    expect(
      catalog.find((model) => model.model === "Kimi-K2.7-Code")
        ?.inputModalities,
    ).toEqual(["text", "image"]);
  });

  it("keeps Codex capacities in step with the OpenClaw preset", () => {
    const codex = codexProviderPresets.find((item) => item.name === "模力方舟");
    const openclaw = openclawProviderPresets.find(
      (item) => item.name === "模力方舟",
    );
    const openclawById = new Map(
      (openclaw?.settingsConfig.models ?? []).map((model) => [model.id, model]),
    );

    for (const model of codex?.modelCatalog ?? []) {
      const counterpart = openclawById.get(model.model);
      expect(
        counterpart,
        `${model.model} exists in the OpenClaw preset`,
      ).toBeDefined();
      expect(model.contextWindow, `${model.model} contextWindow`).toBe(
        counterpart?.contextWindow,
      );
    }
  });

  it("uses the Codex NativeResponses profile for the direct gateway", () => {
    const preset = codexProviderPresets.find(
      (item) => item.name === "模力方舟",
    );

    // moark.com 不在 CODEX_NATIVE_RESPONSES_HOSTS 兜底名单里，而这条路径是原生
    // Responses 直连、没有代理去改写 custom→function。不显式声明 apiFormat 会让
    // resolve_codex_catalog_tool_profile 落成 ProxyChat，即克隆 gpt-5.5 模板
    // （GPT-5 harness + 自由格式 apply_patch），给聚合平台下发它未承诺的能力。
    expect(preset?.apiFormat).toBe("openai_responses");
  });

  it("declares only platform- or vendor-verified Codex reasoning levels", () => {
    const preset = codexProviderPresets.find(
      (item) => item.name === "模力方舟",
    );
    const levels = Object.fromEntries(
      (preset?.modelCatalog ?? []).map((model) => [
        model.model,
        model.reasoningLevels,
      ]),
    );

    // 实测自 https://moark.com/v1/responses：非法 reasoning_effort 触发的 400
    // 里厂商给出的明文枚举。
    expect(levels["DeepSeek-V4-Pro"]).toEqual([
      "low",
      "medium",
      "high",
      "xhigh",
      "max",
    ]);
    expect(levels["GLM-5.3"]).toEqual([
      "none",
      "low",
      "medium",
      "high",
      "xhigh",
      "max",
    ]);
    expect(levels["Kimi-K2.7-Code"]).toEqual([
      "none",
      "minimal",
      "low",
      "medium",
      "high",
      "xhigh",
    ]);
    // MoArk 不校验该字段，退回 DeepSeek 官方目录的档位。
    expect(levels[DEFAULT_MODEL]).toEqual(["low", "high", "max"]);
    // MoArk 对 qwen3-coder-plus 完全不兑现 effort（none 与 ultra 的
    // reasoning_tokens 同为 0）。留空不等于"没有档位"：后端会把未填写理解为沿用
    // 原生模板的 none/high 两档、默认 high，Codex 仍会给出一个不改变任何行为的
    // 选择器，所以要显式声明单档 none。
    expect(levels["qwen3-coder-plus"]).toEqual(["none"]);

    // 只有单档时 Codex 直接应用、不显示选择界面；四款支持思考的模型必须保留多档
    // 且每档都含 high，后端才会保留模板默认 high，与 config 的
    // model_reasoning_effort = "high" 一致。
    const thinkingModels = (preset?.modelCatalog ?? []).filter(
      (model) => model.model !== "qwen3-coder-plus",
    );
    expect(thinkingModels).toHaveLength(4);
    for (const model of thinkingModels) {
      expect(model.reasoningLevels, `${model.model} keeps high`).toContain(
        "high",
      );
      expect(
        model.reasoningLevels?.length,
        `${model.model} still offers a choice`,
      ).toBeGreaterThan(1);
    }
    expect(preset?.config).toContain('model_reasoning_effort = "high"');
  });

  it("uses the OpenAI-compatible endpoint for Pi", () => {
    const preset = piProviderPresets.find((item) => item.name === "模力方舟");

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe(WEBSITE_URL);
    expect(preset?.apiKeyUrl).toBe(API_KEY_URL);
    expect(preset?.category).toBe("aggregator");
    expect(preset?.icon).toBe("moark");
    expect(preset?.providerKey).toBe("cc-switch-moark");
    expect(preset?.settingsConfig.baseUrl).toBe("https://api.moark.com/v1");
    expect(preset?.settingsConfig.api).toBe("openai-completions");
    expect(preset?.settingsConfig.apiKey).toBe("");

    const modelIds = preset?.settingsConfig.models.map((model) => model.id);
    expect(modelIds).toEqual(
      expect.arrayContaining([
        DEFAULT_MODEL,
        "DeepSeek-V4-Pro",
        "GLM-5.3",
        "Kimi-K2.7-Code",
        "qwen3-coder-plus",
      ]),
    );
  });

  it("uses the OpenAI-compatible endpoint for OpenCode", () => {
    const preset = opencodeProviderPresets.find(
      (item) => item.name === "模力方舟",
    );
    const models = preset?.settingsConfig.models ?? {};

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe(WEBSITE_URL);
    expect(preset?.apiKeyUrl).toBe(API_KEY_URL);
    expect(preset?.category).toBe("aggregator");
    expect(preset?.icon).toBe("moark");
    expect(preset?.settingsConfig.npm).toBe("@ai-sdk/openai-compatible");
    expect(preset?.settingsConfig.options?.baseURL).toBe(
      "https://api.moark.com/v1",
    );
    expect(models).toHaveProperty(DEFAULT_MODEL);
    expect(models).toHaveProperty("Kimi-K2.7-Code");
    expect(models[DEFAULT_MODEL]?.name).toBe("DeepSeek V4 Flash");

    // OpenCode only offers thinking variants for models flagged as reasoning;
    // Qwen3 Coder Plus has no thinking mode and stays unflagged.
    for (const id of [
      DEFAULT_MODEL,
      "DeepSeek-V4-Pro",
      "GLM-5.3",
      "Kimi-K2.7-Code",
    ]) {
      expect(models[id]?.reasoning, `${id} reasoning`).toBe(true);
    }
    expect(models["qwen3-coder-plus"]?.reasoning).toBeUndefined();
  });

  it("uses the OpenAI Completions endpoint for OpenClaw", () => {
    const preset = openclawProviderPresets.find(
      (item) => item.name === "模力方舟",
    );
    const modelIds = (preset?.settingsConfig.models ?? []).map(
      (model) => model.id,
    );

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe(WEBSITE_URL);
    expect(preset?.apiKeyUrl).toBe(API_KEY_URL);
    expect(preset?.category).toBe("aggregator");
    expect(preset?.icon).toBe("moark");
    expect(preset?.settingsConfig.baseUrl).toBe("https://api.moark.com/v1");
    expect(preset?.settingsConfig.api).toBe("openai-completions");
    expect(modelIds).toEqual(
      expect.arrayContaining([
        DEFAULT_MODEL,
        "DeepSeek-V4-Pro",
        "GLM-5.3",
        "Kimi-K2.7-Code",
      ]),
    );
    expect(preset?.suggestedDefaults?.model).toEqual({
      primary: `moark/${DEFAULT_MODEL}`,
    });
    expect(preset?.suggestedDefaults?.modelCatalog).toHaveProperty(
      `moark/${DEFAULT_MODEL}`,
    );
  });

  it("declares positive capacity limits for every OpenCode model", () => {
    const preset = opencodeProviderPresets.find(
      (item) => item.name === "模力方舟",
    );
    const models = preset?.settingsConfig.models ?? {};

    // OpenCode treats a missing limit as 0 and then skips auto-compaction
    // (`isOverflow` returns false when `limit.context === 0`), so every model
    // of a custom provider key must carry explicit capacity numbers.
    expect(Object.keys(models)).toHaveLength(5);
    for (const [id, model] of Object.entries(models)) {
      expect(model.limit?.context, `${id} limit.context`).toBeGreaterThan(0);
      expect(model.limit?.output, `${id} limit.output`).toBeGreaterThan(0);
    }
  });

  it("keeps OpenCode capacities consistent with the OpenClaw preset", () => {
    const opencode = opencodeProviderPresets.find(
      (item) => item.name === "模力方舟",
    );
    const openclaw = openclawProviderPresets.find(
      (item) => item.name === "模力方舟",
    );
    const openclawById = new Map(
      (openclaw?.settingsConfig.models ?? []).map((model) => [model.id, model]),
    );

    for (const [id, model] of Object.entries(
      opencode?.settingsConfig.models ?? {},
    )) {
      const counterpart = openclawById.get(id);
      expect(counterpart, `${id} exists in the OpenClaw preset`).toBeDefined();
      expect(model.limit?.context, `${id} context`).toBe(
        counterpart?.contextWindow,
      );
      expect(model.limit?.output, `${id} output`).toBe(counterpart?.maxTokens);
    }
  });

  it("inherits the Pi preset for MiniMax Code", () => {
    const preset = mcodeProviderPresets.find(
      (item) => item.name === "模力方舟",
    );

    expect(preset).toBeDefined();
    expect(preset?.settingsConfig.kind).toBe("custom");
    expect(preset?.settingsConfig.api).toBe("openai-completions");
    expect(preset?.settingsConfig.options.baseURL).toBe(
      "https://api.moark.com/v1",
    );
    expect(Object.keys(preset?.settingsConfig.models ?? {})).toContain(
      DEFAULT_MODEL,
    );
  });

  it("registers the MoArk mark in the icon registry", () => {
    expect(hasIcon("moark")).toBe(true);
    expect(getIcon("moark")).toContain("<title>MoArk</title>");
    expect(getIcon("moark")).toContain('viewBox="0 0 88 88"');
  });
});
