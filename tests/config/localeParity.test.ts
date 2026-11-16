import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import de from "@/i18n/locales/de.json";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import ko from "@/i18n/locales/ko.json";
import ru from "@/i18n/locales/ru.json";
import tr from "@/i18n/locales/tr.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";

// All eight UI languages are maintained together: every English key must exist
// in every other locale, with the same interpolation variables and tags.
const locales = [
  ["zh", zh],
  ["zh-TW", zhTW],
  ["ja", ja],
  ["ko", ko],
  ["tr", tr],
  ["ru", ru],
  ["de", de],
] as const;

function flattenStrings(
  value: unknown,
  path: string[] = [],
  result = new Map<string, string>(),
): Map<string, string> {
  if (typeof value === "string") {
    result.set(path.join("."), value);
  } else if (value && typeof value === "object") {
    for (const [key, child] of Object.entries(value)) {
      flattenStrings(child, [...path, key], result);
    }
  }
  return result;
}

function variables(value: string): string[] {
  return Array.from(
    value.matchAll(/\{\{\s*([\w.]+)\s*\}\}/g),
    ([, name]) => name,
  );
}

function tags(value: string): string {
  return Array.from(value.matchAll(/<\/?\d+>/g), ([m]) => m)
    .sort()
    .join(" ");
}

const reference = flattenStrings(en);

describe("locale parity", () => {
  it.each(locales)("defines every English key in %s", (_name, tree) => {
    const translations = flattenStrings(tree);
    expect(
      [...reference.keys()].filter((key) => !translations.has(key)),
    ).toEqual([]);
  });

  it.each(locales)("has no keys that English removed in %s", (_name, tree) => {
    const translations = flattenStrings(tree);
    expect(
      [...translations.keys()].filter((key) => !reference.has(key)),
    ).toEqual([]);
  });

  it.each(locales)("keeps every translation non-empty in %s", (_name, tree) => {
    expect(
      [...flattenStrings(tree)]
        .filter(([key, value]) => reference.get(key)?.trim() && !value.trim())
        .map(([key]) => key),
    ).toEqual([]);
  });

  // The check above uses English as the reference, so an emptied English
  // string would slip through it. These placeholders are empty on purpose.
  const intentionallyEmpty = new Set([
    "providerForm.modelPlaceholder",
    "providerForm.smallModelPlaceholder",
    "providerForm.haikuModelPlaceholder",
  ]);
  it("keeps every English string non-empty", () => {
    expect(
      [...reference]
        .filter(([key, value]) => !value.trim() && !intentionallyEmpty.has(key))
        .map(([key]) => key),
    ).toEqual([]);
  });

  // A few locales use variables English does not (zh passes {{labelSp}}, zh/ja
  // split dates into {{day}}/{{month}}), so a variable is valid when any other
  // locale uses it too. A variable no other locale uses is a typo or a leftover
  // the caller no longer passes, and would render as literal "{{name}}".
  const all = [["en", en] as const, ...locales].map(
    ([name, tree]) => [name, flattenStrings(tree)] as const,
  );
  it.each(locales)("uses only known interpolation variables in %s", (name) => {
    const own = all.find(([n]) => n === name)![1];
    const unknown: string[] = [];
    for (const [key, value] of own) {
      const known = new Set(
        all
          .filter(([n]) => n !== name)
          .flatMap(([, strings]) => variables(strings.get(key) ?? "")),
      );
      for (const variable of variables(value)) {
        if (!known.has(variable)) unknown.push(`${key}: {{${variable}}}`);
      }
    }
    expect(unknown).toEqual([]);
  });

  // These phrasings take different variables than English, and the callers
  // pass all of them. Every other key uses exactly English's variables.
  const ownVariables: Record<string, string[]> = {
    zh: [
      "providerPreset.matchedVersions",
      "prompts.updatedOn",
      "quota.tierLeft",
      "quota.tierUsed",
      "quota.tierUsedUp",
    ],
    "zh-TW": [
      "providerPreset.matchedVersions",
      "prompts.updatedOn",
      "quota.tierLeft",
      "quota.tierUsed",
      "quota.tierUsedUp",
    ],
    ja: ["prompts.updatedOn"],
  };
  it.each(locales)("uses English's interpolation variables in %s", (name) => {
    const own = all.find(([n]) => n === name)![1];
    const exempt = new Set(ownVariables[name] ?? []);
    const key = (value: string) => variables(value).sort().join(" ");
    expect(
      [...own]
        .filter(([k, value]) => {
          const expected = reference.get(k);
          return (
            expected !== undefined &&
            !exempt.has(k) &&
            key(value) !== key(expected)
          );
        })
        .map(([k]) => k),
    ).toEqual([]);
  });

  it.each(locales)("keeps rich-text tags in %s", (_name, tree) => {
    expect(
      [...flattenStrings(tree)]
        .filter(([key, value]) => {
          const expected = reference.get(key);
          return expected !== undefined && tags(value) !== tags(expected);
        })
        .map(([key]) => key),
    ).toEqual([]);
  });
});

// A key the code passes to t() but English lacks renders as the raw key in
// every language. Parity cannot see it, because all locales lack it together.
describe("translation keys used in code", () => {
  const src = path.resolve(__dirname, "../../src");
  const KEY = `["'\`]([\\w-]+(?:\\.[\\w-]+)+)["'\`]`;
  const keyPatterns = [
    // t("a.b"), i18n.t("a.b")
    new RegExp(`(?<![\\w.])(?:i18n\\.)?t\\(\\s*${KEY}`, "g"),
    // t(cond ? "a.b" : "a.c")
    new RegExp(`(?<![\\w.])t\\([^()]*?\\?\\s*${KEY}\\s*:\\s*${KEY}`, "g"),
    // <Trans i18nKey="a.b" />
    new RegExp(`i18nKey=${KEY}`, "g"),
  ];
  // The session reader passes its scoped translator (keys under
  // sessionManager.reader) to helpers as `t`.
  const readerDir = path.join("components", "sessions", "reader");
  const exists = (key: string) =>
    reference.has(key) ||
    reference.has(`${key}_one`) ||
    reference.has(`${key}_other`);

  it("defines every literal key passed to t() or i18nKey in English", () => {
    const missing: string[] = [];
    let checked = 0;
    const files = readdirSync(src, { recursive: true, encoding: "utf8" });
    for (const file of files.filter((f) => /\.tsx?$/.test(f))) {
      const text = readFileSync(path.join(src, file), "utf8");
      const keys = keyPatterns.flatMap((pattern) =>
        [...text.matchAll(pattern)].flatMap((match) => match.slice(1)),
      );
      for (const key of keys.filter(Boolean)) {
        checked += 1;
        const scoped = file.startsWith(readerDir)
          ? `sessionManager.reader.${key}`
          : undefined;
        if (!exists(key) && !(scoped && exists(scoped))) {
          missing.push(`${file}: ${key}`);
        }
      }
    }
    expect(checked).toBeGreaterThan(1000);
    expect(missing).toEqual([]);
  });
});
