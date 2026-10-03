import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";
import { COPILOT_ERROR_KEYS } from "@/lib/copilotByokMessages";

type TranslationTree = Record<string, unknown>;

function flattenStrings(
  value: unknown,
  path: string[] = [],
  result = new Map<string, string>(),
): Map<string, string> {
  if (typeof value === "string") {
    result.set(path.join("."), value);
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

const reference = flattenStrings(en);
const piKeysOutsideNamespace = new Set([
  "apps.pi",
  "deeplink.api",
  "sessionManager.piDiscoveryUnavailable",
  "sessionManager.piRelativeSessionDir",
  "settings.browsePlaceholderPi",
  "settings.piConfigDir",
  "settings.piConfigDirDescription",
]);
const piReference = new Map(
  [...reference].filter(
    ([key]) => key.startsWith("pi.") || piKeysOutsideNamespace.has(key),
  ),
);
const piProductReferences = new Map(
  [...reference].filter(([, value]) => /\bPi\b/.test(value)),
);
const locales = [
  ["zh", zh],
  ["ja", ja],
  ["zh-TW", zhTW],
] as const;

const copilotKeysOutsideNamespace = new Set([
  "apps.copilotByok",
  "apps.copilot-byok",
  "apps.copilotCli",
  "apps.copilot-cli",
  "usage.appFilter.copilot-byok",
  "usage.appFilter.copilot-cli",
  "common.disabled",
  "common.restore",
]);
const copilotReference = new Map(
  [...reference].filter(
    ([key]) =>
      key.startsWith("copilotByok.") || copilotKeysOutsideNamespace.has(key),
  ),
);
const copilotLocales = [["en", en], ...locales] as const;

describe("locale coverage", () => {
  it.each(copilotLocales)(
    "covers every Copilot key and error operation with nonempty text in %s",
    (_name, tree) => {
      const translations = flattenStrings(tree as TranslationTree);
      const required = new Set([
        ...copilotReference.keys(),
        ...Object.values(COPILOT_ERROR_KEYS),
      ]);
      expect(
        [...required].filter((key) => !translations.get(key)?.trim()),
      ).toEqual([]);
    },
  );

  it.each(copilotLocales)(
    "preserves every Copilot interpolation variable in %s",
    (_name, tree) => {
      const translations = flattenStrings(tree as TranslationTree);
      expect(
        [...copilotReference].flatMap(([key, expected]) => {
          const actual = translations.get(key);
          return actual === undefined ||
            interpolationVariables(actual).join("\0") !==
              interpolationVariables(expected).join("\0")
            ? [key]
            : [];
        }),
      ).toEqual([]);
    },
  );

  it.each(locales)("covers every Pi translation key in %s", (_name, tree) => {
    const translations = flattenStrings(tree as TranslationTree);
    const missing = [...piReference.keys()].filter(
      (key) => !translations.has(key),
    );

    expect(missing).toEqual([]);
  });

  it.each(locales)(
    "preserves every Pi interpolation variable in %s",
    (_name, tree) => {
      const translations = flattenStrings(tree as TranslationTree);
      const mismatched = [...piReference].flatMap(([key, expected]) => {
        const actual = translations.get(key);
        return actual !== undefined &&
          interpolationVariables(actual).join("\0") !==
            interpolationVariables(expected).join("\0")
          ? [key]
          : [];
      });

      expect(mismatched).toEqual([]);
    },
  );

  it.each(locales)(
    "preserves explicit Pi product mentions in %s",
    (_name, tree) => {
      const translations = flattenStrings(tree as TranslationTree);
      const missingMentions = [...piProductReferences.keys()].filter((key) => {
        const actual = translations.get(key);
        return actual === undefined || !/\bPi\b/.test(actual);
      });

      expect(missingMentions).toEqual([]);
    },
  );
});
