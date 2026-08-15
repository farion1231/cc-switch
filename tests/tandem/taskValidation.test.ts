import { describe, expect, it } from "vitest";
import { containsStructuredCredential } from "@/tandem/taskValidation";

describe("containsStructuredCredential", () => {
  it("matches Rust token suffix and Unicode boundary rules", () => {
    expect(containsStructuredCredential("sk-12345678901234567890")).toBe(true);
    expect(containsStructuredCredential("ésk-12345678901234567890")).toBe(
      false,
    );
    expect(containsStructuredCredential("sk-1234567890-1234567890")).toBe(
      false,
    );
    expect(containsStructuredCredential("toKen=123456789012")).toBe(false);
    expect(containsStructuredCredential("İtoken=123456789012")).toBe(false);
  });

  it("detects fixed private-key, token, and named-secret forms without returning matches", () => {
    for (const value of [
      "-----BEGIN PRIVATE KEY-----",
      "sk_" + "live_123456789012345678901234",
      "ghp_123456789012345678901234567890123456",
      "github_pat_1234567890123456789012345678901234567890123456789012345678901234567890123456789012",
      "xoxb-1234567890",
      "AKIA1234567890123456",
      "Token = 123456789012",
    ]) {
      expect(containsStructuredCredential(value)).toBe(true);
    }
  });
});
