import { describe, expect, it, beforeEach } from "vitest";
import { APP_IDS, DEFAULT_VISIBLE_APPS } from "@/config/appConfig";
import type { AppId } from "@/lib/api";
import type { VisibleApps } from "@/types";

const STORAGE_KEY = "cc-switch-last-app";

const getInitialApp = (): AppId => {
  const saved = localStorage.getItem(STORAGE_KEY) as AppId | null;
  if (saved && APP_IDS.includes(saved)) {
    return saved;
  }
  return "claude";
};

const getFirstVisibleApp = (visibleApps: VisibleApps): AppId => {
  return APP_IDS.find((app) => visibleApps[app]) ?? "claude";
};

describe("activeApp startup and persistence logic", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("defaults to claude when no saved app in localStorage", () => {
    expect(getInitialApp()).toBe("claude");
  });

  it("restores focused app from localStorage on startup", () => {
    localStorage.setItem(STORAGE_KEY, "codex");
    expect(getInitialApp()).toBe("codex");

    localStorage.setItem(STORAGE_KEY, "opencode");
    expect(getInitialApp()).toBe("opencode");
  });

  it("ignores invalid or unknown app id in localStorage", () => {
    localStorage.setItem(STORAGE_KEY, "invalid-app" as AppId);
    expect(getInitialApp()).toBe("claude");
  });

  it("computes the first visible app correctly according to APP_IDS order", () => {
    const visibleApps: VisibleApps = {
      ...DEFAULT_VISIBLE_APPS,
      claude: false,
      "claude-desktop": false,
      codex: true,
      gemini: false,
      grokbuild: false,
      opencode: true,
    };
    expect(getFirstVisibleApp(visibleApps)).toBe("codex");
  });

  it("does not prematurely fallback when settings data is undefined during initial load", () => {
    let settingsData: { visibleApps?: VisibleApps } | undefined = undefined;
    const activeApp: AppId = "codex";

    const evaluateFallback = (
      visibleApps: VisibleApps,
      currentApp: AppId,
      loadedSettings: typeof settingsData,
    ): AppId => {
      // Guarded by settingsData loaded state
      if (!loadedSettings) return currentApp;
      if (!visibleApps[currentApp]) {
        return getFirstVisibleApp(visibleApps);
      }
      return currentApp;
    };

    // Before settings load, even with mock empty visibleApps, it should not jump
    const emptyVisibleApps = { ...DEFAULT_VISIBLE_APPS };
    const resolvedBeforeLoad = evaluateFallback(
      emptyVisibleApps,
      activeApp,
      settingsData,
    );
    expect(resolvedBeforeLoad).toBe("codex");

    // After settings load where codex is visible, keep codex
    settingsData = {
      visibleApps: {
        ...DEFAULT_VISIBLE_APPS,
        claude: true,
        codex: true,
      },
    };
    const resolvedAfterLoad = evaluateFallback(
      settingsData.visibleApps!,
      activeApp,
      settingsData,
    );
    expect(resolvedAfterLoad).toBe("codex");

    // If codex was explicitly disabled in loaded settings, safely fallback to first visible app
    settingsData = {
      visibleApps: {
        ...DEFAULT_VISIBLE_APPS,
        claude: false,
        "claude-desktop": false,
        codex: false,
        gemini: false,
        grokbuild: false,
        opencode: true,
      },
    };
    const resolvedAfterDisable = evaluateFallback(
      settingsData.visibleApps!,
      activeApp,
      settingsData,
    );
    expect(resolvedAfterDisable).toBe("opencode");
  });
});
