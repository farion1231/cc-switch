import { describe, expect, it } from "vitest";

describe("browser storage test environment", () => {
  it("uses browser Storage semantics for enumeration and prototype mocks", () => {
    const key = "cc-switch.usage.storage-environment-test";
    try {
      expect(localStorage).toBe(window.localStorage);
      expect(localStorage).toBeInstanceOf(Storage);
      localStorage.setItem(key, "test");
      expect(Object.keys(localStorage)).toContain(key);
    } finally {
      localStorage.removeItem(key);
    }
  });
});
