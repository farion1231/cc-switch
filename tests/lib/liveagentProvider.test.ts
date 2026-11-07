import { describe, expect, it } from "vitest";
import {
  addLiveAgentModel,
  applyFetchedLiveAgentModels,
  enabledLiveAgentModels,
  liveAgentFetchApiFormat,
  setLiveAgentModelEnabled,
  changeLiveAgentModel,
  initialLiveAgentConfig,
  liveagentProviderSchema,
  removeLiveAgentModel,
} from "@/lib/liveagentProvider";
import { fillLiveAgentModel } from "@/components/providers/forms/modelMetadataFill";

const base = {
  api: "openai-responses" as const,
  baseUrl: "https://example.com/v1",
  apiKey: "k",
  models: [{ id: "a" }],
  activeModels: ["a"],
};

describe("liveagentProvider", () => {
  it("rejects fields K-brain cannot decode", () => {
    expect(liveagentProviderSchema.safeParse(base).success).toBe(true);
    expect(
      liveagentProviderSchema.safeParse({ ...base, maxOutputToken: 1 }).success,
    ).toBe(false);
    expect(
      liveagentProviderSchema.safeParse({
        ...base,
        models: [{ id: "a", maxOutputToken: 1 }],
      }).success,
    ).toBe(false);
    expect(
      liveagentProviderSchema.safeParse({
        ...base,
        usageQuery: { enabled: false },
        models: [{ id: "a", limitsSource: "catalog", contextWindow: 10 }],
      }).success,
    ).toBe(true);
  });

  it("keeps a typed native provider's derived API instead of inventing one", () => {
    const native = { type: "claude_code", baseUrl: "", apiKey: "", models: [] };
    const config = initialLiveAgentConfig(native);
    expect(config.api).toBeUndefined();
    expect(liveagentProviderSchema.safeParse(config).success).toBe(true);
    expect(
      liveagentProviderSchema.safeParse({ ...native, type: undefined }).success,
    ).toBe(false);
    expect(initialLiveAgentConfig().api).toBe("openai-completions");
  });

  it("enables each newly added model once its id is typed", () => {
    let config = addLiveAgentModel(addLiveAgentModel(base));
    expect(config.activeModels).toEqual(["a"]);
    config = changeLiveAgentModel(config, 1, { id: "b" });
    config = changeLiveAgentModel(config, 1, { id: "bc" });
    config = changeLiveAgentModel(config, 2, { id: "d" });
    expect(config.models.map((m) => m.id)).toEqual(["a", "bc", "d"]);
    expect(config.activeModels).toEqual(["a", "bc", "d"]);
    expect(removeLiveAgentModel(config, 1).activeModels).toEqual(["a", "d"]);
  });

  it("applies the fetched-model selection and keeps activeModels in step", () => {
    const config = {
      ...base,
      models: [{ id: "a" }, { id: "b" }],
      activeModels: ["a"],
    };
    const next = applyFetchedLiveAgentModels(config, {
      add: ["c", "a", "c"],
      remove: ["b"],
    });
    expect(next.models.map((m) => m.id)).toEqual(["a", "c"]);
    expect(next.activeModels).toEqual(["a", "c"]);
    const all = applyFetchedLiveAgentModels(
      { ...base, activeModels: undefined },
      { add: ["d"], remove: [] },
    );
    expect(all.activeModels).toBeUndefined();
    expect(setLiveAgentModelEnabled(all, "d", false).activeModels).toEqual([
      "a",
    ]);
    expect(liveagentProviderSchema.safeParse(next).success).toBe(true);
  });

  it("fills only empty model fields and never narrows input", () => {
    const filled = fillLiveAgentModel(
      { id: "m", maxTokens: 1000 },
      {
        contextWindow: 200000,
        maxOutputTokens: 64000,
        reasoning: true,
        inputModalities: ["text", "image"],
      },
    );
    expect(filled).toEqual({
      id: "m",
      maxTokens: 1000,
      contextWindow: 200000,
      reasoning: true,
      input: ["text", "image"],
    });
    const textOnly = fillLiveAgentModel(
      { id: "m", reasoning: false },
      { reasoning: true, inputModalities: ["text"] },
    );
    expect(textOnly).toEqual({ id: "m", reasoning: false });
    expect(
      liveagentProviderSchema.safeParse({
        ...base,
        models: [filled],
        activeModels: ["m"],
      }).success,
    ).toBe(true);
  });

  it("derives the model-list API format for typed providers", () => {
    expect(
      liveAgentFetchApiFormat({ ...base, api: "anthropic-messages" }),
    ).toBe("anthropic-messages");
    expect(
      liveAgentFetchApiFormat({ ...base, api: undefined, type: "claude_code" }),
    ).toBe("anthropic-messages");
    expect(
      liveAgentFetchApiFormat({ ...base, api: undefined, type: "xai" }),
    ).toBeUndefined();
  });

  it("offers only enabled models as default-model choices", () => {
    expect(
      enabledLiveAgentModels({
        models: [{ id: "a", name: "A" }, { id: "b" }, { id: "" }],
        activeModels: ["b"],
      }),
    ).toEqual([{ id: "b", name: undefined }]);
    expect(enabledLiveAgentModels({ models: [{ id: "a" }] })).toEqual([
      { id: "a", name: undefined },
    ]);
    expect(enabledLiveAgentModels(null)).toEqual([]);
  });
});
