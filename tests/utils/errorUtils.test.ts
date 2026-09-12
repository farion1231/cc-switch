import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import i18n from "i18next";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import {
  extractErrorMessage,
  translatePiProviderMutationError,
} from "@/utils/errorUtils";

describe("error utilities", () => {
  it("extracts Tauri string errors", () => {
    expect(extractErrorMessage("backend failed")).toBe("backend failed");
  });

  it("maps a simultaneous models.json write to a concise error", () => {
    const t = vi.fn((key: string) => key);

    expect(
      translatePiProviderMutationError(
        "Pi models.json changed outside CC Switch",
        t,
      ),
    ).toBe("pi.provider.writeConflict");
  });

  it("maps a duplicate Pi provider key to validation feedback", () => {
    const t = vi.fn((key: string) => key);

    expect(
      translatePiProviderMutationError(
        "无效输入: Pi provider key 'duplicate' already exists in models.json",
        t,
      ),
    ).toBe("pi.form.providerKeyDuplicate");
  });
});

describe("Codex takeover cleanup error translations", () => {
  const locales = { en, ja, zh, "zh-TW": zhTW };
  const errors = [
    {
      key: "proxy.takeover.cleanup.officialAuthFallback",
      source:
        "无法安全清理 Codex 接管配置：清理会让保留的地址回退使用官方认证。请先恢复原配置，或为该地址配置独立认证。",
    },
    {
      key: "proxy.takeover.cleanup.unverifiedProfiles",
      source:
        "无法安全清理 Codex 接管配置：存在 profile 或无法检查配置目录，不能排除官方认证回退。请先恢复原配置并检查相关 profile。",
    },
  ];
  const resourceText = (resource: unknown, key: string): string => {
    const value = key.split(".").reduce<unknown>((current, part) => {
      if (!current || typeof current !== "object") return undefined;
      return (current as Record<string, unknown>)[part];
    }, resource);
    if (typeof value !== "string" || !value.trim()) {
      throw new Error(`Missing translation resource: ${key}`);
    }
    return value;
  };

  beforeAll(() => {
    for (const [language, resource] of Object.entries(locales)) {
      i18n.addResourceBundle(language, "translation", resource, true, true);
    }
  });

  afterAll(async () => {
    await i18n.changeLanguage("zh");
  });

  it.each(Object.entries(locales))(
    "uses actual %s resources for both backend errors, not missing-key fallback",
    async (language, resource) => {
      await i18n.changeLanguage(language);
      for (const { key, source } of errors) {
        const expected = resourceText(resource, key);
        expect(expected).not.toBe(key);
        if (language !== "zh") expect(expected).not.toBe(source);
        expect(extractErrorMessage(source)).toBe(expected);
      }
    },
  );

  it("translates every supported error envelope without losing its contents", async () => {
    await i18n.changeLanguage("en");
    const { key, source } = errors[0];
    const expected = resourceText(en, key);
    for (const error of [
      source,
      new Error(source),
      { message: source },
      { error: source },
      { detail: source },
      { payload: source },
      { payload: { message: source } },
      { payload: { error: source } },
      { payload: { detail: source } },
    ]) {
      expect(extractErrorMessage(error)).toBe(expected);
    }
  });

  it("observes runtime language changes instead of caching the first translation", async () => {
    for (const language of ["en", "ja", "zh-TW"] as const) {
      await i18n.changeLanguage(language);
      expect(extractErrorMessage(errors[1].source)).toBe(
        resourceText(locales[language], errors[1].key),
      );
    }
  });

  it("replaces repeated cleanup messages and retains unrelated aggregate errors", async () => {
    await i18n.changeLanguage("en");
    const [auth, profiles] = errors;
    const prefix = "恢复失败: Codex: ";
    const suffix = "; Claude: permission denied; Gemini: 无法读取配置";
    expect(
      extractErrorMessage(
        `${prefix}${auth.source}; ${profiles.source}; ${auth.source}${suffix}`,
      ),
    ).toBe(
      `${prefix}${resourceText(en, auth.key)}; ${resourceText(en, profiles.key)}; ${resourceText(en, auth.key)}${suffix}`,
    );
  });

  it("retains the existing extraction behavior for unknown and empty errors", () => {
    for (const [error, expected] of [
      ["unrelated backend error", "unrelated backend error"],
      ["官方认证: unrelated error", "官方认证: unrelated error"],
      ["  ", "  "],
      [new Error("unknown failure"), "unknown failure"],
      [{ message: "first", error: "second" }, "first"],
      [{ message: "", error: "not selected" }, ""],
      [{ message: " ", payload: " nested error " }, " nested error "],
      [{ payload: { detail: "unknown nested error" } }, "unknown nested error"],
      [new Error(" "), ""],
      [null, ""],
      [undefined, ""],
      [false, ""],
      [0, ""],
      [{}, ""],
    ] as const) {
      expect(extractErrorMessage(error)).toBe(expected);
    }
  });

  it("keeps the translated message contract aligned with the Rust validator", () => {
    const source = readFileSync(
      resolve(__dirname, "../../src-tauri/src/codex_config.rs"),
      "utf8",
    );
    const start = source.indexOf(
      "pub(crate) fn validate_codex_takeover_cleanup(",
    );
    const end = source.indexOf(
      "pub(crate) fn remove_codex_temporary_takeover_route(",
    );
    expect(start).toBeGreaterThanOrEqual(0);
    expect(end).toBeGreaterThan(start);
    for (const error of errors) {
      expect(source.slice(start, end)).toContain(error.source);
    }
  });
});
