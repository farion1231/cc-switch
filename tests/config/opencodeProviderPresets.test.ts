import { describe, expect, it } from "vitest";
import {
  opencodeProviderPresets,
  opencodeNpmPackages,
  OPENCODE_PRESET_MODEL_VARIANTS,
} from "@/config/opencodeProviderPresets";

describe("AWS Bedrock OpenCode Provider Presets", () => {
  it("should include @ai-sdk/amazon-bedrock in npm packages", () => {
    const bedrockPkg = opencodeNpmPackages.find(
      (p) => p.value === "@ai-sdk/amazon-bedrock",
    );
    expect(bedrockPkg).toBeDefined();
    expect(bedrockPkg!.label).toBe("Amazon Bedrock");
  });

  it("should include Bedrock model variants", () => {
    const variants = OPENCODE_PRESET_MODEL_VARIANTS["@ai-sdk/amazon-bedrock"];
    expect(variants).toBeDefined();
    expect(variants.length).toBeGreaterThan(0);

    const opusModel = variants.find((v) =>
      v.id.includes("anthropic.claude-opus-4-6"),
    );
    expect(opusModel).toBeDefined();
  });

  const bedrockPreset = opencodeProviderPresets.find(
    (p) => p.name === "AWS Bedrock",
  );

  it("should include AWS Bedrock preset", () => {
    expect(bedrockPreset).toBeDefined();
  });

  it("Bedrock preset should use @ai-sdk/amazon-bedrock npm package", () => {
    expect(bedrockPreset!.settingsConfig.npm).toBe(
      "@ai-sdk/amazon-bedrock",
    );
  });

  it("Bedrock preset should have region in options", () => {
    expect(bedrockPreset!.settingsConfig.options).toHaveProperty("region");
  });

  it("Bedrock preset should have cloud_provider category", () => {
    expect(bedrockPreset!.category).toBe("cloud_provider");
  });

  it("Bedrock preset should have template values for AWS credentials", () => {
    expect(bedrockPreset!.templateValues).toBeDefined();
    expect(bedrockPreset!.templateValues!.region).toBeDefined();
    expect(bedrockPreset!.templateValues!.region.editorValue).toBe(
      "us-west-2",
    );
    expect(bedrockPreset!.templateValues!.accessKeyId).toBeDefined();
    expect(bedrockPreset!.templateValues!.secretAccessKey).toBeDefined();
  });

  it("Bedrock preset should include Claude models", () => {
    const models = bedrockPreset!.settingsConfig.models;
    expect(models).toBeDefined();
    const modelIds = Object.keys(models!);
    expect(
      modelIds.some((id) => id.includes("anthropic.claude")),
    ).toBe(true);
  });
});
