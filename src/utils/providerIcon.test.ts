import { describe, expect, it } from "vitest";
import { resolveProviderIcon } from "./providerIcon";

describe("resolveProviderIcon", () => {
  it("clears the legacy automatic Grok Build icon", () => {
    expect(resolveProviderIcon("grokbuild", "grok", "")).toBeUndefined();
    expect(resolveProviderIcon("grokbuild", "grok")).toBeUndefined();
  });

  it("preserves a Grok icon explicitly selected by the user", () => {
    expect(resolveProviderIcon("grokbuild", "grok", "currentColor")).toBe(
      "grok",
    );
  });

  it("does not reinterpret another app's provider icon", () => {
    expect(resolveProviderIcon("codex", "grok", "")).toBe("grok");
  });

  it("clears legacy automatic Cursor protocol icons", () => {
    expect(resolveProviderIcon("cursor", "openai", "")).toBeUndefined();
    expect(resolveProviderIcon("cursor", "anthropic")).toBeUndefined();
  });

  it("preserves Cursor icons explicitly selected by the user", () => {
    expect(resolveProviderIcon("cursor", "openai", "#000000")).toBe("openai");
    expect(resolveProviderIcon("cursor", "deepseek", "")).toBe("deepseek");
  });

  it("normalizes an empty icon to the initials fallback", () => {
    expect(resolveProviderIcon("grokbuild", "  ", "")).toBeUndefined();
  });
});
