import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

beforeEach(() => {
  vi.resetModules();
  localStorage.clear();
});
afterEach(() => {
  vi.unstubAllGlobals();
  localStorage.clear();
});

describe("Korean language", () => {
  it.each(["ko", "ko-KR"])("detects %s on first launch", async (language) => {
    vi.stubGlobal("navigator", { language });
    const { default: i18n, i18nReady } = await import("@/i18n");
    await i18nReady;
    expect(i18n.language).toBe("ko");
    expect(i18n.t("common.save")).toBe("저장");
  });
  it("falls back to English for Konkani instead of selecting Korean", async () => {
    vi.stubGlobal("navigator", { language: "kok-IN" });
    const { default: i18n, i18nReady } = await import("@/i18n");
    await i18nReady;
    expect(i18n.language).toBe("en");
    expect(i18n.t("common.save")).toBe("Save");
  });
  it("restores Korean from backend settings and caches it for restart", async () => {
    vi.stubGlobal("navigator", { language: "en-US" });
    const {
      default: i18n,
      i18nReady,
      syncLanguageFromSettings,
    } = await import("@/i18n");
    await i18nReady;
    await syncLanguageFromSettings("ko");
    expect(i18n.language).toBe("ko");
    expect(localStorage.getItem("language")).toBe("ko");
    vi.resetModules();
    const restarted = await import("@/i18n");
    await restarted.i18nReady;
    expect(restarted.default.language).toBe("ko");
  });
  it("respects a saved language on a Korean system", async () => {
    localStorage.setItem("language", "ja");
    vi.stubGlobal("navigator", { language: "ko-KR" });
    const { default: i18n, i18nReady } = await import("@/i18n");
    await i18nReady;
    expect(i18n.language).toBe("ja");
  });
});
