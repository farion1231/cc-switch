import { createInstance } from "i18next";
import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";

describe("Antigravity usage limitations", () => {
  it.each(["en", "ja", "zh", "zh-TW"])(
    "resolves the dashboard hint from real %s resources",
    async (lng) => {
      const i18n = createInstance();
      await i18n.init({
        lng,
        fallbackLng: "en",
        resources: {
          en: { translation: en },
          ja: { translation: ja },
          zh: { translation: zh },
          "zh-TW": { translation: zhTW },
        },
      });
      const text = i18n.t("usage.agyUsageHint");
      const expected = { en, ja, zh, "zh-TW": zhTW }[
        lng as "en" | "ja" | "zh" | "zh-TW"
      ];
      expect(text).toBe(expected.usage.agyUsageHint);
      expect(text).toContain("unknown");
      expect(text).toContain("0");
      expect(text).not.toBe("usage.agyUsageHint");
    },
  );
});
