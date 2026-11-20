import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";
import { piProviderPresets } from "@/config/piProviderPresets";

// A model list pasted into several presets drifts: the next model refresh
// updates some copies and misses the rest. Each list is written once as a
// `const <preset>Models = () => [...]` and every preset that offers the same
// models calls it.
const CONFIG_DIR = path.resolve(__dirname, "../../src/config");
const PRESET_FILES = [
  "codexProviderPresets.ts",
  "hermesProviderPresets.ts",
  "openclawProviderPresets.ts",
  "opencodeProviderPresets.ts",
  "piProviderPresets.ts",
];

/** Literal values of `models:` / `modelCatalog:` keys spanning 3+ lines. */
function modelListLiterals(source: string): string[] {
  const literals: string[] = [];
  const key = /\b(?:models|modelCatalog)\s*:\s*(?:modelCatalog\()?([[{])/g;
  for (const match of source.matchAll(key)) {
    const start = match.index! + match[0].length - 1;
    let depth = 0;
    let quote: string | null = null;
    let end = start;
    for (; end < source.length; end++) {
      const char = source[end];
      if (quote) {
        if (char === "\\") end++;
        else if (char === quote) quote = null;
      } else if (char === '"' || char === "'" || char === "`") {
        quote = char;
      } else if (source.startsWith("//", end)) {
        end = source.indexOf("\n", end);
      } else if (char === "[" || char === "{") {
        depth++;
      } else if ((char === "]" || char === "}") && --depth === 0) {
        break;
      }
    }
    const literal = source.slice(start, end + 1);
    if (literal.split("\n").length >= 3) {
      literals.push(literal.replace(/\s+/g, " "));
    }
  }
  return literals;
}

describe("preset model lists", () => {
  it.each(PRESET_FILES)("%s writes each model list once", (file) => {
    const source = readFileSync(path.join(CONFIG_DIR, file), "utf8");
    const seen = new Set<string>();
    const repeated = modelListLiterals(source).filter(
      (literal) => seen.has(literal) || !seen.add(literal),
    );
    expect(
      repeated,
      `${file} repeats a model list; hoist it into a shared ` +
        "`const <preset>Models = (): ... => [...]` and call it from each preset",
    ).toEqual([]);
  });

  it("gives every preset its own model objects", () => {
    const lists: unknown[] = [
      ...codexProviderPresets.map((preset) => preset.modelCatalog),
      ...hermesProviderPresets.map((preset) => preset.settingsConfig.models),
      ...openclawProviderPresets.map((preset) => preset.settingsConfig.models),
      ...opencodeProviderPresets.map((preset) => preset.settingsConfig.models),
      ...piProviderPresets.map((preset) => preset.settingsConfig.models),
    ];
    const seen = new Set<object>();
    for (const list of lists.filter(Boolean) as object[]) {
      for (const value of [list, ...Object.values(list)]) {
        expect(seen.has(value), "a model object is shared by two presets").toBe(
          false,
        );
        seen.add(value);
      }
    }
  });
});
