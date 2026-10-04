import { describe, expect, it } from "vitest";

import type { ClaudeStackModel } from "@/types";
import { normalizeClaudeStackModels } from "./ClaudeStackModelsField";

describe("normalizeClaudeStackModels: contextWindow", () => {
  it("keeps positive integers and drops invalid values", () => {
    expect(
      normalizeClaudeStackModels([
        { model: "gpt-6.1-sol", contextWindow: 800000 },
        { model: "glm-5.2", contextWindow: 0 },
        { model: "kimi-k3", contextWindow: -5 },
        { model: "kimi-k3-air", contextWindow: Number.NaN },
        { model: "m" },
      ]),
    ).toEqual([
      { model: "gpt-6.1-sol", contextWindow: 800000 },
      { model: "glm-5.2" },
      { model: "kimi-k3" },
      { model: "kimi-k3-air" },
      { model: "m" },
    ]);
  });

  it("keeps the first window of duplicated models", () => {
    expect(
      normalizeClaudeStackModels([
        { model: "kimi-k3" },
        { model: "kimi-k3", contextWindow: 96000 },
        { model: "glm-5.2", contextWindow: 200000 },
        { model: "glm-5.2", contextWindow: 800000 },
      ]),
    ).toEqual([
      { model: "kimi-k3", contextWindow: 96000 },
      { model: "glm-5.2", contextWindow: 200000 },
    ]);
  });

  it("keeps the window of 1M rows so unchecking 1M restores it", () => {
    const rows: ClaudeStackModel[] = [
      { model: "glm-5.2", oneM: true, contextWindow: 1000000 },
    ];
    expect(normalizeClaudeStackModels(rows)).toEqual([
      { model: "glm-5.2", oneM: true, contextWindow: 1000000 },
    ]);
  });
});
