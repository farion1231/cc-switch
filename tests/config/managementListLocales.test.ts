import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";

const requiredPaths = [
  "common.enableAllForApp",
  "common.disableAllForApp",
  "common.bulkToggleFailed",
  "skills.installedSearchPlaceholder",
  "skills.installedSearchAriaLabel",
  "skills.noInstalledSearchResults",
  "mcp.unifiedPanel.searchPlaceholder",
  "mcp.unifiedPanel.searchAriaLabel",
  "mcp.unifiedPanel.noSearchResults",
  "prompts.searchPlaceholder",
  "prompts.searchAriaLabel",
  "prompts.noSearchResults",
] as const;

type Locale = Record<string, unknown>;

const locales = [
  ["en", en],
  ["ja", ja],
  ["zh", zh],
  ["zh-TW", zhTW],
] as const;

function getTranslation(locale: Locale, path: string): unknown {
  return path.split(".").reduce<unknown>((value, key) => {
    if (!value || typeof value !== "object") return undefined;
    return (value as Record<string, unknown>)[key];
  }, locale);
}

function interpolationVariables(value: string): string[] {
  return Array.from(
    value.matchAll(/\{\{([^}]+)\}\}/g),
    ([, name]) => name,
  ).sort();
}

describe("management list locale coverage", () => {
  it.each(locales)("defines every management key in %s", (_name, locale) => {
    const missing = requiredPaths.filter((path) => {
      const value = getTranslation(locale as Locale, path);
      return typeof value !== "string" || value.trim().length === 0;
    });

    expect(missing).toEqual([]);
  });

  it.each(locales.slice(1))(
    "preserves interpolation variables in %s",
    (_name, locale) => {
      for (const path of requiredPaths) {
        const expected = getTranslation(en as Locale, path) as string;
        const actual = getTranslation(locale as Locale, path) as string;

        expect(interpolationVariables(actual)).toEqual(
          interpolationVariables(expected),
        );
      }
    },
  );
});
