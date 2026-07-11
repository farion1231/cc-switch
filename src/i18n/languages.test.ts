import { describe, expect, it } from "vitest";
import { DEFAULT_LANGUAGE, normalizeLanguage } from "./languages";

describe("normalizeLanguage", () => {
  it("keeps exact supported language tags", () => {
    expect(normalizeLanguage("zh")).toBe("zh");
    expect(normalizeLanguage("zh-TW")).toBe("zh-TW");
    expect(normalizeLanguage("en")).toBe("en");
    expect(normalizeLanguage("ja")).toBe("ja");
    expect(normalizeLanguage("ru")).toBe("ru");
  });

  it("normalizes common regional language tags", () => {
    expect(normalizeLanguage("en-US")).toBe("en");
    expect(normalizeLanguage("ja-JP")).toBe("ja");
    expect(normalizeLanguage("ru-RU")).toBe("ru");
  });

  it("normalizes traditional Chinese variants", () => {
    expect(normalizeLanguage("zh_Hant")).toBe("zh-TW");
    expect(normalizeLanguage("zh-Hant-TW")).toBe("zh-TW");
    expect(normalizeLanguage("zh-HK")).toBe("zh-TW");
    expect(normalizeLanguage("zh-MO")).toBe("zh-TW");
  });

  it("falls back to simplified Chinese for other Chinese variants", () => {
    expect(normalizeLanguage("zh-CN")).toBe("zh");
    expect(normalizeLanguage("zh-Hans-CN")).toBe("zh");
  });

  it("falls back to the default language for empty or unsupported values", () => {
    expect(normalizeLanguage()).toBe(DEFAULT_LANGUAGE);
    expect(normalizeLanguage("")).toBe(DEFAULT_LANGUAGE);
    expect(normalizeLanguage("de-DE")).toBe(DEFAULT_LANGUAGE);
  });
});
