import { afterEach, describe, expect, it } from "vitest";
import i18n, { i18nReady, syncLanguageFromSettings } from "@/i18n";

describe("syncLanguageFromSettings", () => {
  afterEach(async () => {
    await i18nReady;
    localStorage.removeItem("language");
    await i18n.changeLanguage("en");
  });

  it("switches to the saved language and refills the cache when localStorage is empty", async () => {
    await i18nReady;
    await i18n.changeLanguage("en");
    localStorage.removeItem("language");

    await syncLanguageFromSettings("zh");

    expect(i18n.language).toBe("zh");
    expect(localStorage.getItem("language")).toBe("zh");
  });

  it("overrides a stale cached language", async () => {
    await i18nReady;
    await i18n.changeLanguage("ja");
    localStorage.setItem("language", "ja");

    await syncLanguageFromSettings("zh-TW");

    expect(i18n.language).toBe("zh-TW");
    expect(localStorage.getItem("language")).toBe("zh-TW");
  });

  it("restores Turkish and its translated labels from saved settings", async () => {
    await i18nReady;
    await i18n.changeLanguage("en");
    await syncLanguageFromSettings("tr");
    expect(i18n.language).toBe("tr");
    expect(localStorage.getItem("language")).toBe("tr");
  });

  it("leaves the current language alone when nothing is saved", async () => {
    await i18nReady;
    await i18n.changeLanguage("en");

    await syncLanguageFromSettings(undefined);
    await syncLanguageFromSettings("fr");

    expect(i18n.language).toBe("en");
    expect(localStorage.getItem("language")).toBeNull();
  });

  it("loads locales lazily on demand and preserves translations across roundtrip switches", async () => {
    await i18nReady;
    expect(i18n.hasResourceBundle("de", "translation")).toBe(false);
    expect(i18n.hasResourceBundle("ru", "translation")).toBe(false);

    await syncLanguageFromSettings("zh");
    const zhSave = i18n.t("common.save");
    expect(i18n.hasResourceBundle("zh", "translation")).toBe(true);

    await i18n.changeLanguage("ko");
    expect(i18n.hasResourceBundle("ko", "translation")).toBe(true);
    expect(i18n.t("common.save")).not.toBe(zhSave);

    await i18n.changeLanguage("zh");
    expect(i18n.t("common.save")).toBe(zhSave);
    expect(i18n.hasResourceBundle("de", "translation")).toBe(false);
    expect(i18n.hasResourceBundle("ru", "translation")).toBe(false);
  });
});
