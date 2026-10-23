import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import i18n from "@/i18n";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import tr from "@/i18n/locales/tr.json";
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

describe("custom Codex catalog errors", () => {
  const path = "/tmp/a&b<team>/models.json";
  const catalogError = (code: string, context: Record<string, unknown> = {}) =>
    `供应商模型目录读取失败 (Provider model catalog failed): CC_SWITCH_CODEX_CATALOG_ERROR:${JSON.stringify({ code, context: { path, ...context } })}`;

  beforeAll(() => {
    for (const [language, resource] of Object.entries({
      en,
      ja,
      tr,
      zh,
      "zh-TW": zhTW,
    })) {
      i18n.addResourceBundle(language, "translation", resource, true, true);
    }
  });

  afterEach(async () => {
    await i18n.changeLanguage("zh");
  });

  it.each([
    ["zh", "无法读取 Codex 自定义模型目录"],
    ["zh-TW", "無法讀取 Codex 自訂模型目錄"],
    ["en", "Cannot read the custom Codex model catalog"],
    ["ja", "Codex のカスタムモデルカタログを読み取れません"],
    ["tr", "Özel Codex model kataloğu okunamadı"],
  ])(
    "uses the current %s language for wrapped Tauri errors",
    async (language, expected) => {
      await i18n.changeLanguage(language);
      const result = extractErrorMessage({
        partial: false,
        message: catalogError("readFailed", { detail: "Permission denied" }),
      });
      expect(result).toContain(expected);
      expect(result).toContain(path);
      expect(result).toContain("Permission denied");
      expect(result).not.toContain("Provider model catalog failed");
      expect(result).not.toContain("CC_SWITCH_CODEX_CATALOG_ERROR");
    },
  );

  it.each([
    [
      "invalidConfig",
      { detail: "line 2" },
      "invalid Codex config.toml. line 2",
    ],
    ["invalidJson", { detail: "line 3" }, "invalid JSON"],
    ["invalidModels", {}, "must contain a models array"],
    [
      "invalidSlug",
      { index: 0 },
      "models[0] must be an object with a non-empty slug string",
    ],
  ])(
    "translates %s with its diagnostic context",
    async (code, context, expected) => {
      await i18n.changeLanguage("en");
      expect(
        extractErrorMessage(new Error(catalogError(code, context))),
      ).toContain(expected);
    },
  );

  it("reflects language changes without recreating the error", async () => {
    const error = catalogError("invalidModels");
    await i18n.changeLanguage("en");
    expect(extractErrorMessage(error)).toContain("must contain a models array");
    await i18n.changeLanguage("zh");
    expect(extractErrorMessage(error)).toContain("必须包含 models 数组");
  });

  it.each([
    "ordinary backend error",
    "CC_SWITCH_CODEX_CATALOG_ERROR:not JSON",
    catalogError("unknownCode"),
    'CC_SWITCH_CODEX_CATALOG_ERROR:{"code":"readFailed","context":null}',
  ])("preserves unrecognized errors: %s", (error) => {
    expect(extractErrorMessage(error)).toBe(error);
  });
});
