import { describe, expect, it } from "vitest";
import {
  chatCompletionsUrl,
  workbuddyProviderPresets,
  setWorkBuddyCanDisableThinking,
  setWorkBuddyEffort,
  workbuddyThinkingFields,
} from "./workbuddyProviderPresets";
import { piProviderPresets } from "./piProviderPresets";

describe("workbuddyProviderPresets", () => {
  it("derives only Chat Completions Pi presets", () => {
    expect(workbuddyProviderPresets.length).toBeGreaterThan(0);
    for (const preset of workbuddyProviderPresets) {
      const pi = piProviderPresets.find((p) => p.name === preset.name)!;
      expect(pi.settingsConfig.api).toBe("openai-completions");
      expect(preset.settingsConfig).not.toHaveProperty("api");
      expect(preset.settingsConfig.apiKey).toBe("");
      expect(preset.settingsConfig.models.map((model) => model.id)).toEqual(
        pi.settingsConfig.models.map((model) => model.id),
      );
      // 菜单显示「厂商:模型 ID」，名字由写入时的厂商标签补上
      expect(preset.settingsConfig.vendor).toBe(preset.name);
      for (const model of preset.settingsConfig.models)
        expect(model).not.toHaveProperty("name");
    }
  });

  it("writes the full Chat Completions URL", () => {
    for (const preset of workbuddyProviderPresets) {
      expect(preset.settingsConfig.url).toMatch(
        /^https?:\/\/.+\/chat\/completions$/,
      );
    }
  });

  it("turns Pi thinking maps into WorkBuddy effort choices", () => {
    // Kimi K3 系：off 不支持，low / high / max
    expect(
      workbuddyThinkingFields({
        off: null,
        minimal: null,
        low: "low",
        medium: null,
        high: "high",
        xhigh: null,
        max: "max",
      }).reasoning,
    ).toEqual({
      supportedEfforts: ["low", "high", "max"],
      defaultEffort: "high",
      canDisableThinking: false,
    });
    // DeepSeek V4：off 没写＝支持，只有 high / max
    const deepseek = workbuddyThinkingFields({
      minimal: null,
      low: null,
      medium: null,
      high: "high",
      max: "max",
    });
    expect(deepseek.reasoning).toEqual({
      supportedEfforts: ["high", "max"],
      defaultEffort: "high",
      canDisableThinking: true,
    });
    // CLI 用它翻译请求，原样带上
    expect(deepseek.thinkingLevelMap).toEqual({
      minimal: null,
      low: null,
      medium: null,
      high: "high",
      max: "max",
    });
    // 只有 max：默认取最高档
    expect(
      workbuddyThinkingFields({
        off: null,
        minimal: null,
        low: null,
        medium: null,
        high: null,
        max: "max",
      }).reasoning,
    ).toEqual({
      supportedEfforts: ["max"],
      defaultEffort: "max",
      canDisableThinking: false,
    });
    // 空映射＝Pi 运行时默认，没有核对过的档位，不生成
    expect(workbuddyThinkingFields({})).toEqual({});
    expect(workbuddyThinkingFields(undefined)).toEqual({});
    expect(workbuddyThinkingFields({ bogus: "x" })).toEqual({});
  });

  it("carries reviewed thinking maps onto preset models", () => {
    const kimi = workbuddyProviderPresets.find((p) => p.name === "Kimi")!;
    const k31 = kimi.settingsConfig.models.find(
      (model) => model.id === "kimi-k3.1-pro",
    )!;
    expect(k31.reasoning?.supportedEfforts).toEqual(["low", "high", "max"]);
    expect(k31.reasoning?.canDisableThinking).toBe(false);
    expect(k31.thinkingLevelMap?.max).toBe("max");

    for (const preset of workbuddyProviderPresets) {
      for (const model of preset.settingsConfig.models) {
        if (!model.supportsReasoning)
          expect(model).not.toHaveProperty("reasoning");
        if (model.reasoning) {
          expect(model.reasoning.supportedEfforts).toContain(
            model.reasoning.defaultEffort,
          );
        }
      }
    }
  });

  it("edits effort levels from the form", () => {
    // 只有开关的自定义模型：勾一个强度就只有这一档，默认可关闭思考
    const max = setWorkBuddyEffort(
      { id: "m", supportsReasoning: true },
      "max",
      true,
    );
    expect(max.reasoning).toEqual({
      supportedEfforts: ["max"],
      defaultEffort: "max",
      canDisableThinking: true,
    });
    expect(max.thinkingLevelMap).toEqual({
      minimal: null,
      low: null,
      medium: null,
      high: null,
      xhigh: null,
      max: "max",
    });
    const both = setWorkBuddyEffort(max, "high", true);
    expect(both.reasoning?.supportedEfforts).toEqual(["high", "max"]);
    // 用户选的默认档还在就保留
    expect(both.reasoning?.defaultEffort).toBe("max");
    const locked = setWorkBuddyCanDisableThinking(both, false);
    expect(locked.reasoning?.canDisableThinking).toBe(false);
    expect(locked.thinkingLevelMap?.off).toBeNull();
    expect(
      setWorkBuddyCanDisableThinking(locked, true).thinkingLevelMap,
    ).not.toHaveProperty("off");
    // 全部取消：回到只有开关
    const none = setWorkBuddyEffort(
      setWorkBuddyEffort(both, "high", false),
      "max",
      false,
    );
    expect(none).toEqual({ id: "m", supportsReasoning: true });
  });

  it("keeps reviewed upstream values when re-selecting a level", () => {
    const model = {
      id: "m",
      supportsReasoning: true,
      ...workbuddyThinkingFields({ low: null, high: "deep", max: "max" }),
    };
    const off = setWorkBuddyEffort(model, "high", false);
    expect(off.thinkingLevelMap?.high).toBeNull();
    // 映射里已经是 null，勾回来只能填同名值；没动过的档位原样保留
    expect(setWorkBuddyEffort(model, "xhigh", true).thinkingLevelMap).toEqual({
      low: null,
      high: "deep",
      xhigh: "xhigh",
      max: "max",
    });
    // 只手写了 reasoning、没有映射的，按列出的强度还原
    const handWritten = setWorkBuddyEffort(
      {
        id: "m",
        reasoning: {
          supportedEfforts: ["low", "medium"],
          defaultEffort: "medium",
          canDisableThinking: false,
        },
      },
      "high",
      true,
    );
    expect(handWritten.reasoning).toEqual({
      supportedEfforts: ["low", "medium", "high"],
      defaultEffort: "medium",
      canDisableThinking: false,
    });
  });

  it("keeps default levels when a bare off:null map allows turning off", () => {
    const code = workbuddyProviderPresets
      .find((p) => p.name === "Kimi")!
      .settingsConfig.models.find((model) => model.id === "kimi-k2.7-code")!;
    expect(code.thinkingLevelMap).toEqual({ off: null });
    const next = setWorkBuddyCanDisableThinking(code, true);
    expect(next.reasoning).toEqual({
      ...code.reasoning,
      canDisableThinking: true,
    });
  });

  it("normalizes base URLs", () => {
    expect(chatCompletionsUrl("https://a.test/v1/")).toBe(
      "https://a.test/v1/chat/completions",
    );
    expect(chatCompletionsUrl("https://a.test/v1/chat/completions")).toBe(
      "https://a.test/v1/chat/completions",
    );
  });
});
