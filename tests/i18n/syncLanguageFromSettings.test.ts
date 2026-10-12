import { afterEach, describe, expect, it } from "vitest";
import i18n, { syncLanguageFromSettings } from "@/i18n";

describe("syncLanguageFromSettings", () => {
  afterEach(async () => {
    localStorage.removeItem("language");
    await i18n.changeLanguage("en");
  });

  it("switches to the saved language and refills the cache when localStorage is empty", async () => {
    await i18n.changeLanguage("en");
    localStorage.removeItem("language");

    syncLanguageFromSettings("zh");

    expect(i18n.language).toBe("zh");
    expect(localStorage.getItem("language")).toBe("zh");
  });

  it("overrides a stale cached language", async () => {
    await i18n.changeLanguage("ja");
    localStorage.setItem("language", "ja");

    syncLanguageFromSettings("zh-TW");

    expect(i18n.language).toBe("zh-TW");
    expect(localStorage.getItem("language")).toBe("zh-TW");
  });

  it("leaves the current language alone when nothing is saved", async () => {
    await i18n.changeLanguage("en");

    syncLanguageFromSettings(undefined);
    syncLanguageFromSettings("fr");

    expect(i18n.language).toBe("en");
    expect(localStorage.getItem("language")).toBeNull();
  });
});
