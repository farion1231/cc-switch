import { describe, expect, it } from "vitest";
import { mapCopilotModelsForCodex } from "@/components/providers/forms/CodexFormFields";
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
  it("只保留 vendor 为 openai 的模型（大小写不敏感）", () => {
    const models: CopilotModel[] = [
      copilotModel({ id: "gpt-5", vendor: "openai" }),
      copilotModel({ id: "gpt-5-mini", vendor: "OpenAI" }),
      copilotModel({ id: "claude-opus-4.6", vendor: "anthropic" }),
      copilotModel({ id: "gemini-3-pro", vendor: "google" }),
    ];

    expect(mapCopilotModelsForCodex(models)).toEqual([
      { id: "gpt-5", ownedBy: "openai" },
      { id: "gpt-5-mini", ownedBy: "OpenAI" },
    ]);
  });

  it("不按 'gpt-' 前缀匹配，保留 o1/o3 等非 gpt- 命名的 OpenAI 模型", () => {
    const models: CopilotModel[] = [
      copilotModel({ id: "o1", vendor: "openai" }),
      copilotModel({ id: "o3-mini", vendor: "openai" }),
      copilotModel({ id: "chatgpt-4o-latest", vendor: "openai" }),
    ];

    expect(mapCopilotModelsForCodex(models)).toEqual([
      { id: "o1", ownedBy: "openai" },
      { id: "o3-mini", ownedBy: "openai" },
      { id: "chatgpt-4o-latest", ownedBy: "openai" },
    ]);
  });

  it("空列表 / 全部非 openai vendor 时返回空数组", () => {
    expect(mapCopilotModelsForCodex([])).toEqual([]);
    expect(
      mapCopilotModelsForCodex([
        copilotModel({ id: "claude-sonnet-4.6", vendor: "anthropic" }),
      ]),
    ).toEqual([]);
  });
});
