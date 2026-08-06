import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";

describe("prompt locale coverage", () => {
  it.each([
    ["en", en],
    ["ja", ja],
    ["zh", zh],
    ["zh-TW", zhTW],
  ])(
    "defines the enabled prompt count label in %s",
    (_locale, translations) => {
      expect(translations.prompts.enabledCount).toEqual(expect.any(String));
      expect(translations.prompts.enabledCount.trim()).not.toBe("");
    },
  );
});
