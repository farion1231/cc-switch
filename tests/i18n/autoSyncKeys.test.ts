import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";

const locales = [
  ["en", en],
  ["ja", ja],
  ["zh-TW", zhTW],
  ["zh", zh],
] as const;

describe("auto sync locale coverage", () => {
  it.each(locales)(
    "defines autoSyncContextWindowTooltipOn in %s",
    (_locale, translations) => {
      const value = translations.providerForm.autoSyncContextWindowTooltipOn;

      expect(typeof value).toBe("string");
      expect(value.trim().length).toBeGreaterThan(0);
    },
  );

  it.each(locales)(
    "defines providerForm.liveLoadFailed in %s",
    (_locale, translations) => {
      const value = translations.providerForm.liveLoadFailed;

      expect(typeof value).toBe("string");
      expect(value.trim().length).toBeGreaterThan(0);
    },
  );
});
