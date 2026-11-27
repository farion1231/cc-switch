import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  DEFAULT_AUTH_CENTER_PREFERENCES,
  readAuthCenterPreferences,
  updateAuthCenterPreferences,
  useAuthCenterPreferences,
} from "@/lib/authCenterPreferences";

const STORAGE_KEY = "cc-switch.authCenter.apps";

beforeEach(() => {
  localStorage.removeItem(STORAGE_KEY);
  readAuthCenterPreferences();
});

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.removeItem(STORAGE_KEY);
});

describe("authCenterPreferences", () => {
  it("repairs outdated preferences while preserving known order and visibility", () => {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        order: ["codex_oauth", "unknown", "claude_official", "codex_oauth"],
        hidden: ["github_copilot", "unknown", "github_copilot"],
      }),
    );
    const saved = readAuthCenterPreferences();
    expect(saved.order.slice(0, 2)).toEqual(["codex_oauth", "claude_official"]);
    expect(saved.order).toHaveLength(
      DEFAULT_AUTH_CENTER_PREFERENCES.order.length,
    );
    expect(saved.hidden).toEqual(["github_copilot"]);
    expect(saved.order).toContain("workbuddy_account");
    expect(saved.order).toContain("codebuddy_oauth");
    expect(saved.customOrder).toBe(true);
  });

  it.each(["{broken", "null", "42", '{"order":false,"hidden":{}}'])(
    "falls back safely for invalid preferences: %s",
    (raw) => {
      localStorage.setItem(STORAGE_KEY, raw);
      expect(readAuthCenterPreferences()).toEqual(
        DEFAULT_AUTH_CENTER_PREFERENCES,
      );
    },
  );

  it("keeps multiple mounted panels and another window in sync", () => {
    const first = renderHook(() => useAuthCenterPreferences());
    const second = renderHook(() => useAuthCenterPreferences());
    act(() =>
      updateAuthCenterPreferences((current) => ({
        ...current,
        hidden: ["claude_official"],
      })),
    );
    expect(first.result.current.hidden).toEqual(["claude_official"]);
    expect(second.result.current.hidden).toEqual(["claude_official"]);
    expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toEqual(
      first.result.current,
    );

    act(() => {
      localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({ hidden: ["xai_oauth"] }),
      );
      window.dispatchEvent(new StorageEvent("storage", { key: STORAGE_KEY }));
    });
    expect(first.result.current.hidden).toEqual(["xai_oauth"]);
    expect(second.result.current.hidden).toEqual(["xai_oauth"]);
  });

  it("keeps changes in memory if local storage rejects writes", async () => {
    vi.resetModules();
    const isolated = await import("@/lib/authCenterPreferences");
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new DOMException("Storage unavailable", "QuotaExceededError");
    });
    isolated.updateAuthCenterPreferences((current) => ({
      ...current,
      hidden: ["kimi_oauth"],
    }));
    expect(isolated.readAuthCenterPreferences().hidden).toEqual(["kimi_oauth"]);
  });
});
