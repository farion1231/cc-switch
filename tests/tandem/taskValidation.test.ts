import { describe, expect, it } from "vitest";
import { containsStructuredCredential } from "@/tandem/taskValidation";

const positiveCases = [
  "-----BEGIN PRIVATE KEY-----",
  "-----BEGIN RSA PRIVATE KEY-----",
  "sk-12345678901234567890",
  "sk_" + "live_123456789012345678901234",
  "ghp_123456789012345678901234567890123456",
  "github_pat_1234567890123456789012345678901234567890123456789012345678901234567890123456789012",
  "xoxb-1234567890",
  "xoxa-1234567890",
  "xoxp-1234567890",
  "xoxr-1234567890",
  "xoxs-1234567890",
  "AKIA1234567890123456",
  "API_KEY=123456789012",
  "apikey:123456789012",
  "Token = 123456789012",
  "secret: 123456789012",
  "PASSWORD=123456789012",
];

const nearMissCases = [
  "Rotate the api key before release.",
  "The token: should be stored outside this task.",
  "password = use the team vault",
  "Discuss sk- prefixes without including a token.",
  "AKIA is an AWS access-key prefix.",
  "sk-1234567890123456789",
  "sk_" + "live_12345678901234567890123",
  "ghp_12345678901234567890123456789012345",
  "github_pat_123456789012345678901234567890123456789012345678901234567890123456789012345678901",
  "xoxb-123456789",
  "AKIA123456789012345",
  "api_key=12345678901",
  "ésk-12345678901234567890",
  "épassword=123456789012",
  "prefixsk-12345678901234567890",
  "prefixAKIA1234567890123456",
];

describe("containsStructuredCredential", () => {
  it("matches every Rust positive credential case", () => {
    for (const value of positiveCases) {
      expect(containsStructuredCredential(value), value).toBe(true);
    }
  });

  it("keeps every Rust credential near miss valid", () => {
    for (const value of nearMissCases) {
      expect(containsStructuredCredential(value), value).toBe(false);
    }
  });

  it("uses Rust Unicode Alphabetic or Number semantics at boundaries", () => {
    expect(containsStructuredCredential("ͅsk-12345678901234567890")).toBe(false);
    expect(containsStructuredCredential("Ⅷsk-12345678901234567890")).toBe(
      false,
    );
  });

  it("uses Unicode White_Space for trimming and scalar scanning", () => {
    expect(containsStructuredCredential("token=123456789012")).toBe(true);
    expect(containsStructuredCredential("token﻿=123456789012")).toBe(false);
    expect(containsStructuredCredential("token=12345678901﻿")).toBe(true);
  });

  it("preserves the ASCII-only token suffix policy", () => {
    expect(containsStructuredCredential("sk-1234567890-1234567890")).toBe(
      false,
    );
    expect(containsStructuredCredential("sk-1234567890123456789é")).toBe(false);
  });

  it("uses ASCII-only lowercasing for named spellings", () => {
    expect(containsStructuredCredential("toKen=123456789012")).toBe(false);
    expect(containsStructuredCredential("İtoken=123456789012")).toBe(false);
  });
});
