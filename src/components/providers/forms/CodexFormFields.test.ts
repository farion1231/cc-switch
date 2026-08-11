import { describe, expect, it } from "vitest";
import {
  applyCodexCustomModelCatalogSelection,
  customRowsMatchModels,
} from "./CodexFormFields";

describe("applyCodexCustomModelCatalogSelection", () => {
  it("refreshes or clears every hidden capability field with the upstream model", () => {
    const previous = {
      model: "gpt-5.2",
      providerId: "provider-1",
      upstreamModel: "old-model",
      displayName: "Old Model",
      contextWindow: 64_000,
      supportsParallelToolCalls: false,
      inputModalities: ["text"],
      baseInstructions: "old instructions",
    };

    const selected = applyCodexCustomModelCatalogSelection(
      previous,
      "new-model",
      {
        model: "new-model",
        displayName: " New Model ",
        contextWindow: 256_000,
        supportsParallelToolCalls: true,
        inputModalities: ["text", "image"],
        baseInstructions: "new instructions",
      },
    );
    expect(selected).toMatchObject({
      upstreamModel: "new-model",
      displayName: "New Model",
      contextWindow: 256_000,
      supportsParallelToolCalls: true,
      inputModalities: ["text", "image"],
      baseInstructions: "new instructions",
    });

    const uncatalogued = applyCodexCustomModelCatalogSelection(
      selected,
      "uncatalogued-model",
      undefined,
    );
    expect(uncatalogued.displayName).toBe("");
    expect(uncatalogued.contextWindow).toBe("");
    expect(uncatalogued.supportsParallelToolCalls).toBeUndefined();
    expect(uncatalogued.inputModalities).toBeUndefined();
    expect(uncatalogued.baseInstructions).toBeUndefined();
  });
});

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
        [
          {
            model: "gpt-5.2",
            providerId: "provider-1",
            upstreamModel: undefined,
          },
        ],
        [{ model: "gpt-5.2", providerId: "provider-1", upstreamModel: "" }],
      ),
    ).toBe(true);
  });

  it("compares contextWindow across number and string representations", () => {
    expect(
      customRowsMatchModels(
        [
          {
            model: "gpt-5.2",
            providerId: "provider-1",
            contextWindow: 128_000,
          },
        ],
        [
          {
            model: "gpt-5.2",
            providerId: "provider-1",
            contextWindow: "128000",
          },
        ],
      ),
    ).toBe(true);
  });
});
