import { describe, expect, it } from "vitest";
import {
  providerCustomHeadersToEntries,
  providerCustomHeadersToRecord,
} from "@/components/providers/forms/ProviderForm";

describe("ProviderForm custom headers helpers", () => {
  it("keeps explicit custom headers and preserves legacy User-Agent fallback", () => {
    const entries = providerCustomHeadersToEntries({
      customHeaders: {
        "x-api-key": "sk-xxx",
        "User-Agent": "claude-code/0.1.0",
      },
      customUserAgent: "ignored-because-explicit-ua-exists",
    });

    expect(entries).toEqual([
      { key: "x-api-key", value: "sk-xxx" },
      { key: "User-Agent", value: "claude-code/0.1.0" },
    ]);
    expect(providerCustomHeadersToRecord(entries)).toEqual({
      "x-api-key": "sk-xxx",
      "User-Agent": "claude-code/0.1.0",
    });
  });

  it("falls back to legacy customUserAgent when no explicit User-Agent exists", () => {
    const entries = providerCustomHeadersToEntries({
      customHeaders: {
        "X-Custom-Header": "value",
      },
      customUserAgent: "claude-code/0.1.0",
    });

    expect(entries).toEqual([
      { key: "User-Agent", value: "claude-code/0.1.0" },
      { key: "X-Custom-Header", value: "value" },
    ]);
    expect(providerCustomHeadersToRecord(entries)).toEqual({
      "User-Agent": "claude-code/0.1.0",
      "X-Custom-Header": "value",
    });
  });

  it("drops empty keys when converting back to a record", () => {
    expect(
      providerCustomHeadersToRecord([
        { key: "  ", value: "ignored" },
        { key: "X-Test", value: "" },
      ]),
    ).toEqual({
      "X-Test": "",
    });
  });
});
