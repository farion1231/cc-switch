import { describe, expect, it } from "vitest";

import { providerPresets } from "./claudeProviderPresets";
import { getIconMetadata, getIconUrl, isUrlIcon } from "../icons/extracted";

const anthropicBaseUrl = "https://us-proxy.unipaxtools.com";

function findUnipaxtoolsEntry<T extends { name: string }>(
  entries: readonly T[],
) {
  return entries.find((entry) => entry.name === "unipaxtools");
}

describe("unipaxtools provider presets", () => {
  it("Claude Code registers exactly one unipaxtools preset", () => {
    expect(
      providerPresets.filter((entry) => entry.name === "unipaxtools"),
    ).toHaveLength(1);
  });

  it("configures Claude Code with the Anthropic endpoint", () => {
    const preset = findUnipaxtoolsEntry(providerPresets)!;
    expect(preset).toMatchObject({
      websiteUrl: anthropicBaseUrl,
      apiKeyUrl: "https://us-proxy.unipaxtools.com/keys",
      category: "aggregator",
      icon: "unipaxtools",
      iconColor: "#0FA37F",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: anthropicBaseUrl,
          ANTHROPIC_AUTH_TOKEN: "",
        },
      },
      endpointCandidates: [anthropicBaseUrl],
    });
  });

  // 这家的 base url 必须是裸域名：带 /v1 或者结尾带斜杠都会被上游判成非法路径直接 401。
  it("keeps the base url free of a /v1 suffix and a trailing slash", () => {
    const preset = findUnipaxtoolsEntry(providerPresets)!;
    const env = (preset.settingsConfig as { env: Record<string, string> }).env;
    for (const url of [
      env.ANTHROPIC_BASE_URL,
      ...(preset.endpointCandidates ?? []),
    ]) {
      expect(url.endsWith("/")).toBe(false);
      expect(url.endsWith("/v1")).toBe(false);
    }
  });

  it("registers the unipaxtools brand icon", () => {
    expect(isUrlIcon("unipaxtools")).toBe(true);
    expect(getIconUrl("unipaxtools")).not.toBe("");
    expect(getIconMetadata("unipaxtools")).toMatchObject({
      displayName: "unipaxtools",
      defaultColor: "#0FA37F",
    });
  });
});
