import { describe, expect, it } from "vitest";

import {
  fillCodexCatalogModel,
  fillHermesModel,
  fillOpenClawModel,
  fillOpenCodeModel,
} from "@/components/providers/forms/modelMetadataFill";
import type { KnownModelMetadata } from "@/lib/modelMetadata";

const metadata: KnownModelMetadata = {
  contextWindow: 262144,
  maxOutputTokens: 32768,
  reasoning: true,
  reasoningEfforts: ["max", "low", "turbo", "high"],
  inputModalities: ["text", "image", "video"],
  outputModalities: ["text"],
  cost: { input: 1, output: 4 },
};

const CODEX_LEVELS = ["none", "low", "medium", "high", "xhigh", "max"];

describe("fillCodexCatalogModel", () => {
  it("fills blank fields in Codex's level order", () => {
    expect(
      fillCodexCatalogModel(
        { model: "kimi-k2.6", contextWindow: "" },
        metadata,
        CODEX_LEVELS,
      ),
    ).toEqual({
      model: "kimi-k2.6",
      contextWindow: "262144",
      reasoningLevels: ["low", "high", "max"],
      inputModalities: ["text", "image"],
    });
  });

  it("never overwrites what the user already set", () => {
    const row = {
      model: "kimi-k2.6",
      contextWindow: "128000",
      reasoningLevels: ["high"],
      inputModalities: ["text"],
    };
    expect(fillCodexCatalogModel(row, metadata, CODEX_LEVELS)).toEqual(row);
  });
});

describe("fillOpenClawModel", () => {
  it("fills numbers and cost, and upgrades the default text-only input", () => {
    expect(
      fillOpenClawModel({ id: "m", name: "m", input: ["text"] }, metadata),
    ).toEqual({
      id: "m",
      name: "m",
      contextWindow: 262144,
      maxTokens: 32768,
      reasoning: true,
      input: ["text", "image"],
      cost: { input: 1, output: 4 },
    });
  });

  it("keeps user values and never downgrades capabilities", () => {
    const model = {
      id: "m",
      name: "m",
      contextWindow: 1000,
      maxTokens: 10,
      reasoning: true,
      input: ["text", "image"],
      cost: { input: 9, output: 9 },
    };
    expect(
      fillOpenClawModel(model, {
        ...metadata,
        reasoning: false,
        inputModalities: ["text"],
      }),
    ).toEqual(model);
  });
});

describe("fillHermesModel", () => {
  it("fills context_length only when blank", () => {
    expect(fillHermesModel({ id: "m" }, metadata)).toEqual({
      id: "m",
      context_length: 262144,
    });
    expect(fillHermesModel({ id: "m", context_length: 1 }, metadata)).toEqual({
      id: "m",
      context_length: 1,
    });
  });
});

describe("fillOpenCodeModel", () => {
  it("fills missing limits and modalities", () => {
    expect(fillOpenCodeModel({ name: "m" }, metadata)).toEqual({
      name: "m",
      limit: { context: 262144, output: 32768 },
      modalities: { input: ["text", "image", "video"], output: ["text"] },
    });
  });

  it("only fills the missing half of limit", () => {
    expect(
      fillOpenCodeModel(
        {
          name: "m",
          limit: { context: 1000 },
          modalities: { input: ["text"] },
        },
        metadata,
      ),
    ).toEqual({
      name: "m",
      limit: { context: 1000, output: 32768 },
      modalities: { input: ["text"] },
    });
  });

  it("leaves the model untouched when nothing is known", () => {
    expect(fillOpenCodeModel({ name: "m" }, {})).toEqual({ name: "m" });
  });
});
