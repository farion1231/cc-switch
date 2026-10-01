import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import ja from "@/i18n/locales/ja.json";

function leafKeys(value: Record<string, unknown>, prefix = ""): string[] {
  return Object.entries(value)
    .flatMap(([key, entry]) => {
      const path = prefix ? `${prefix}.${key}` : key;
      return typeof entry === "object" && entry !== null
        ? leafKeys(entry as Record<string, unknown>, path)
        : [path];
    })
    .sort();
}

describe("VPS translations", () => {
  it.each(Object.entries({ zh, "zh-TW": zhTW, ja }))(
    "has all VPS keys in %s",
    (_language, locale) => {
      expect(leafKeys(locale.vps)).toEqual(leafKeys(en.vps));
    },
  );
});
