import { describe, expect, it } from "vitest";
import { customRowsMatchModels } from "./CodexFormFields";

describe("customRowsMatchModels", () => {
  it("detects changes to hidden native model profile fields", () => {
    const visibleFields = {
      model: "gpt-5.2",
      providerId: "provider-1",
      upstreamModel: "vendor-model",
      displayName: "Vendor Model",
      contextWindow: 128_000,
    };

    expect(
      customRowsMatchModels(
        [
          {
            ...visibleFields,
            supportsParallelToolCalls: false,
            inputModalities: ["text"],
            baseInstructions: "old instructions",
          },
        ],
        [
          {
            ...visibleFields,
            supportsParallelToolCalls: true,
            inputModalities: ["text", "image"],
            baseInstructions: "new instructions",
          },
        ],
      ),
    ).toBe(false);
  });
});
