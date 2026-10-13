import { describe, expect, it } from "vitest";
import tr from "@/i18n/locales/tr.json";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";

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
  ["tr", tr],
  ["zh", zh],
  ["ja", ja],
  ["zh-TW", zhTW],
] as const;

describe("locale coverage", () => {
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

describe("Turkish locale", () => {
  it("covers the complete English catalog without stale keys or empty values", () => {
    const translations = flattenStrings(tr);
    expect([...translations.keys()].sort()).toEqual(
      [...reference.keys()].sort(),
    );
    expect(
      [...translations].filter(
        ([key, value]) => reference.get(key)?.trim() && !value.trim(),
      ),
    ).toEqual([]);
  });
  it("preserves interpolation variables across the full catalog", () => {
    const translations = flattenStrings(tr);
    expect(
      [...reference]
        .filter(
          ([key, value]) =>
            interpolationVariables(value).join("\0") !==
            interpolationVariables(translations.get(key) ?? "").join("\0"),
        )
        .map(([key]) => key),
    ).toEqual([]);
  });

  it("preserves protocol fields, configuration literals, and commands", () => {
    const translations = flattenStrings(tr);
    const exactLiterals = [
      "opencode.extraOptionKeyPlaceholder",
      "opencode.modelExtraFieldKeyPlaceholder",
      "opencode.modelOptionKeyPlaceholder",
      "opencode.modelOptionValuePlaceholder",
    ];

    for (const key of exactLiterals) {
      expect(translations.get(key), key).toBe(reference.get(key));
    }

    expect(translations.get("usageScript.fieldRemaining")).toContain(
      "• remaining:",
    );
    expect(translations.get("usageScript.fieldUnit")).toContain("• unit:");
    expect(translations.get("usageScript.fieldTotal")).toContain("• total:");
    expect(translations.get("usageScript.fieldUsed")).toContain("• used:");
    expect(translations.get("usageScript.fieldExtra")).toContain("• extra:");
    expect(translations.get("settings.s3Sync.presets.r2Hint")).toContain(
      "'auto'",
    );
    expect(translations.get("settings.s3Sync.presets.cosHint")).toContain(
      "ap-beijing",
    );
    expect(translations.get("hermes.webui.launchConfirmMessage")).toContain(
      "pip install hermes-agent[web]",
    );
  });
});
