import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { generateUUID } from "@/utils/uuid";

describe("generateUUID", () => {
  beforeEach(() => {
    vi.stubGlobal("crypto", {
      getRandomValues: (arr: Uint8Array) => {
        for (let i = 0; i < arr.length; i++) {
          arr[i] = Math.floor(Math.random() * 256);
        }
        return arr;
      },
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("returns a valid UUID v4 format", () => {
    const uuid = generateUUID();
    expect(uuid).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    );
  });

  it("uses crypto.randomUUID when available", () => {
    const mockRandomUUID = vi.fn().mockReturnValue("mock-uuid-v4");
    vi.stubGlobal("crypto", {
      randomUUID: mockRandomUUID,
      getRandomValues: vi.fn(),
    });
    const result = generateUUID();
    expect(mockRandomUUID).toHaveBeenCalled();
    expect(result).toBe("mock-uuid-v4");
  });

  it("falls back to getRandomValues when randomUUID is missing", () => {
    const getRandomValues = vi.fn((arr: Uint8Array) => {
      for (let i = 0; i < arr.length; i++) {
        arr[i] = i * 17;
      }
      return arr;
    });
    vi.stubGlobal("crypto", {
      randomUUID: undefined,
      getRandomValues,
    });
    const result = generateUUID();
    expect(getRandomValues).toHaveBeenCalled();
    expect(result).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    );
  });

  it("throws when crypto API is completely unavailable", () => {
    vi.stubGlobal("crypto", undefined);
    expect(() => generateUUID()).toThrow("crypto API not available");
  });
});
