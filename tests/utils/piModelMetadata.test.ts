import { describe, expect, it } from "vitest";

import { piProviderPresets } from "@/config/piProviderPresets";
import type { ModelsDevResponse } from "@/lib/modelsDevPricing";
import { resolvePiModelMetadata } from "@/utils/piModelMetadata";

describe("resolvePiModelMetadata", () => {
  it("uses the selected Pi preset before public catalog metadata", () => {
    const preset = piProviderPresets.find(
      (candidate) => candidate.name === "ZetaAPI",
    );
    expect(preset).toBeDefined();

    expect(
      resolvePiModelMetadata("gpt-5.6-sol", {
        selectedPreset: preset,
        modelsDevCatalog: {
          test: {
            models: {
              "gpt-5.6-sol": {
                name: "Different catalog name",
                reasoning: false,
                modalities: { input: ["text"] },
                limit: { context: 200000, output: 32000 },
              },
            },
          },
        },
      }),
    ).toEqual({
      name: "GPT-5.6 Sol",
      reasoning: true,
      imageInput: true,
      contextWindow: 200000,
      maxTokens: 32000,
      sources: ["preset", "models-dev"],
    });
  });

  it("collapses duplicate exact Models.dev matches into stable metadata", () => {
    const catalog: ModelsDevResponse = {
      providerA: {
        models: {
          "gpt-5.6-luna": {
            id: "gpt-5.6-luna",
            name: "GPT-5.6 Luna",
            reasoning: true,
            modalities: { input: ["text", "image", "pdf"] },
            limit: { context: 1_050_000, output: 128_000 },
          },
        },
      },
      providerB: {
        models: {
          alias: {
            id: "gpt-5.6-luna",
            name: "GPT-5.6 Luna",
            reasoning: true,
            modalities: { input: ["text", "image"] },
            limit: { context: 1_050_000, output: 128_000 },
          },
        },
      },
    };

    expect(
      resolvePiModelMetadata("gpt-5.6-luna", {
        modelsDevCatalog: catalog,
      }),
    ).toEqual({
      name: "GPT-5.6 Luna",
      reasoning: true,
      imageInput: true,
      contextWindow: 1_050_000,
      maxTokens: 128_000,
      sources: ["models-dev"],
    });
  });

  it("uses owned-by only to disambiguate exact catalog matches", () => {
    const catalog: ModelsDevResponse = {
      upstream: {
        name: "Upstream",
        models: {
          shared: {
            name: "Upstream Shared",
            reasoning: true,
            modalities: { input: ["text", "image"] },
          },
        },
      },
      proxy: {
        name: "Proxy",
        models: {
          shared: {
            name: "Proxy Shared",
            reasoning: false,
            modalities: { input: ["text"] },
          },
        },
      },
    };

    expect(
      resolvePiModelMetadata("shared", {
        modelsDevCatalog: catalog,
        preferredProvider: "upstream",
      }),
    ).toMatchObject({
      name: "Upstream Shared",
      reasoning: true,
      imageInput: true,
    });
    expect(
      resolvePiModelMetadata("unknown-shared", {
        modelsDevCatalog: catalog,
        preferredProvider: "upstream",
      }),
    ).toBeUndefined();
  });

  it("does not infer unknown model capabilities from a similar name", () => {
    const catalog: ModelsDevResponse = {
      upstream: {
        models: {
          "known-model": {
            name: "Known model",
            reasoning: true,
            modalities: { input: ["text", "image"] },
          },
        },
      },
    };

    expect(
      resolvePiModelMetadata("known-model-proxy", {
        modelsDevCatalog: catalog,
      }),
    ).toBeUndefined();
  });
});
