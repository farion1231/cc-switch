import { afterEach, describe, expect, it, vi } from "vitest";
import { deepClone } from "@/utils/deepClone";

describe("deepClone", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("falls back when structuredClone is unavailable", () => {
    vi.stubGlobal("structuredClone", undefined);

    const source = {
      nested: { value: "original" },
      list: [{ enabled: true }],
      createdAt: new Date("2026-01-30T00:00:00.000Z"),
    };

    const cloned = deepClone(source);
    cloned.nested.value = "changed";
    cloned.list[0].enabled = false;

    expect(cloned).not.toBe(source);
    expect(cloned.nested).not.toBe(source.nested);
    expect(cloned.list).not.toBe(source.list);
    expect(cloned.createdAt).not.toBe(source.createdAt);
    expect(cloned.createdAt.getTime()).toBe(source.createdAt.getTime());
    expect(source.nested.value).toBe("original");
    expect(source.list[0].enabled).toBe(true);
  });
});
