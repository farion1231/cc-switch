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
      v.id.includes("anthropic.claude-opus-5"),
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
    expect(bedrockPreset!.settingsConfig.npm).toBe("@ai-sdk/amazon-bedrock");
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
    expect(bedrockPreset!.templateValues!.region.editorValue).toBe("us-west-2");
    expect(bedrockPreset!.templateValues!.accessKeyId).toBeDefined();
    expect(bedrockPreset!.templateValues!.secretAccessKey).toBeDefined();
  });

  it("Bedrock preset should include Claude models", () => {
    const models = bedrockPreset!.settingsConfig.models;
    expect(models).toBeDefined();
    const modelIds = Object.keys(models!);
    expect(modelIds.some((id) => id.includes("anthropic.claude"))).toBe(true);
  });

  it("Kimi For Coding preset should use Anthropic with the coding endpoint", () => {
    const kimiForCodingPreset = opencodeProviderPresets.find(
      (p) => p.name === "Kimi For Coding",
    );

    expect(kimiForCodingPreset).toBeDefined();
    expect(kimiForCodingPreset!.settingsConfig.npm).toBe("@ai-sdk/anthropic");
    expect(kimiForCodingPreset!.settingsConfig.options?.baseURL).toBe(
      "https://api.kimi.com/coding/v1",
    );
    expect(kimiForCodingPreset!.templateValues?.baseURL.defaultValue).toBe(
      "https://api.kimi.com/coding/v1",
    );
  });

  it("Xiaomi MiMo presets should include official OpenCode model metadata", () => {
    const presets = ["Xiaomi MiMo", "Xiaomi MiMo Token Plan (China)"].map(
      (name) => opencodeProviderPresets.find((preset) => preset.name === name),
    );

    for (const preset of presets) {
      expect(preset).toBeDefined();
      expect(preset!.settingsConfig.models["mimo-v2.5-pro"]).toMatchObject({
        limit: { context: 1048576, output: 131072 },
        modalities: { input: ["text"], output: ["text"] },
      });
      expect(preset!.settingsConfig.models["mimo-v2.5"]).toMatchObject({
        limit: { context: 1048576, output: 131072 },
        modalities: { input: ["text", "image"], output: ["text"] },
      });
    }
  });
});

describe("OpenCode Go usage script", () => {
  it("maps the official usage payload to three complete windows", () => {
    const preset = opencodeProviderPresets.find(
      (item) => item.name === "OpenCode Go",
    );
    expect(preset?.meta?.usage_script).toBeDefined();

    const script = JSON.parse(preset!.meta!.usage_script!.code);
    expect(script.request.url).toBe("{{baseUrl}}/usage");
    expect(
      script.request.url.replace(
        "{{baseUrl}}",
        preset!.settingsConfig.options.baseURL,
      ),
    ).toBe("https://opencode.ai/zen/go/v1/usage");

    const extractor = new Function(`return (${script.extractor})`)();
    const now = Date.now();
    const response = {
      usage: {
        rolling: {
          percent: 0,
          resetsAt: new Date(now + 4 * 3600000 + 59 * 60000 + 30000),
        },
        weekly: {
          percent: 19,
          resetsAt: new Date(now + 6 * 86400000 + 3 * 3600000 + 1800000),
        },
        monthly: {
          percent: 5,
          resetsAt: new Date(now + 21 * 86400000 + 7 * 3600000 + 1800000),
        },
      },
    };

    expect(extractor(response)).toEqual({
      isValid: true,
      planName: "OpenCode Go",
      extra: "5h:0%4h59m 7d:19%6d3h 30d:5%21d7h",
    });
  });
});
