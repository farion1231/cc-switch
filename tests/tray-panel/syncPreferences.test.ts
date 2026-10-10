import { describe, expect, it, vi } from "vitest";
import {
  isPreferenceKey,
  syncPreferencesFromStorage,
} from "@/tray-panel/syncPreferences";

function storageOf(values: Record<string, string>) {
  return { getItem: (key: string) => values[key] ?? null };
}

describe("托盘面板同步主界面的主题和语言", () => {
  it("主界面改了语言后，面板跟着切换", () => {
    const changeLanguage = vi.fn();
    syncPreferencesFromStorage(storageOf({ language: "en" }), {
      setTheme: vi.fn(),
      currentLanguage: "zh",
      changeLanguage,
    });
    expect(changeLanguage).toHaveBeenCalledWith("en");
  });

  it("语言没变或不认识时不切换", () => {
    const changeLanguage = vi.fn();
    syncPreferencesFromStorage(storageOf({ language: "en" }), {
      setTheme: vi.fn(),
      currentLanguage: "en",
      changeLanguage,
    });
    syncPreferencesFromStorage(storageOf({ language: "fr" }), {
      setTheme: vi.fn(),
      currentLanguage: "zh",
      changeLanguage,
    });
    expect(changeLanguage).not.toHaveBeenCalled();
  });

  it("按存下的主题套用，非法值忽略", () => {
    const setTheme = vi.fn();
    const targets = {
      setTheme,
      currentLanguage: "zh",
      changeLanguage: vi.fn(),
    };
    syncPreferencesFromStorage(
      storageOf({ "cc-switch-theme": "dark" }),
      targets,
    );
    syncPreferencesFromStorage(
      storageOf({ "cc-switch-theme": "blue" }),
      targets,
    );
    expect(setTheme).toHaveBeenCalledTimes(1);
    expect(setTheme).toHaveBeenCalledWith("dark");
  });

  it("storage 事件只关心主题和语言两个键", () => {
    expect(isPreferenceKey("language")).toBe(true);
    expect(isPreferenceKey("cc-switch-theme")).toBe(true);
    expect(isPreferenceKey("other")).toBe(false);
    expect(isPreferenceKey(null)).toBe(false);
  });
});
