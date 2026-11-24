import { describe, expect, it } from "vitest";
import {
  ALL_MOBILE_VIEWS,
  DEFAULT_PINNED_VIEWS,
  MIN_PINNED_SHORTCUTS,
  SHORTCUTS_STORAGE_KEY,
  getUnpinnedViews,
  loadShortcuts,
  maxPinnedShortcuts,
  moveShortcut,
  normalizePinnedShortcuts,
  reorderShortcut,
  resetShortcuts,
  saveShortcuts,
  toggleShortcutPin,
} from "@/mobile/shortcuts";

function createMemoryStorage() {
  const map = new Map<string, string>();
  return {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => {
      map.set(k, v);
    },
  };
}

describe("mobile customizable bottom shortcut bar", () => {
  it("computes sensible max pinned limits based on total view count", () => {
    expect(ALL_MOBILE_VIEWS.length).toBe(7);
    // 7 views > 5 bar slots -> reserve 1 slot for "More", so max 4 pinned
    expect(maxPinnedShortcuts(7)).toBe(4);
    expect(maxPinnedShortcuts(6)).toBe(4);
    // When all views fit in 5 slots, allow pinning all of them
    expect(maxPinnedShortcuts(5)).toBe(5);
    expect(maxPinnedShortcuts(3)).toBe(3);
  });

  it("toggles pin/unpin while enforcing min 1 and max 4 bounds", () => {
    // Default starts at 4 pinned (full)
    expect(DEFAULT_PINNED_VIEWS.length).toBe(4);
    expect(getUnpinnedViews(DEFAULT_PINNED_VIEWS)).toEqual([
      "remote",
      "models",
      "preferences",
    ]);

    // Attempting to pin a 5th view while already at max (4) is a no-op
    expect(toggleShortcutPin(DEFAULT_PINNED_VIEWS, "remote")).toEqual([
      ...DEFAULT_PINNED_VIEWS,
    ]);

    // Unpinning one makes room to pin another (including "remote")
    const afterRemove = toggleShortcutPin(DEFAULT_PINNED_VIEWS, "proxy");
    expect(afterRemove).toEqual(["overview", "providers", "logs"]);
    const afterPin = toggleShortcutPin(afterRemove, "remote");
    expect(afterPin).toEqual(["overview", "providers", "logs", "remote"]);

    // Cannot unpin below MIN_PINNED_SHORTCUTS (1)
    expect(MIN_PINNED_SHORTCUTS).toBe(1);
    expect(toggleShortcutPin(["overview"], "overview")).toEqual(["overview"]);
  });

  it("reorders pinned tabs via arrow moves and drag indices", () => {
    const start = ["overview", "providers", "logs", "proxy"] as const;

    // Move earlier / later
    expect(moveShortcut(start, "logs", -1)).toEqual([
      "overview",
      "logs",
      "providers",
      "proxy",
    ]);
    expect(moveShortcut(start, "overview", 1)).toEqual([
      "providers",
      "overview",
      "logs",
      "proxy",
    ]);

    // Out-of-bounds moves are safe no-ops
    expect(moveShortcut(start, "overview", -1)).toEqual([...start]);
    expect(moveShortcut(start, "proxy", 1)).toEqual([...start]);

    // Drag-and-drop index reorder
    expect(reorderShortcut(start, 3, 0)).toEqual([
      "proxy",
      "overview",
      "providers",
      "logs",
    ]);
    expect(reorderShortcut(start, 0, 2)).toEqual([
      "providers",
      "logs",
      "overview",
      "proxy",
    ]);
  });

  it("persists to localStorage and recovers safely from invalid or corrupt data", () => {
    const storage = createMemoryStorage();
    expect(loadShortcuts(storage)).toEqual([...DEFAULT_PINNED_VIEWS]);

    saveShortcuts(["logs", "models", "overview"], storage);
    expect(loadShortcuts(storage)).toEqual(["logs", "models", "overview"]);

    // Deduplicates, strips invalid view IDs, and caps at maxPinned
    expect(
      normalizePinnedShortcuts([
        "models",
        "not-a-view",
        "models",
        "logs",
        "proxy",
        "preferences",
        "overview",
      ]),
    ).toEqual(["models", "logs", "proxy", "preferences"]);

    // Empty or corrupt payloads fall back to defaults
    storage.setItem(SHORTCUTS_STORAGE_KEY, "{not valid json");
    expect(loadShortcuts(storage)).toEqual([...DEFAULT_PINNED_VIEWS]);

    storage.setItem(
      SHORTCUTS_STORAGE_KEY,
      JSON.stringify({ version: 1, pinned: ["invalid"] }),
    );
    expect(loadShortcuts(storage)).toEqual([...DEFAULT_PINNED_VIEWS]);

    expect(resetShortcuts()).toEqual([...DEFAULT_PINNED_VIEWS]);
  });
});
