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

  it("does not mirror legacy customUserAgent into custom headers entries", () => {
    const entries = providerCustomHeadersToEntries({
      customHeaders: {
        "X-Custom-Header": "value",
      },
      customUserAgent: "claude-code/0.1.0",
    });

    expect(entries).toEqual([{ key: "X-Custom-Header", value: "value" }]);
    expect(providerCustomHeadersToRecord(entries)).toEqual({
      "X-Custom-Header": "value",
    });
  });

  it("keeps legacy customUserAgent available on its own field", () => {
    const meta = {
      customHeaders: {
        "X-Custom-Header": "value",
      },
      customUserAgent: "claude-code/0.1.0",
    };

    expect(meta.customUserAgent).toBe("claude-code/0.1.0");
    expect(providerCustomHeadersToEntries(meta)).toEqual([
      { key: "X-Custom-Header", value: "value" },
    ]);
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
