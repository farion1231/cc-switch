import { describe, expect, it } from "vitest";
import { orderAppIds } from "@/config/appConfig";
import { APP_IDS } from "@/config/appSpecs";

describe("orderAppIds", () => {
  it("falls back to the default order when nothing is saved", () => {
    const alphabetical = [
      "agy",
      "claude",
      "claude-desktop",
      "codex",
      "dsh",
      "gemini",
      "copilot",
      "grokbuild",
      "hermes",
      "kimi",
      "mcode",
      "omp",
      "openclaw",
      "opencode",
      "pi",
      "workbuddy",
    ];
    expect(orderAppIds(undefined)).toEqual(alphabetical);
    expect(orderAppIds([])).toEqual(alphabetical);
  });

  it("keeps the saved order and drops unknown or repeated ids", () => {
    const saved = [...APP_IDS].reverse();
    expect(orderAppIds(["future-app", ...saved, "claude"])).toEqual(saved);
  });

  it("puts an app missing from the saved order after its default predecessor", () => {
    const defaults = orderAppIds(undefined);
    const saved = defaults.filter((app) => app !== "kimi" && app !== "hermes");
    expect(orderAppIds([...saved, "hermes"])).toEqual([
      ...saved,
      "hermes",
      "kimi",
    ]);
    expect(orderAppIds(defaults.filter((app) => app !== "agy"))[0]).toBe("agy");
  });
});
