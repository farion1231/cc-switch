import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import ko from "@/i18n/locales/ko.json";
import ru from "@/i18n/locales/ru.json";
import tr from "@/i18n/locales/tr.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";

// All seven UI languages are maintained together: every English key must exist
// in every other locale, with the same interpolation variables and tags.
const locales = [
  ["zh", zh],
  ["zh-TW", zhTW],
  ["ja", ja],
  ["ko", ko],
  ["tr", tr],
  ["ru", ru],
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
