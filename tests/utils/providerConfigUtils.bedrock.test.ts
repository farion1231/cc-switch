import { describe, expect, it } from "vitest";
import {
  getApiKeyFromConfig,
  hasApiKeyField,
  setApiKeyInConfig,
} from "@/utils/providerConfigUtils";

const bedrockConfig = (env: Record<string, string>, extra = {}) =>
  JSON.stringify({
    ...extra,
    env: { CLAUDE_CODE_USE_BEDROCK: "1", AWS_REGION: "us-west-2", ...env },
  });

describe("Bedrock API Key field", () => {
  it("reads and writes env.AWS_BEARER_TOKEN_BEDROCK", () => {
    const config = bedrockConfig({ AWS_BEARER_TOKEN_BEDROCK: "" });
    expect(hasApiKeyField(config, "claude")).toBe(true);

    const updated = setApiKeyInConfig(config, "bedrock-key", {
      appType: "claude",
    });
    const parsed = JSON.parse(updated);
    expect(parsed.env.AWS_BEARER_TOKEN_BEDROCK).toBe("bedrock-key");
    expect(parsed).not.toHaveProperty("apiKey");
    expect(parsed.env).not.toHaveProperty("ANTHROPIC_AUTH_TOKEN");
    expect(getApiKeyFromConfig(updated, "claude")).toBe("bedrock-key");
  });

  // 旧版预设把 Key 写在顶层 apiKey：存量行照常显示，编辑时写回原处，不改写行结构
  it("keeps legacy rows with a top-level apiKey working", () => {
    const legacy = bedrockConfig({}, { apiKey: "legacy-key" });
    expect(hasApiKeyField(legacy, "claude")).toBe(true);
    expect(getApiKeyFromConfig(legacy, "claude")).toBe("legacy-key");

    const parsed = JSON.parse(
      setApiKeyInConfig(legacy, "rotated-key", { appType: "claude" }),
    );
    expect(parsed.apiKey).toBe("rotated-key");
    expect(parsed.env).not.toHaveProperty("AWS_BEARER_TOKEN_BEDROCK");
  });

  it("prefers the env token over a leftover top-level apiKey", () => {
    const both = bedrockConfig(
      { AWS_BEARER_TOKEN_BEDROCK: "env-key" },
      { apiKey: "stale-key" },
    );
    expect(getApiKeyFromConfig(both, "claude")).toBe("env-key");
  });
});
