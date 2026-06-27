import { describe, expect, it } from "vitest";
import {
  normalizeCodexAuthForSave,
  pickCodexApiKey,
} from "@/components/providers/forms/hooks/useCodexConfigState";

const codexConfigWithBearerToken = (token: string) =>
  [
    'model_provider = "thirdparty"',
    "",
    "[model_providers.thirdparty]",
    'name = "Thirdparty"',
    'base_url = "https://api.example.com/v1"',
    `experimental_bearer_token = "${token}"`,
    "",
  ].join("\n");

describe("useCodexConfigState", () => {
  it("prefers the provider bearer token for third-party Codex configs", () => {
    const config = codexConfigWithBearerToken("sk-provider-token");

    expect(
      pickCodexApiKey({ OPENAI_API_KEY: "sk-old-auth" }, config, "aggregator"),
    ).toBe("sk-provider-token");

    expect(
      normalizeCodexAuthForSave(
        { OPENAI_API_KEY: "sk-old-auth" },
        config,
        "aggregator",
      ),
    ).toEqual({
      OPENAI_API_KEY: "sk-provider-token",
    });
  });

  it("keeps auth.json as the source of truth for official Codex configs", () => {
    const config = codexConfigWithBearerToken("sk-third-party-token");

    expect(
      pickCodexApiKey({ OPENAI_API_KEY: "sk-official-auth" }, config, "official"),
    ).toBe("sk-official-auth");

    expect(
      normalizeCodexAuthForSave(
        { OPENAI_API_KEY: "sk-official-auth" },
        config,
        "official",
      ),
    ).toEqual({
      OPENAI_API_KEY: "sk-official-auth",
    });
  });

  it("falls back to bearer token when auth is empty", () => {
    const config = codexConfigWithBearerToken("sk-provider-token");

    expect(pickCodexApiKey({}, config, "third_party")).toBe(
      "sk-provider-token",
    );
  });
});
