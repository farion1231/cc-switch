import { describe, expect, it } from "vitest";

describe("custom header normalization for model fetch", () => {
  it("keeps non-empty header keys and preserves values", () => {
    const headers = [
      { key: "User-Agent", value: "claude-code/0.1.0" },
      { key: " x-api-key ", value: "sk-xxx" },
      { key: "   ", value: "ignored" },
    ];

    const normalized = Object.fromEntries(
      headers
        .map(({ key, value }) => [key.trim(), value] as const)
        .filter(([key]) => key.length > 0),
    );

    expect(normalized).toEqual({
      "User-Agent": "claude-code/0.1.0",
      "x-api-key": "sk-xxx",
    });
  });
});
