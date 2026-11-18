import { describe, expect, it } from "vitest";
import {
  ATOMCODE_PROVIDER_TYPES,
  ATOMCODE_VENDOR_PROVIDER_LABELS,
  atomCodeCustomTemplate,
  atomcodeProviderPresets,
  normalizeAtomCodeProvider,
} from "./atomcodeProviderPresets";

// 与后端 atomcode_config::validate_provider 同款约束
const KEY_PATTERN = /^[a-zA-Z0-9._-]+$/;
// AtomCode 预设 id（atomcode 仓库 provider_preset.rs 的 PRESETS 子集）
const PROVIDER_TYPES = new Set([
  // 通用协议
  "openai-compatible",
  "anthropic-compatible",
  "openai-responses",
  "deepseek-messages",
  "ollama",
  // 厂商预设
  "openai",
  "anthropic",
  "deepseek",
  "zhipu",
  "zhipu-coding",
  "aliyun",
  "siliconflow",
  "moonshot",
  "minimax",
  "minimax-token-plan",
  "xiaomi-mimo",
  "longcat",
  "volcengine",
  "volcengine-coding",
  "openrouter",
  "taotoken",
  "opencode",
]);

describe("atomcodeProviderPresets", () => {
  it("uses account shapes that pass backend validate_provider", () => {
    for (const preset of atomcodeProviderPresets) {
      expect(KEY_PATTERN.test(preset.providerKey), preset.name).toBe(true);
      const { account, models } = preset.settingsConfig;
      expect(
        PROVIDER_TYPES.has(account.provider),
        `${preset.name}/account.provider`,
      ).toBe(true);
      expect(() => new URL(account.base_url), preset.name).not.toThrow();
      expect(Object.keys(models).length, preset.name).toBeGreaterThan(0);
      for (const [key, model] of Object.entries(models)) {
        expect(key.trim(), `${preset.name}/${key}`).not.toBe("");
        expect(model.model.trim(), `${preset.name}/${key}`).not.toBe("");
        expect(
          model.context_window ?? 1,
          `${preset.name}/${key}`,
        ).toBeGreaterThan(0);
        // account 字段由后端写入时统一改写，预设里不该带
        expect(model, `${preset.name}/${key}`).not.toHaveProperty("account");
      }
    }
  });

  it("keeps the DeepSeek official preset in sync with the 2026-10-25 Flash-only lineup", () => {
    const deepseek = atomcodeProviderPresets.find((p) => p.name === "DeepSeek");
    expect(deepseek).toBeDefined();
    expect(deepseek!.providerKey).toBe("deepseek");
    expect(deepseek!.settingsConfig.account.provider).toBe("deepseek");
    expect(deepseek!.settingsConfig.account.base_url).toBe(
      "https://api.deepseek.com/v1",
    );
    expect(Object.keys(deepseek!.settingsConfig.models)).toEqual([
      "deepseek-flash",
    ]);
  });

  it("categorizes Anthropic and OpenAI under model vendors (cn_official)", () => {
    const anthropic = atomcodeProviderPresets.find(
      (p) => p.name === "Anthropic",
    );
    expect(anthropic).toBeDefined();
    expect(anthropic!.category).toBe("cn_official");
    expect(anthropic!.icon).toBe("anthropic");

    const openai = atomcodeProviderPresets.find((p) => p.name === "OpenAI");
    expect(openai).toBeDefined();
    expect(openai!.category).toBe("cn_official");
    expect(openai!.icon).toBe("openai");
  });

  it("declares exactly the protocol ids AtomCode's TUI offers", () => {
    expect(ATOMCODE_PROVIDER_TYPES.map((type) => type.value)).toEqual([
      "openai-compatible",
      "anthropic-compatible",
      "openai-responses",
      "deepseek-messages",
      "ollama",
    ]);
  });

  it("ships the documented Ollama local preset", () => {
    const ollama = atomcodeProviderPresets.find((p) =>
      p.name.startsWith("Ollama"),
    );
    expect(ollama).toBeDefined();
    expect(ollama!.category).toBe("local");
    expect(ollama!.settingsConfig.account.provider).toBe("ollama");
    expect(ollama!.settingsConfig.account.base_url).toBe(
      "http://localhost:11434",
    );
    expect(ollama!.settingsConfig.models["llama3.2"]).toMatchObject({
      model: "llama3.2",
      context_window: 8000,
    });
  });

  it("uses the anthropic preset id and current Sonnet model", () => {
    const anthropic = atomcodeProviderPresets.find(
      (p) => p.name === "Anthropic",
    );
    expect(anthropic!.settingsConfig.account.provider).toBe("anthropic");
    expect(anthropic!.settingsConfig.models["claude-sonnet-4-6"]).toMatchObject(
      { model: "claude-sonnet-4-6", context_window: 128000 },
    );
  });

  it("gives the custom template an empty account and no models", () => {
    const template = atomCodeCustomTemplate();
    expect(template.account.provider).toBe("openai-compatible");
    expect(template.account.base_url).toBe("");
    expect(template.models).toEqual({});
  });

  it("normalizes legacy wire names to AtomCode preset ids", () => {
    // claude 若不归一化会被 AtomCode 当成未知 id 回退 openai-compatible，直接不可用
    expect(normalizeAtomCodeProvider("claude")).toBe("anthropic");
    expect(normalizeAtomCodeProvider("openai_responses")).toBe(
      "openai-responses",
    );
    expect(normalizeAtomCodeProvider("gemini")).toBe("openai-compatible");
    // 已经是预设 id 的值原样保留
    expect(normalizeAtomCodeProvider("openrouter")).toBe("openrouter");
    expect(normalizeAtomCodeProvider("anthropic-compatible")).toBe(
      "anthropic-compatible",
    );
  });

  it("gives every vendor preset a label for the edit form", () => {
    for (const preset of atomcodeProviderPresets) {
      const provider = preset.settingsConfig.account.provider;
      const isProtocol = ATOMCODE_PROVIDER_TYPES.some(
        (type) => type.value === provider,
      );
      if (!isProtocol) {
        expect(
          ATOMCODE_VENDOR_PROVIDER_LABELS[provider],
          preset.name,
        ).toBeTruthy();
      }
    }
  });

  // AtomCode 源码 provider_preset.rs（main，registry 19 个厂商）里的 provider id 与
  // base_url。deepseek 与 ollama 另有专项测试，不在此表。
  it("mirrors AtomCode's preset registry ids and base URLs", () => {
    const expected: Record<string, [string, string]> = {
      anthropic: ["anthropic", "https://api.anthropic.com"],
      openai: ["openai", "https://api.openai.com/v1"],
      zhipu: ["zhipu", "https://open.bigmodel.cn/api/paas/v4"],
      "zhipu-coding": [
        "zhipu-coding",
        "https://open.bigmodel.cn/api/coding/paas/v4",
      ],
      moonshot: ["moonshot", "https://api.moonshot.cn/v1"],
      aliyun: ["aliyun", "https://dashscope.aliyuncs.com/compatible-mode/v1"],
      minimax: ["minimax", "https://api.minimaxi.com/v1"],
      "minimax-token-plan": ["minimax-token-plan", "https://api.minimax.cn/v1"],
      "xiaomi-mimo": ["xiaomi-mimo", "https://api.xiaomimimo.com/v1"],
      longcat: ["longcat", "https://api.longcat.chat/openai/v1"],
      volcengine: ["volcengine", "https://ark.cn-beijing.volces.com/api/v3"],
      "volcengine-coding": [
        "volcengine-coding",
        "https://ark.cn-beijing.volces.com/api/coding/v3",
      ],
      siliconflow: ["siliconflow", "https://api.siliconflow.cn/v1"],
      openrouter: ["openrouter", "https://openrouter.ai/api/v1"],
      taotoken: ["taotoken", "https://taotoken.net/api/v1"],
      opencode: ["opencode", "https://opencode.ai/zen/v1"],
    };
    for (const [key, [provider, baseUrl]] of Object.entries(expected)) {
      const preset = atomcodeProviderPresets.find((p) => p.providerKey === key);
      expect(preset, key).toBeDefined();
      expect(preset!.settingsConfig.account.provider, key).toBe(provider);
      expect(preset!.settingsConfig.account.base_url, key).toBe(baseUrl);
    }
  });

  it("keeps provider keys unique", () => {
    const keys = atomcodeProviderPresets.map((p) => p.providerKey);
    expect(new Set(keys).size).toBe(keys.length);
  });
});
