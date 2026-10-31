import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";
import {
  findMissing,
  findOrphan,
  flattenStrings,
  selectCoveredKeys,
} from "./localeGate";

/**
 * The selector is intentionally structural: a closed namespace prefix plus an
 * explicit list of keys that live outside the namespace. No value regexes —
 * those silently change meaning when copy is reworded.
 */
const NAMESPACES = ["remote."] as const;
const EXTRA_KEYS = ["settings.remoteEnabled"] as const;

const reference = flattenStrings(en as Record<string, unknown>);
const covered = selectCoveredKeys(reference, [...NAMESPACES], [...EXTRA_KEYS]);

const locales = [
  ["zh", zh],
  ["ja", ja],
  ["zh-TW", zhTW],
] as const;

/**
 * i18next substitutes `{{name}}` at render time, so a translation that drops or
 * renames a variable does not fail to load — it renders with the literal
 * `{{name}}` pasted into the UI. Comparing the sorted variable *sets* is the
 * only cheap way to catch it, and it deliberately ignores ordering: zh/ja may
 * legitimately place the placeholder earlier than en.
 */
function interpolationVariables(value: string): string[] {
  return Array.from(
    value.matchAll(/\{\{([^}]+)\}\}/g),
    ([, name]) => name,
  ).sort();
}

/**
 * Keys whose translation interpolates a different set of variables than the en
 * reference. A key absent from the translation is not reported here — missing
 * keys are the previous test's job.
 */
function mismatchedInterpolationKeys(
  referenceStrings: Map<string, string>,
  coveredKeys: Set<string>,
  translations: Map<string, string>,
): string[] {
  return [...coveredKeys].filter((key) => {
    const expected = referenceStrings.get(key);
    const actual = translations.get(key);
    if (expected === undefined || actual === undefined) return false;

    return (
      interpolationVariables(actual).join("\0") !==
      interpolationVariables(expected).join("\0")
    );
  });
}

describe("remote locale coverage", () => {
  it.each(locales)(
    "covers every remote.* translation key in %s",
    (_name, tree) => {
      const locale = tree as Record<string, unknown>;

      expect(findMissing(locale, covered)).toEqual([]);
      expect(findOrphan(locale, reference, [...NAMESPACES])).toEqual([]);
    },
  );

  it.each(locales)("preserves interpolation variables in %s", (_name, tree) => {
    const translations = flattenStrings(tree as Record<string, unknown>);

    expect(
      mismatchedInterpolationKeys(reference, covered, translations),
    ).toEqual([]);
  });
});

describe("remote interpolation gate", () => {
  it("flags a dropped variable and ignores reordering", () => {
    // Synthetic trees only, for the same reason as the locale gate self-test
    // above: a negative case must never touch the real locale files.
    const syntheticReference = flattenStrings({
      remote: {
        approval: {
          hint: "Bound to {{scope}} on port {{port}}",
          plain: "No variables here",
        },
      },
      settings: { remoteEnabled: "Remote port is {{port}}" },
    });
    const syntheticCovered = selectCoveredKeys(
      syntheticReference,
      ["remote."],
      ["settings.remoteEnabled"],
    );

    // Same variables, different order: a legitimate rewrite, not a regression.
    const reordered = flattenStrings({
      remote: {
        approval: {
          hint: "已绑定到端口 {{port}}（{{scope}}）",
          plain: "没有变量",
        },
      },
      settings: { remoteEnabled: "远程端口：{{port}}" },
    });
    expect(
      mismatchedInterpolationKeys(
        syntheticReference,
        syntheticCovered,
        reordered,
      ),
    ).toEqual([]);

    // One variable dropped: the UI would render a literal `{{scope}}`.
    const dropped = flattenStrings({
      remote: {
        approval: {
          hint: "已绑定到端口 {{port}}",
          plain: "没有变量",
        },
      },
      settings: { remoteEnabled: "远程端口：{{port}}" },
    });
    expect(
      mismatchedInterpolationKeys(
        syntheticReference,
        syntheticCovered,
        dropped,
      ),
    ).toEqual(["remote.approval.hint"]);

    // Guard against the gate going vacuous: the reference it just rejected
    // really does carry two variables.
    expect(
      interpolationVariables("Bound to {{scope}} on port {{port}}"),
    ).toEqual(["port", "scope"]);
  });
});

describe("locale gate", () => {
  it("locale gate self-test", () => {
    // Synthetic trees only: a negative case must never touch the real locale
    // files, otherwise this self-test stops being runnable in CI.
    const syntheticReference = flattenStrings({
      remote: {
        approval: { title: "Approve device", evidence: { tailnet: "Tailnet" } },
      },
      settings: { remoteEnabled: "Enable Remote Management Server" },
    });
    const syntheticCovered = selectCoveredKeys(
      syntheticReference,
      ["remote."],
      ["settings.remoteEnabled"],
    );

    // A mis-written selector must fail loudly instead of looping zero times.
    expect(syntheticCovered.size).toBeGreaterThan(0);
    expect([...syntheticCovered].sort()).toEqual([
      "remote.approval.evidence.tailnet",
      "remote.approval.title",
      "settings.remoteEnabled",
    ]);

    // A key deleted from a translation is reported.
    const deleted = {
      remote: { approval: { evidence: { tailnet: "Tailnet" } } },
      settings: { remoteEnabled: "启用远程管理服务" },
    };
    expect(findMissing(deleted, syntheticCovered)).toEqual([
      "remote.approval.title",
    ]);

    // A key present only in a translation is reported as an orphan.
    const withOrphan = {
      remote: {
        approval: {
          title: "Approve device",
          evidence: { tailnet: "Tailnet" },
          ghost: "Removed upstream",
        },
      },
      settings: { remoteEnabled: "启用远程管理服务" },
    };
    expect(findOrphan(withOrphan, syntheticReference, ["remote."])).toEqual([
      "remote.approval.ghost",
    ]);

    // Keys outside the namespaces and outside `extraKeys` are out of scope:
    // neither required nor orphans.
    const outOfScope = {
      remote: {
        approval: { title: "Approve device", evidence: { tailnet: "Tailnet" } },
      },
      settings: { remoteEnabled: "启用远程管理服务", unrelated: "Unrelated" },
    };
    expect(findMissing(outOfScope, syntheticCovered)).toEqual([]);
    expect(findOrphan(outOfScope, syntheticReference, ["remote."])).toEqual([]);

    // The real covered set is non-empty too.
    expect(covered.size).toBeGreaterThan(0);
  });
});
