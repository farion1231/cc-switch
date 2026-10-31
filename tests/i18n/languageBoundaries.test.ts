import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

beforeEach(() => {
  vi.resetModules();
  localStorage.clear();
});

afterEach(() => {
  vi.unstubAllGlobals();
  localStorage.clear();
});

describe("system language subtag boundaries", () => {
  it.each([
    ["zh", "zh"],
    ["zh-CN", "zh"],
    ["zh-TW", "zh-TW"],
    ["zh-TW-u-nu-hanidec", "zh-TW"],
    ["zh-HK", "zh-TW"],
    ["zh-MO", "zh-TW"],
    ["zh-Hant-TW", "zh-TW"],
    ["zh-Hans-CN", "zh"],
    ["ja", "ja"],
    ["ja-JP", "ja"],
    ["tr", "tr"],
    ["tr-TR", "tr"],
    ["en", "en"],
    ["en-US", "en"],
    ["zha", "en"],
    ["zhb-CN", "en"],
    ["jam", "en"],
    ["trv-TW", "en"],
    ["zh-hantx", "zh"],
    ["zh-hkx", "zh"],
  ])("maps %s to %s on first launch", async (language, expected) => {
    vi.stubGlobal("navigator", { language });
    const { default: i18n } = await import("@/i18n");
    expect(i18n.language).toBe(expected);
  });
});
