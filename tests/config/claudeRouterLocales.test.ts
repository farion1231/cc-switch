import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";

type TranslationTree = Record<string, unknown>;

function flattenStrings(
  value: unknown,
  path: string[] = [],
  result: Record<string, string> = {},
): Record<string, string> {
  if (typeof value === "string") {
    result[path.join(".")] = value;
  } else if (typeof value === "object" && value !== null) {
    for (const [key, child] of Object.entries(value)) {
      flattenStrings(child, [...path, key], result);
    }
  }
  return result;
}

function interpolationVariables(value: string): string[] {
  return Array.from(
    value.matchAll(/\{\{\s*([^}]+?)\s*\}\}/g),
    ([, name]) => name,
  ).sort();
}

const requiredKeys = [
  "claudeRouter.launchIsolated",
  "claudeRouter.providerBadge",
  "claudeRouter.form.enabled",
  "claudeRouter.form.alias",
  "claudeRouter.form.displayName",
  "claudeRouter.form.upstreamModel",
  "claudeRouter.validation.modelsRequired",
  "claudeRouter.validation.fieldsRequired",
  "claudeRouter.validation.duplicateAlias",
  "claudeRouter.panel.title",
  "claudeRouter.panel.status",
  "claudeRouter.setup.title",
  "claudeRouter.setup.modelCount",
  "claudeRouter.setup.mergeWarning",
  "claudeRouter.setup.reloadHint",
  "claudeRouter.setup.copy",
] as const;

const reference = flattenStrings(en as TranslationTree);
const locales = [
  ["en", en],
  ["zh", zh],
  ["ja", ja],
  ["zh-TW", zhTW],
] as const;

describe("Claude router locale coverage", () => {
  it.each(locales)("contains required router strings in %s", (_name, tree) => {
    const translations = flattenStrings(tree as TranslationTree);
    expect(requiredKeys.filter((key) => !translations[key])).toEqual([]);
  });

  it.each(locales)(
    "preserves router key and interpolation parity in %s",
    (_name, tree) => {
      const translations = flattenStrings(tree as TranslationTree);
      const routerReference = Object.entries(reference).filter(([key]) =>
        key.startsWith("claudeRouter."),
      );
      const missingOrMismatched = routerReference.flatMap(([key, expected]) => {
        const actual = translations[key];
        return actual &&
          interpolationVariables(actual).join("\0") ===
            interpolationVariables(expected).join("\0")
          ? []
          : [key];
      });
      expect(missingOrMismatched).toEqual([]);
    },
  );
});
