import { describe, expect, it } from "vitest";
import {
  openclawProviderPresets,
  rebaseOpenClawSuggestedDefaults,
  savedOpenClawModelIds,
} from "@/config/openclawProviderPresets";

describe("rebaseOpenClawSuggestedDefaults", () => {
  it("registers the models saved in the form, keeping preset aliases", () => {
    const preset = openclawProviderPresets.find(
      (item) => item.name === "OpenRouter",
    );

    const defaults = rebaseOpenClawSuggestedDefaults(
      preset!.suggestedDefaults!,
      "my-or",
      ["anthropic/claude-sonnet-5.5", "stealth/ox-alpha"],
    );

    expect(defaults.modelCatalog).toEqual({
      "my-or/anthropic/claude-sonnet-5.5": { alias: "Sonnet" },
      "my-or/stealth/ox-alpha": {},
    });
    expect(defaults.model).toEqual({
      primary: "my-or/anthropic/claude-sonnet-5.5",
      fallbacks: [],
    });

    const kept = rebaseOpenClawSuggestedDefaults(
      preset!.suggestedDefaults!,
      "my-or",
      [
        "stealth/ox-alpha",
        "anthropic/claude-sonnet-5.5",
        "anthropic/claude-opus-5.5",
      ],
    );
    expect(kept.model).toEqual({
      primary: "my-or/anthropic/claude-opus-5.5",
      fallbacks: ["my-or/anthropic/claude-sonnet-5.5"],
    });
  });
});

describe("savedOpenClawModelIds", () => {
  it("keeps only the non-empty model ids that were saved", () => {
    expect(
      savedOpenClawModelIds(
        JSON.stringify({ models: [{ id: "a" }, { id: " " }, { name: "b" }] }),
      ),
    ).toEqual(["a"]);
    expect(savedOpenClawModelIds("{}")).toEqual([]);
    expect(savedOpenClawModelIds("null")).toEqual([]);
  });
});
