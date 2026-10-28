import { describe, expect, it } from "vitest";
import { summarizeOmpReferences } from "@/components/providers/ompModelReferences";
import type { OmpModelManagementState } from "@/lib/api/omp";

const state: OmpModelManagementState = {
  scope: "global",
  settingsPath: "config.yml",
  modelsPath: "models.yml",
  settingsRevision: "s",
  modelsRevision: "m",
  defaultModel: null,
  defaultThinkingLevel: null,
  roleNames: ["slow", "custom"],
  modelRoles: { slow: [" @custom:high, cpa/second:low "], custom: "cpa/first" },
  agentModelOverrides: {
    scout: " pi/slow, cpa/second:inherit ",
    cycle: "@cycle",
  },
  modelOverrides: [],
  catalogStatus: "ready",
  catalogMessage: null,
  models: ["first", "second", "literal,comma"].map((id) => ({
    provider: "cpa",
    id,
    kind: "chat",
    name: id,
    contextWindow: null,
    maxContextWindow: null,
    maxTokens: null,
    input: ["text"],
    reasoning: true,
    thinking: null,
    source: "configured",
    cacheUpdatedAt: null,
  })),
};

describe("configured OMP reference summaries", () => {
  it("resolves comma candidates, array entries and legacy aliases without rewriting originals", () => {
    const result = summarizeOmpReferences({
      ...state,
      modelRoles: { ...state.modelRoles, cycle: "@cycle" },
    });
    expect(result.byModel.get("cpa/first")).toContain(
      "task.agentModelOverrides.scout →  pi/slow, cpa/second:inherit ",
    );
    expect(result.byModel.get("cpa/second")).toContain(
      "task.agentModelOverrides.scout →  pi/slow, cpa/second:inherit ",
    );
    expect(result.unresolved).toContain(
      "task.agentModelOverrides.cycle → @cycle (@cycle)",
    );
  });

  it("prefers exact catalog identities over comma splitting or pi role aliases", () => {
    const snapshot: OmpModelManagementState = {
      ...state,
      modelRoles: { slow: "cpa/first" },
      agentModelOverrides: { literal: "cpa/literal,comma", legacy: "pi/slow" },
      models: [
        ...state.models,
        { ...state.models[0], provider: "pi", id: "slow" },
      ],
    };
    const result = summarizeOmpReferences(snapshot);
    expect(result.byModel.get("cpa/literal,comma")).toEqual([
      "task.agentModelOverrides.literal → cpa/literal,comma",
    ]);
    expect(result.byModel.get("pi/slow")).toEqual([
      "task.agentModelOverrides.legacy → pi/slow",
    ]);
    expect(result.byModel.get("cpa/first")).toEqual([
      "modelRoles.slow → cpa/first",
    ]);
  });

  it("recognizes unambiguous thinking abbreviations and exact custom alias keys", () => {
    const snapshot: OmpModelManagementState = {
      ...state,
      modelRoles: {
        "custom/path:variant": "cpa/first",
        "custom:med": "cpa/second",
      },
      agentModelOverrides: {
        short: "cpa/first:med, cpa/second:xh",
        slash: "@custom/path:variant:xh",
        exact: "@custom:med",
        legacy: "pi/custom/path:variant",
      },
    };
    const result = summarizeOmpReferences(snapshot);
    expect(result.byModel.get("cpa/first")).toContain(
      "task.agentModelOverrides.short → cpa/first:med, cpa/second:xh",
    );
    expect(result.byModel.get("cpa/second")).toContain(
      "task.agentModelOverrides.short → cpa/first:med, cpa/second:xh",
    );
    expect(result.byModel.get("cpa/first")).toContain(
      "task.agentModelOverrides.slash → @custom/path:variant:xh",
    );
    expect(result.byModel.get("cpa/second")).toContain(
      "task.agentModelOverrides.exact → @custom:med",
    );
    expect(result.byModel.get("cpa/first")).toContain(
      "task.agentModelOverrides.legacy → pi/custom/path:variant",
    );
    expect(result.unresolved).toEqual([]);
  });
});
