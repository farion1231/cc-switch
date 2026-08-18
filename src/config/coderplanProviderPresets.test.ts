import { describe, expect, it } from "vitest";

import { providerPresets } from "./claudeProviderPresets";
import { getIcon, getIconMetadata } from "../icons/extracted";

const anthropicBaseUrl = "https://api.coderplan.ai";
const brandDetails = {
  websiteUrl: "https://coderplan.ai",
  apiKeyUrl: "https://coderplan.ai/dashboard/keys",
  category: "aggregator",
  icon: "coderplan",
  iconColor: "#1e1e2e",
};

function findCoderPlanEntry<T extends { name: string }>(entries: readonly T[]) {
  return entries.find((entry) => entry.name === "CoderPlan");
}

describe("CoderPlan provider presets", () => {
  it("registers exactly one CoderPlan preset", () => {
    expect(
      providerPresets.filter((entry) => entry.name === "CoderPlan"),
    ).toHaveLength(1);
  });

  it("configures Claude Code with the Anthropic endpoint", () => {
    const preset = findCoderPlanEntry(providerPresets)!;
    expect(preset).toMatchObject({
      ...brandDetails,
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: anthropicBaseUrl,
          ANTHROPIC_AUTH_TOKEN: "",
          ANTHROPIC_MODEL: "claude-sonnet-5",
          ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-haiku-4-5-20251001",
          ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet-5",
          ANTHROPIC_DEFAULT_OPUS_MODEL: "claude-opus-5",
        },
      },
      endpointCandidates: [anthropicBaseUrl],
    });
  });

  it("registers the coderplan icon and metadata", () => {
    expect(getIcon("coderplan")).toContain("<svg");
    expect(getIconMetadata("coderplan")).toMatchObject({
      displayName: "CoderPlan",
      category: "ai-provider",
    });
  });
});
