import { beforeEach, expect, it } from "vitest";
import { readStoredView, storeView, VIEW_STORAGE_KEY } from "@/lib/navigation";

beforeEach(() => localStorage.clear());

it("returns to Apps after restart rather than restoring an unselected custom app", () => {
  storeView("customApps");
  expect(readStoredView()).toBe("apps");
});

it("handles a custom app view saved by the earlier prototype", () => {
  localStorage.setItem(VIEW_STORAGE_KEY, "customApps");
  expect(readStoredView()).toBe("apps");
});
