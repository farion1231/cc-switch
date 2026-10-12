import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

beforeEach(() => {
  vi.resetModules();
  localStorage.clear();
});
afterEach(() => {
  vi.unstubAllGlobals();
  localStorage.clear();
});

describe("Turkish language detection", () => {
  it.each(["tr", "tr-TR"])("detects %s on first launch", async (language) => {
    vi.stubGlobal("navigator", { language });
    const { default: i18n } = await import("@/i18n");
    expect(i18n.language).toBe("tr");
    expect(i18n.t("common.save")).toBe("Kaydet");
  });
  it("keeps a saved language instead of overriding it with the system language", async () => {
    localStorage.setItem("language", "en");
    vi.stubGlobal("navigator", { language: "tr-TR" });
    const { default: i18n } = await import("@/i18n");
    expect(i18n.language).toBe("en");
  });
});
