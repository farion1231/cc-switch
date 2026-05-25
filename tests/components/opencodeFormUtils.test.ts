import { describe, expect, it } from "vitest";

import { mergeFetchedModelsIntoOpenCodeModels } from "@/components/providers/forms/helpers/opencodeFormUtils";
import type { OpenCodeModel } from "@/types";

describe("mergeFetchedModelsIntoOpenCodeModels", () => {
  it("adds fetched models to an empty OpenCode model config", () => {
    const merged = mergeFetchedModelsIntoOpenCodeModels(
      {},
      [{ id: "claude-sonnet-4-6" }, { id: "gpt-5.4" }],
    );

    expect(merged).toEqual({
      "claude-sonnet-4-6": { name: "claude-sonnet-4-6" },
      "gpt-5.4": { name: "gpt-5.4" },
    });
  });

  it("preserves existing model configuration", () => {
    const current: Record<string, OpenCodeModel> = {
      "gpt-5.4": {
        name: "GPT 5.4",
        options: { provider: "apiway" },
      },
    };

    const merged = mergeFetchedModelsIntoOpenCodeModels(current, [
      { id: "gpt-5.4" },
      { id: "claude-sonnet-4-6" },
    ]);

    expect(merged["gpt-5.4"]).toEqual({
      name: "GPT 5.4",
      options: { provider: "apiway" },
    });
    expect(merged["claude-sonnet-4-6"]).toEqual({
      name: "claude-sonnet-4-6",
    });
  });

  it("returns the original object when no new models are added", () => {
    const current = { "gpt-5.4": { name: "GPT 5.4" } };

    const merged = mergeFetchedModelsIntoOpenCodeModels(current, [
      { id: "gpt-5.4" },
      { id: " " },
    ]);

    expect(merged).toBe(current);
  });
});
