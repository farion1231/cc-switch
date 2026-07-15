import { describe, expect, it } from "vitest";
import {
  normalizeCodexCatalogModelsForSave,
  resolveProviderTypeForSave,
} from "@/components/providers/forms/ProviderForm";

describe("ProviderForm Codex catalog helpers", () => {
  it("normalizes catalog rows and removes empty or duplicate models", () => {
    expect(
      normalizeCodexCatalogModelsForSave([
        { model: " deepseek-v4-flash ", displayName: " DeepSeek " },
        { model: "deepseek-v4-flash", displayName: "Duplicate" },
        { model: "", displayName: "Empty" },
        { model: "kimi-k2", contextWindow: "128000 tokens" },
      ]),
    ).toEqual([
      { model: "deepseek-v4-flash", displayName: "DeepSeek" },
      { model: "kimi-k2", contextWindow: 128000 },
    ]);
  });

  it("preserves native-profile overrides (parallel tool calls + input modalities + base instructions)", () => {
    expect(
      normalizeCodexCatalogModelsForSave([
        {
          model: "MiniMax-M3",
          displayName: "MiniMax-M3",
          contextWindow: 1000000,
          supportsParallelToolCalls: true,
          inputModalities: ["text", "image"],
          baseInstructions:
            "  You are Codex, a coding agent based on MiniMax-M3.  ",
        },
        // false must be preserved (not dropped as falsy); empty modalities dropped;
        // empty/whitespace baseInstructions dropped
        {
          model: "mimo-v2.5-pro",
          supportsParallelToolCalls: false,
          inputModalities: [],
          baseInstructions: "   ",
        },
      ]),
    ).toEqual([
      {
        model: "MiniMax-M3",
        displayName: "MiniMax-M3",
        contextWindow: 1000000,
        supportsParallelToolCalls: true,
        inputModalities: ["text", "image"],
        baseInstructions: "You are Codex, a coding agent based on MiniMax-M3.",
      },
      { model: "mimo-v2.5-pro", supportsParallelToolCalls: false },
    ]);
  });
});

describe("resolveProviderTypeForSave", () => {
  it("优先使用预设/已保存数据的 providerType", () => {
    expect(
      resolveProviderTypeForSave({
        templatePresetProviderType: "github_copilot",
        activePresetProviderType: "codex_oauth",
        initialProviderType: "codex_oauth",
        isCopilotProvider: false,
      }),
    ).toBe("github_copilot");

    expect(
      resolveProviderTypeForSave({
        activePresetProviderType: "codex_oauth",
        initialProviderType: "github_copilot",
        isCopilotProvider: false,
      }),
    ).toBe("codex_oauth");

    expect(
      resolveProviderTypeForSave({
        initialProviderType: "codex_oauth",
        isCopilotProvider: true,
      }),
    ).toBe("codex_oauth");
  });

  it("自定义 Codex 供应商仅凭 base_url 含 githubcopilot.com 被识别为 Copilot 时，也要落盘 providerType=github_copilot", () => {
    // 对应 Codex review 建议 1：用户新建/编辑自定义供应商，未选预设、
    // initialData.meta 也没有 providerType，只是手填了 githubcopilot.com 的
    // base_url —— isCopilotProvider 为 true，此前会漏掉这个标记
    expect(
      resolveProviderTypeForSave({
        isCopilotProvider: true,
      }),
    ).toBe("github_copilot");
  });

  it("非 Copilot 且没有任何来源时返回 undefined", () => {
    expect(
      resolveProviderTypeForSave({
        isCopilotProvider: false,
      }),
    ).toBeUndefined();
  });
});
