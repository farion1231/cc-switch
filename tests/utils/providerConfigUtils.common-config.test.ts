import { describe, expect, it } from "vitest";
import { parse as parseToml } from "smol-toml";
import {
  hasCommonConfigSnippet,
  hasTomlCommonConfigSnippet,
  updateCommonConfigSnippet,
  updateTomlCommonConfigSnippet,
} from "@/utils/providerConfigUtils";

describe("common config precedence utils", () => {
  it("preserves JSON provider overrides while applying common config", () => {
    const providerConfig = JSON.stringify(
      {
        includeCoAuthoredBy: true,
        nested: {
          shared: "provider",
          providerOnly: "keep",
        },
      },
      null,
      2,
    );
    const snippet = JSON.stringify(
      {
        includeCoAuthoredBy: false,
        nested: {
          shared: "common",
          commonOnly: "shared",
        },
      },
      null,
      2,
    );

    const result = updateCommonConfigSnippet(providerConfig, snippet, true);

    expect(result.error).toBeUndefined();
    expect(JSON.parse(result.updatedConfig)).toEqual({
      includeCoAuthoredBy: true,
      nested: {
        shared: "provider",
        commonOnly: "shared",
        providerOnly: "keep",
      },
    });
    expect(hasCommonConfigSnippet(result.updatedConfig, snippet)).toBe(true);
  });

  it("preserves TOML provider overrides while applying common config", () => {
    const providerConfig = [
      'model = "provider-model"',
      "",
      "[profiles.default]",
      'approval_policy = "never"',
      "",
    ].join("\n");
    const snippet = [
      'model = "common-model"',
      "",
      "[profiles.default]",
      'sandbox_mode = "workspace-write"',
      "",
    ].join("\n");

    const result = updateTomlCommonConfigSnippet(providerConfig, snippet, true);

    expect(result.error).toBeUndefined();
    expect(parseToml(result.updatedConfig)).toEqual({
      model: "provider-model",
      profiles: {
        default: {
          approval_policy: "never",
          sandbox_mode: "workspace-write",
        },
      },
    });
    expect(hasTomlCommonConfigSnippet(result.updatedConfig, snippet)).toBe(
      true,
    );
  });
});
