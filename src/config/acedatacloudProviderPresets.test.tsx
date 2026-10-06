import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { parse } from "smol-toml";
import { ProviderIcon } from "../components/ProviderIcon";
import {
  filterPresetEntries,
  type PresetEntry,
} from "../components/providers/forms/ProviderPresetSelector";
import { getIconMetadata } from "../icons/extracted";
import { providerPresets } from "./claudeProviderPresets";
import { claudeDesktopProviderPresets } from "./claudeDesktopProviderPresets";
import { codexProviderPresets } from "./codexProviderPresets";
import { grokBuildProviderPresets } from "./grokBuildProviderPresets";
import { opencodeProviderPresets } from "./opencodeProviderPresets";
import { openclawProviderPresets } from "./openclawProviderPresets";
import { hermesProviderPresets } from "./hermesProviderPresets";
import { piProviderPresets } from "./piProviderPresets";
import { mcodeProviderPresets } from "./mcodeProviderPresets";
import { geminiProviderPresets } from "./geminiProviderPresets";

const name = "Ace Data Cloud";
const base = "https://api.acedata.cloud/v1";
const groups = [
  ["Claude Code", providerPresets],
  ["Claude Desktop", claudeDesktopProviderPresets],
  ["Codex", codexProviderPresets],
  ["Grok Build", grokBuildProviderPresets],
  ["OpenCode", opencodeProviderPresets],
  ["OpenClaw", openclawProviderPresets],
  ["Hermes", hermesProviderPresets],
  ["Pi", piProviderPresets],
  ["MiniMax Code", mcodeProviderPresets],
] as const;

function find<T extends { name: string }>(presets: readonly T[]): T {
  const preset = presets.find((item) => item.name === name);
  if (!preset) throw new Error(`Missing ${name} preset`);
  return preset;
}

describe("Ace Data Cloud presets", () => {
  it.each(groups)(
    "%s is discoverable by display name and compact brand",
    (_, presets) => {
      const entries = presets.map((preset, index) => ({
        id: String(index),
        preset,
      })) as PresetEntry[];
      for (const query of [name, "acedatacloud"]) {
        const matches = filterPresetEntries(entries, query, (key) => key);
        expect(matches.map((entry) => entry.preset.name)).toEqual([name]);
      }
      expect(find(presets as readonly { name: string }[])).toMatchObject({
        websiteUrl: "https://platform.acedata.cloud",
        apiKeyUrl: "https://platform.acedata.cloud/console/credentials",
        category: "aggregator",
        icon: "acedatacloud",
      });
      expect(find(presets as readonly { name: string }[])).not.toHaveProperty(
        "isPartner",
      );
    },
  );

  it("renders the bundled brand asset as an image", () => {
    render(<ProviderIcon icon="acedatacloud" name={name} />);
    expect(screen.getByRole("img")).toHaveAttribute(
      "src",
      expect.stringContaining("acedatacloud.png"),
    );
    expect(getIconMetadata("acedatacloud")?.displayName).toBe(name);
  });

  it("keeps Claude on native Messages with an empty credential", () => {
    expect(find(providerPresets)).toMatchObject({
      apiFormat: "anthropic",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://api.acedata.cloud",
          ANTHROPIC_AUTH_TOKEN: "",
          ANTHROPIC_MODEL: "claude-sonnet-4-6",
        },
      },
    });
    expect(find(claudeDesktopProviderPresets)).toMatchObject({
      mode: "direct",
      apiFormat: "anthropic",
      baseUrl: "https://api.acedata.cloud",
      modelRoutes: [
        {
          routeId: "claude-sonnet-4-6",
          upstreamModel: "claude-sonnet-4-6",
          supports1m: false,
        },
      ],
    });
  });

  it.each([
    ["Codex", codexProviderPresets, "gpt-5.5"],
    ["Grok Build", grokBuildProviderPresets, "grok-4.7"],
  ] as const)(
    "uses native Responses with one credential source (%s)",
    (_app, presets, model) => {
      const preset = find(presets);
      const config = parse(preset.config);
      expect(config.model).toBe(model);
      expect(config.model_providers).toEqual({
        custom: {
          name,
          base_url: base,
          wire_api: "responses",
          requires_openai_auth: true,
        },
      });
      expect(preset.auth).toEqual({ OPENAI_API_KEY: "" });
      expect(preset.apiFormat).toBe("openai_responses");
    },
  );

  it("uses the validated Chat model and mode for coexist tools", () => {
    expect(find(opencodeProviderPresets).settingsConfig).toMatchObject({
      npm: "@ai-sdk/openai-compatible",
      options: { baseURL: base, apiKey: "" },
      models: { "gpt-4.1": { name: "GPT-4.1" } },
    });
    expect(find(openclawProviderPresets)).toMatchObject({
      settingsConfig: {
        baseUrl: base,
        apiKey: "",
        api: "openai-completions",
        models: [{ id: "gpt-4.1" }],
      },
      suggestedDefaults: { model: { primary: "acedatacloud/gpt-4.1" } },
    });
    expect(find(hermesProviderPresets)).toMatchObject({
      settingsConfig: {
        name: "acedatacloud",
        base_url: base,
        api_key: "",
        api_mode: "chat_completions",
        models: [{ id: "gpt-4.1" }],
      },
      suggestedDefaults: {
        model: { default: "gpt-4.1", provider: "acedatacloud" },
      },
    });
  });

  it("derives the MiniMax Code preset from Pi without losing Responses", () => {
    const pi = find(piProviderPresets);
    const mcode = find(mcodeProviderPresets);
    expect(pi.providerKey).toBe("cc-switch-acedatacloud");
    expect(pi.settingsConfig).toMatchObject({
      api: "openai-responses",
      baseUrl: base,
      apiKey: "",
      models: [{ id: "gpt-5.5" }],
    });
    expect(mcode.providerKey).toBe(pi.providerKey);
    expect(mcode.settingsConfig).toMatchObject({
      api: "openai-responses",
      options: { baseURL: base, apiKey: "" },
      models: { "gpt-5.5": { name: "GPT-5.5" } },
    });
  });

  it("does not advertise unverified Gemini native authentication", () => {
    expect(geminiProviderPresets.some((preset) => preset.name === name)).toBe(
      false,
    );
  });
});
