import { describe, expect, it } from "vitest";
import type { ProviderMeta } from "@/types";
import { mergeProviderMeta } from "@/utils/providerMetaUtils";

const buildEndpoint = (url: string) => ({
  url,
  addedAt: 1,
});

describe("mergeProviderMeta", () => {
  it("returns undefined when no initial meta and no endpoints", () => {
    expect(mergeProviderMeta(undefined, null)).toBeUndefined();
    expect(mergeProviderMeta(undefined, undefined)).toBeUndefined();
  });

  it("creates meta when endpoints are provided for new provider", () => {
    const result = mergeProviderMeta(undefined, {
      "https://example.com": buildEndpoint("https://example.com"),
    });

    expect(result).toEqual({
      custom_endpoints: {
        "https://example.com": buildEndpoint("https://example.com"),
      },
    });
  });

  it("overrides custom endpoints but preserves other fields", () => {
    const initial: ProviderMeta = {
      usage_script: {
        enabled: true,
        language: "javascript",
        code: "console.log(1);",
      },
      custom_endpoints: {
        "https://old.com": buildEndpoint("https://old.com"),
      },
    };

    const result = mergeProviderMeta(initial, {
      "https://new.com": buildEndpoint("https://new.com"),
    });

    expect(result).toEqual({
      usage_script: initial.usage_script,
      custom_endpoints: {
        "https://new.com": buildEndpoint("https://new.com"),
      },
    });
  });

  it("removes custom endpoints when result is empty but keeps other meta", () => {
    const initial: ProviderMeta = {
      usage_script: {
        enabled: true,
        language: "javascript",
        code: "console.log(1);",
      },
      custom_endpoints: {
        "https://example.com": buildEndpoint("https://example.com"),
      },
    };

    const result = mergeProviderMeta(initial, null);

    expect(result).toEqual({
      usage_script: initial.usage_script,
    });
  });

  it("returns undefined when removing last field", () => {
    const initial: ProviderMeta = {
      custom_endpoints: {
        "https://example.com": buildEndpoint("https://example.com"),
      },
    };

    expect(mergeProviderMeta(initial, null)).toBeUndefined();
  });
});
