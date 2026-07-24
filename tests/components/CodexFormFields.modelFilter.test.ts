import { describe, expect, it } from "vitest";
import {
  isCopilotOneMEnabled,
  mapCopilotModelsForCodex,
  resolveCopilotContextWindowAutofill,
  resolveCopilotOneMToggle,
} from "@/components/providers/forms/CodexFormFields";
import type { CopilotModel } from "@/lib/api/copilot";

function copilotModel(overrides: Partial<CopilotModel>): CopilotModel {
  return {
    id: "model-id",
    name: "Model",
    vendor: "openai",
    model_picker_enabled: true,
    ...overrides,
  };
}

describe("mapCopilotModelsForCodex", () => {
  it("展示账号可见的全部模型，不再按 vendor 过滤（后端按 live vendor 自动路由）", () => {
    const models: CopilotModel[] = [
      copilotModel({ id: "gpt-5", vendor: "openai" }),
      copilotModel({ id: "gpt-5-mini", vendor: "OpenAI" }),
      copilotModel({ id: "claude-opus-4.6", vendor: "anthropic" }),
      copilotModel({ id: "gemini-3-pro", vendor: "google" }),
    ];

    expect(mapCopilotModelsForCodex(models)).toEqual([
      { id: "gpt-5", ownedBy: "openai" },
      { id: "gpt-5-mini", ownedBy: "OpenAI" },
      { id: "claude-opus-4.6", ownedBy: "anthropic" },
      { id: "gemini-3-pro", ownedBy: "google" },
    ]);
  });

  it("保留 o1/o3 等非 gpt- 命名的 OpenAI 模型，以及 -1m 扩展上下文变体", () => {
    const models: CopilotModel[] = [
      copilotModel({ id: "o1", vendor: "openai" }),
      copilotModel({ id: "o3-mini", vendor: "openai" }),
      copilotModel({ id: "chatgpt-4o-latest", vendor: "openai" }),
      copilotModel({ id: "claude-opus-4.7-1m-internal", vendor: "anthropic" }),
    ];

    expect(mapCopilotModelsForCodex(models)).toEqual([
      { id: "o1", ownedBy: "openai" },
      { id: "o3-mini", ownedBy: "openai" },
      { id: "chatgpt-4o-latest", ownedBy: "openai" },
      { id: "claude-opus-4.7-1m-internal", ownedBy: "anthropic" },
    ]);
  });

  it("空列表时返回空数组", () => {
    expect(mapCopilotModelsForCodex([])).toEqual([]);
  });
});

describe("resolveCopilotContextWindowAutofill", () => {
  it("行为空且 live 有真实 contextWindow 时回填（1M 变体场景）", () => {
    expect(resolveCopilotContextWindowAutofill(undefined, 1_000_000)).toBe(
      "1000000",
    );
    expect(resolveCopilotContextWindowAutofill("", 1_000_000)).toBe("1000000");
  });

  it("行已有值时绝不覆盖用户手填内容", () => {
    expect(
      resolveCopilotContextWindowAutofill("128000", 1_000_000),
    ).toBeUndefined();
  });

  it("live 没有该 id 的 contextWindow（未拉取过/字段缺失）时不回填", () => {
    expect(
      resolveCopilotContextWindowAutofill(undefined, undefined),
    ).toBeUndefined();
  });

  it("live contextWindow 非正数时不回填", () => {
    expect(resolveCopilotContextWindowAutofill(undefined, 0)).toBeUndefined();
    expect(resolveCopilotContextWindowAutofill(undefined, -1)).toBeUndefined();
  });
});

describe("isCopilotOneMEnabled", () => {
  it("model 带 [1M] 标记时为 true", () => {
    expect(isCopilotOneMEnabled("claude-sonnet-4-6[1M]", undefined)).toBe(true);
  });

  it("contextWindow >= 1_000_000 时为 true（无论是字符串还是数字）", () => {
    expect(isCopilotOneMEnabled("claude-opus-4.7-1m-internal", 1_000_000)).toBe(
      true,
    );
    expect(isCopilotOneMEnabled("claude-opus-4.7-1m-internal", "1000000")).toBe(
      true,
    );
  });

  it("两者都不满足时为 false", () => {
    expect(isCopilotOneMEnabled("claude-opus-4-5", 200_000)).toBe(false);
    expect(isCopilotOneMEnabled("claude-opus-4-5", undefined)).toBe(false);
  });
});

describe("resolveCopilotOneMToggle", () => {
  const liveModels: CopilotModel[] = [
    copilotModel({
      id: "claude-opus-4-5",
      vendor: "anthropic",
      context_window: 200_000,
    }),
    copilotModel({
      id: "claude-opus-4.7-1m-internal",
      vendor: "anthropic",
      context_window: 1_000_000,
    }),
  ];

  it("Anthropic 模型勾选 1M：保持实际请求模型，只声明 contextWindow", () => {
    const result = resolveCopilotOneMToggle(
      "claude-opus-4-5",
      true,
      liveModels,
    );
    expect(result).toEqual({
      model: "claude-opus-4-5",
      contextWindow: "1000000",
    });
  });

  it("Anthropic 模型未拉取 live 列表时勾选 1M：也不追加 [1M]", () => {
    const result = resolveCopilotOneMToggle("claude-sonnet-4.6", true, []);
    expect(result).toEqual({
      model: "claude-sonnet-4.6",
      contextWindow: "1000000",
    });
  });

  it("GPT 模型勾选 1M：只声明 contextWindow，不修改实际请求模型", () => {
    const result = resolveCopilotOneMToggle("gpt-5.3-codex", true, []);
    expect(result).toEqual({
      model: "gpt-5.3-codex",
      contextWindow: "1000000",
    });
  });

  it("GPT 模型取消 1M：保持实际请求模型并恢复 live contextWindow", () => {
    const result = resolveCopilotOneMToggle("gpt-5.3-codex", false, [
      copilotModel({
        id: "gpt-5.3-codex",
        vendor: "openai",
        context_window: 400_000,
      }),
    ]);
    expect(result).toEqual({
      model: "gpt-5.3-codex",
      contextWindow: "400000",
    });
  });

  it("Anthropic 模型取消 1M：保持实际请求模型并恢复其 live contextWindow", () => {
    const result = resolveCopilotOneMToggle(
      "claude-opus-4-5",
      false,
      liveModels,
    );
    expect(result).toEqual({
      model: "claude-opus-4-5",
      contextWindow: "200000",
    });
  });

  it("取消勾选但未拉取过模型：只剥离标记、清空 contextWindow（不猜数字）", () => {
    const result = resolveCopilotOneMToggle("claude-sonnet-4.6[1M]", false, []);
    expect(result).toEqual({
      model: "claude-sonnet-4.6",
      contextWindow: undefined,
    });
  });
});
