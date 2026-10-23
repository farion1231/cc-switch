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
    const { default: i18n } = await import("@/i18n");
    expect(i18n.language).toBe("ko");
    expect(i18n.t("common.save")).toBe("저장");
  });
  it("restores Korean from backend settings and caches it for restart", async () => {
    vi.stubGlobal("navigator", { language: "en-US" });
    const { default: i18n, syncLanguageFromSettings } = await import("@/i18n");
    syncLanguageFromSettings("ko");
    expect(i18n.language).toBe("ko");
    expect(localStorage.getItem("language")).toBe("ko");
    vi.resetModules();
    const restarted = await import("@/i18n");
    expect(restarted.default.language).toBe("ko");
  });
  it("respects a saved language on a Korean system", async () => {
    localStorage.setItem("language", "ja");
    vi.stubGlobal("navigator", { language: "ko-KR" });
    const { default: i18n } = await import("@/i18n");
    expect(i18n.language).toBe("ja");
  });
});
