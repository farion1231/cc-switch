import type { ClaudeSubagentRoute, Provider, ProviderMeta } from "@/types";

/** Reserved alias written to live CLAUDE_CODE_SUBAGENT_MODEL during takeover. */
export const CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS = "cc-switch-subagent-route";

/**
 * Extract CLAUDE_CODE_SUBAGENT_MODEL from a Claude provider settings_config.
 * Used by the provider form for read-only target model display and validation.
 */
export function extractClaudeSubagentModel(
  settingsConfig:
    | Provider["settingsConfig"]
    | Record<string, unknown>
    | undefined,
): string {
  if (!settingsConfig || typeof settingsConfig !== "object") return "";
  const env = (settingsConfig as { env?: Record<string, unknown> }).env;
  if (!env || typeof env !== "object") return "";
  const value = env.CLAUDE_CODE_SUBAGENT_MODEL;
  return typeof value === "string" ? value.trim() : "";
}

/** Strip trailing `[1M]` marker (case-insensitive) for alias comparison. */
export function stripClaudeOneMSuffix(model: string): string {
  return model.replace(/\[1m\]$/i, "").trim();
}

/** True when model is the reserved CC Switch subagent-route alias (optional [1M]). */
export function isReservedClaudeSubagentRouteAlias(model: string): boolean {
  return (
    stripClaudeOneMSuffix(model).toLowerCase() ===
    CLAUDE_SUBAGENT_ROUTE_MODEL_ALIAS
  );
}

export type ClaudeSubagentRouteValidation =
  | { ok: true }
  | {
      ok: false;
      reason: "missing_target" | "missing_model" | "reserved_alias";
    };

export interface ClaudeSubagentRouteBuildInput {
  /** App id of the form being saved */
  appId: string;
  /** Current provider id (empty when creating) */
  providerId?: string;
  /** Selected route target id; empty string means current provider */
  selectedTargetProviderId: string;
  /** Lookup of saved Claude providers by id (for hard save validation) */
  providersById?: Record<string, Provider | undefined>;
}

/**
 * Build the `claudeSubagentRoute` meta fragment for ProviderForm save.
 *
 * - Same-provider / empty selection → undefined (field omitted)
 * - Foreign selection → `{ providerId }` only (no credentials)
 */
export function buildClaudeSubagentRouteMeta(
  input: Pick<
    ClaudeSubagentRouteBuildInput,
    "appId" | "providerId" | "selectedTargetProviderId"
  >,
): ClaudeSubagentRoute | undefined {
  if (input.appId !== "claude") return undefined;
  const targetId = input.selectedTargetProviderId.trim();
  if (!targetId) return undefined;
  if (input.providerId && targetId === input.providerId) return undefined;
  return { providerId: targetId };
}

/**
 * Hard validation used before ProviderForm save.
 * Only runs when a foreign target is selected.
 */
export function validateClaudeSubagentRoute(
  input: ClaudeSubagentRouteBuildInput,
): ClaudeSubagentRouteValidation {
  if (input.appId !== "claude") return { ok: true };
  const targetId = input.selectedTargetProviderId.trim();
  if (!targetId) return { ok: true };
  if (input.providerId && targetId === input.providerId) return { ok: true };

  const target = input.providersById?.[targetId];
  if (!target) {
    return { ok: false, reason: "missing_target" };
  }

  const model = extractClaudeSubagentModel(target.settingsConfig);
  if (!model) {
    return { ok: false, reason: "missing_model" };
  }
  if (isReservedClaudeSubagentRouteAlias(model)) {
    return { ok: false, reason: "reserved_alias" };
  }
  return { ok: true };
}

/**
 * Apply route meta to a base ProviderMeta: set foreign route or clear residual.
 * Never copies credentials into meta.
 */
export function applyClaudeSubagentRouteToMeta(
  baseMeta: ProviderMeta,
  route: ClaudeSubagentRoute | undefined,
): ProviderMeta {
  const next: ProviderMeta = { ...baseMeta };
  if (route?.providerId?.trim()) {
    next.claudeSubagentRoute = { providerId: route.providerId.trim() };
  } else {
    delete next.claudeSubagentRoute;
  }
  return next;
}
