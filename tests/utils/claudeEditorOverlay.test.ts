import { describe, expect, it } from "vitest";
import { withClaudeGatewayDefaults } from "@/utils/claudeEditorOverlay";

describe("withClaudeGatewayDefaults", () => {
  const gateway = { env: { ANTHROPIC_BASE_URL: "https://gw.example" } };

  it.each(["third_party", "aggregator", "cn_official", "custom", undefined])(
    "turns auto mode server checks off for %s providers",
    (category) => {
      expect(withClaudeGatewayDefaults(gateway, category)).toEqual({
        env: {
          ANTHROPIC_BASE_URL: "https://gw.example",
          CLAUDE_CODE_AUTO_MODE_SERVER: "0",
        },
      });
    },
  );

  it.each(["official", "cloud_provider"])(
    "leaves %s providers on the default",
    (category) => {
      expect(withClaudeGatewayDefaults(gateway, category)).toBe(gateway);
    },
  );

  it("keeps a value the preset sets itself", () => {
    const preset = { env: { CLAUDE_CODE_AUTO_MODE_SERVER: "1" } };
    expect(withClaudeGatewayDefaults(preset, "third_party")).toBe(preset);
  });

  it("adds env when the preset has none", () => {
    expect(withClaudeGatewayDefaults({}, "custom")).toEqual({
      env: { CLAUDE_CODE_AUTO_MODE_SERVER: "0" },
    });
  });
});
