import { describe, expect, it } from "vitest";
import {
  applyClaudeSubagentRouteToMeta,
  buildClaudeSubagentRouteMeta,
  CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS,
  extractClaudeSubagentModel,
  isReservedClaudeSubagentRouteAlias,
  validateClaudeSubagentRoute,
} from "@/utils/claudeSubagentRoute";
import type { Provider, ProviderMeta } from "@/types";

const makeProvider = (id: string, subagentModel?: string): Provider =>
  ({
    id,
    name: id,
    settingsConfig: subagentModel
      ? { env: { CLAUDE_CODE_SUBAGENT_MODEL: subagentModel } }
      : { env: {} },
  }) as Provider;

describe("extractClaudeSubagentModel", () => {
  it("reads CLAUDE_CODE_SUBAGENT_MODEL from settings env", () => {
    expect(
      extractClaudeSubagentModel({
        env: { CLAUDE_CODE_SUBAGENT_MODEL: "target-sub[1M]" },
      }),
    ).toBe("target-sub[1M]");
  });

  it("returns empty for missing/blank values", () => {
    expect(extractClaudeSubagentModel(undefined)).toBe("");
    expect(extractClaudeSubagentModel({})).toBe("");
    expect(
      extractClaudeSubagentModel({ env: { CLAUDE_CODE_SUBAGENT_MODEL: "  " } }),
    ).toBe("");
  });
});

describe("isReservedClaudeSubagentRouteAlias", () => {
  it("detects reserved alias with optional [1M] and case", () => {
    expect(
      isReservedClaudeSubagentRouteAlias(CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS),
    ).toBe(true);
    expect(
      isReservedClaudeSubagentRouteAlias("cc-switch-subagent-route[1M]"),
    ).toBe(true);
    expect(isReservedClaudeSubagentRouteAlias("CC-Switch-Subagent-Route")).toBe(
      true,
    );
    expect(isReservedClaudeSubagentRouteAlias("claude-sonnet-4-6")).toBe(false);
  });
});

describe("buildClaudeSubagentRouteMeta", () => {
  it("returns undefined for non-claude app", () => {
    expect(
      buildClaudeSubagentRouteMeta({
        appId: "codex",
        providerId: "a",
        selectedTargetProviderId: "b",
      }),
    ).toBeUndefined();
  });

  it("returns undefined for current/empty/self selection (clear path)", () => {
    expect(
      buildClaudeSubagentRouteMeta({
        appId: "claude",
        providerId: "a",
        selectedTargetProviderId: "",
      }),
    ).toBeUndefined();
    expect(
      buildClaudeSubagentRouteMeta({
        appId: "claude",
        providerId: "a",
        selectedTargetProviderId: "a",
      }),
    ).toBeUndefined();
  });

  it("serializes only providerId without credentials", () => {
    const meta = buildClaudeSubagentRouteMeta({
      appId: "claude",
      providerId: "a",
      selectedTargetProviderId: "provider-b",
    });
    expect(meta).toEqual({ providerId: "provider-b" });
    const json = JSON.stringify(meta);
    expect(json).not.toContain("apiKey");
    expect(json).not.toContain("token");
    expect(json).not.toContain("sk-");
  });
});

describe("validateClaudeSubagentRoute", () => {
  it("accepts empty/current selection", () => {
    expect(
      validateClaudeSubagentRoute({
        appId: "claude",
        providerId: "a",
        selectedTargetProviderId: "",
      }),
    ).toEqual({ ok: true });
  });

  it("rejects missing/deleted target", () => {
    expect(
      validateClaudeSubagentRoute({
        appId: "claude",
        providerId: "a",
        selectedTargetProviderId: "gone",
        providersById: {},
      }),
    ).toEqual({ ok: false, reason: "missing_target" });
  });

  it("rejects target without subagent model", () => {
    expect(
      validateClaudeSubagentRoute({
        appId: "claude",
        providerId: "a",
        selectedTargetProviderId: "b",
        providersById: { b: makeProvider("b") },
      }),
    ).toEqual({ ok: false, reason: "missing_model" });
  });

  it("rejects target whose subagent model is the reserved alias", () => {
    expect(
      validateClaudeSubagentRoute({
        appId: "claude",
        providerId: "a",
        selectedTargetProviderId: "b",
        providersById: {
          b: makeProvider("b", CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS),
        },
      }),
    ).toEqual({ ok: false, reason: "reserved_alias" });
    expect(
      validateClaudeSubagentRoute({
        appId: "claude",
        providerId: "a",
        selectedTargetProviderId: "b",
        providersById: {
          b: makeProvider("b", "cc-switch-subagent-route[1M]"),
        },
      }),
    ).toEqual({ ok: false, reason: "reserved_alias" });
  });

  it("accepts valid foreign target", () => {
    expect(
      validateClaudeSubagentRoute({
        appId: "claude",
        providerId: "a",
        selectedTargetProviderId: "b",
        providersById: { b: makeProvider("b", "target-sub[1M]") },
      }),
    ).toEqual({ ok: true });
  });
});

describe("applyClaudeSubagentRouteToMeta persistence", () => {
  it("persists foreign route on edit and clears residual on reload to current", () => {
    const withRoute = applyClaudeSubagentRouteToMeta(
      { customUserAgent: "keep-me" },
      { providerId: "provider-b" },
    );
    expect(withRoute.claudeSubagentRoute).toEqual({
      providerId: "provider-b",
    });
    expect(withRoute.customUserAgent).toBe("keep-me");

    // Simulate save with "Current provider" after previous foreign route
    const cleared = applyClaudeSubagentRouteToMeta(withRoute, undefined);
    expect(cleared.claudeSubagentRoute).toBeUndefined();
    expect(cleared.customUserAgent).toBe("keep-me");
    expect(JSON.stringify(cleared)).not.toContain("claudeSubagentRoute");
  });

  it("omits route when default/current provider is selected", () => {
    const meta: ProviderMeta = applyClaudeSubagentRouteToMeta({}, undefined);
    expect(meta.claudeSubagentRoute).toBeUndefined();
    expect(JSON.stringify(meta)).toBe("{}");
  });

  it("never embeds secrets into route meta", () => {
    const meta = applyClaudeSubagentRouteToMeta(
      {
        claudeSubagentRoute: { providerId: "old" },
      },
      buildClaudeSubagentRouteMeta({
        appId: "claude",
        providerId: "a",
        selectedTargetProviderId: "b",
      }),
    );
    const json = JSON.stringify(meta);
    expect(json).toContain('"providerId":"b"');
    expect(json).not.toContain("apiKey");
    expect(json).not.toContain("ANTHROPIC");
    expect(json).not.toContain("sk-");
  });
});
