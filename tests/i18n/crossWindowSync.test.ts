import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import i18n, {
  LANGUAGE_CHANGED_EVENT,
  initI18nCrossWindowSync,
  isSupportedLanguage,
  normalizeLanguage,
} from "@/i18n";
import { emitTauriEvent } from "../msw/tauriMocks";
import * as eventModule from "@tauri-apps/api/event";

describe("i18n cross-window synchronization", () => {
  let cleanup: (() => void) | undefined;
  let emitSpy: ReturnType<typeof vi.spyOn<any, any>>;

  beforeEach(async () => {
    window.localStorage.clear();
    await i18n.changeLanguage("zh");
    emitSpy = vi.spyOn(eventModule, "emit");
    emitSpy.mockClear();
    cleanup = initI18nCrossWindowSync();
  });

  afterEach(async () => {
    cleanup?.();
    emitSpy.mockRestore();
    await i18n.changeLanguage("zh");
    window.localStorage.clear();
  });

  describe("language helpers", () => {
    it("identifies supported languages correctly", () => {
      expect(isSupportedLanguage("zh")).toBe(true);
      expect(isSupportedLanguage("zh-TW")).toBe(true);
      expect(isSupportedLanguage("en")).toBe(true);
      expect(isSupportedLanguage("ja")).toBe(true);
      expect(isSupportedLanguage("zh_CN")).toBe(true);
      expect(isSupportedLanguage("fr")).toBe(false);
      expect(isSupportedLanguage(null)).toBe(false);
      expect(isSupportedLanguage(undefined)).toBe(false);
    });

    it("normalizes languages and aliases correctly", () => {
      expect(normalizeLanguage("zh")).toBe("zh");
      expect(normalizeLanguage("zh-cn")).toBe("zh");
      expect(normalizeLanguage("zh_TW")).toBe("zh-TW");
      expect(normalizeLanguage("zh-Hant")).toBe("zh-TW");
      expect(normalizeLanguage("zh-HK")).toBe("zh-TW");
      expect(normalizeLanguage("en-US")).toBe("en");
      expect(normalizeLanguage("ja-JP")).toBe("ja");
      expect(normalizeLanguage("unsupported")).toBe("zh");
      expect(normalizeLanguage(null)).toBe("zh");
    });
  });

  describe("cross-window event sync", () => {
    it("updates local i18n and localStorage when receiving language-changed event", async () => {
      expect(i18n.language).toBe("zh");

      // 模拟其他窗口广播 language-changed 事件
      emitTauriEvent(LANGUAGE_CHANGED_EVENT, "en");

      // 等待 i18n 变更生效
      expect(i18n.language).toBe("en");
      expect(window.localStorage.getItem("language")).toBe("en");
    });

    it("does not re-emit when language was updated from incoming event", async () => {
      emitSpy.mockClear();

      // 模拟接收外部事件
      emitTauriEvent(LANGUAGE_CHANGED_EVENT, "ja");

      expect(i18n.language).toBe("ja");
      expect(window.localStorage.getItem("language")).toBe("ja");

      // 不应反向广播，避免回环
      expect(emitSpy).not.toHaveBeenCalledWith(LANGUAGE_CHANGED_EVENT, "ja");
    });

    it("broadcasts language-changed event and updates localStorage when local language changes", async () => {
      emitSpy.mockClear();

      await i18n.changeLanguage("zh-TW");

      expect(window.localStorage.getItem("language")).toBe("zh-TW");
      expect(emitSpy).toHaveBeenCalledWith(LANGUAGE_CHANGED_EVENT, "zh-TW");
    });
  });
});
