import { describe, expect, it } from "vitest";
import { normalizeCodexCustomModelsForSave } from "@/components/providers/forms/ProviderForm";
import { CODEX_OFFICIAL_PROVIDER_ID } from "@/utils/providerCapabilities";

describe("normalizeCodexCustomModelsForSave", () => {
  it("filters rows bound to the official provider in aggregate mode (official login off)", () => {
    expect(
      normalizeCodexCustomModelsForSave(
        [
          {
            model: "gpt-5.2",
            providerId: "deepseek",
            upstreamModel: "deepseek-v4-flash",
          },
          { model: "gpt-5.4", providerId: CODEX_OFFICIAL_PROVIDER_ID },
        ],
        { officialLogin: false },
      ),
    ).toEqual([
      {
        model: "gpt-5.2",
        providerId: "deepseek",
        upstreamModel: "deepseek-v4-flash",
      },
    ]);
  });

  it("keeps official-provider rows when official login is enabled", () => {
    expect(
      normalizeCodexCustomModelsForSave(
        [{ model: "gpt-5.2", providerId: CODEX_OFFICIAL_PROVIDER_ID }],
        { officialLogin: true },
      ),
    ).toEqual([{ model: "gpt-5.2", providerId: CODEX_OFFICIAL_PROVIDER_ID }]);
  });

  it("trims fields and drops empty model / providerId rows", () => {
    expect(
      normalizeCodexCustomModelsForSave(
        [
          { model: " gpt-5.2 ", providerId: " deepseek ", upstreamModel: "  " },
          { model: "", providerId: "deepseek" },
          { model: "gpt-5.4", providerId: "   " },
        ],
        { officialLogin: false },
      ),
    ).toEqual([{ model: "gpt-5.2", providerId: "deepseek" }]);
  });

  it("converts string contextWindow and preserves native-profile overrides", () => {
    expect(
      normalizeCodexCustomModelsForSave(
        [
          {
            model: "gpt-5.2",
            providerId: "deepseek",
            contextWindow: "128000",
            supportsParallelToolCalls: true,
            inputModalities: ["text"],
            baseInstructions: "instructions",
          },
        ],
        { officialLogin: false },
      ),
    ).toEqual([
      {
        model: "gpt-5.2",
        providerId: "deepseek",
        contextWindow: 128000,
        supportsParallelToolCalls: true,
        inputModalities: ["text"],
        baseInstructions: "instructions",
      },
    ]);
  });
});
