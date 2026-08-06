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

  it("returns true for equal rows including hidden profile fields", () => {
    const rows = [
      {
        model: "gpt-5.2",
        providerId: "provider-1",
        upstreamModel: "vendor-model",
        displayName: "Vendor Model",
        contextWindow: 128_000,
        supportsParallelToolCalls: true,
        inputModalities: ["text", "image"],
        baseInstructions: "instructions",
      },
    ];
    expect(customRowsMatchModels(rows, [...rows])).toBe(true);
  });

  it("returns false when row counts differ", () => {
    expect(
      customRowsMatchModels(
        [{ model: "gpt-5.2", providerId: "provider-1" }],
        [
          { model: "gpt-5.2", providerId: "provider-1" },
          { model: "gpt-5.4", providerId: "provider-2" },
        ],
      ),
    ).toBe(false);
  });

  it("treats undefined and empty-string optional fields as equal", () => {
    expect(
      customRowsMatchModels(
        [{ model: "gpt-5.2", providerId: "provider-1", upstreamModel: undefined }],
        [{ model: "gpt-5.2", providerId: "provider-1", upstreamModel: "" }],
      ),
    ).toBe(true);
  });

  it("compares contextWindow across number and string representations", () => {
    expect(
      customRowsMatchModels(
        [{ model: "gpt-5.2", providerId: "provider-1", contextWindow: 128_000 }],
        [{ model: "gpt-5.2", providerId: "provider-1", contextWindow: "128000" }],
      ),
    ).toBe(true);
  });
});
