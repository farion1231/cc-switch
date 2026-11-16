export interface OmoLocalFileData {
  agents?: Record<string, Record<string, unknown>>;
  categories?: Record<string, Record<string, unknown>>;
  otherFields?: Record<string, unknown>;
  filePath: string;
  lastModified?: string;
}

export interface OmoAgentDef {
  key: string;
  display: string;
  descKey: string;
  tooltipKey: string;
  recommended?: string;
  group: "main" | "sub";
}

export interface OmoCategoryDef {
  key: string;
  display: string;
  descKey: string;
  tooltipKey: string;
  recommended?: string;
}

export const OMO_BUILTIN_AGENTS: OmoAgentDef[] = [
  {
    key: "sisyphus",
    display: "Sisyphus",
    descKey: "omo.agentDesc.sisyphus",
    tooltipKey: "omo.agentTooltip.sisyphus",
    recommended: "claude-opus-5",
    group: "main",
  },
  {
    key: "hephaestus",
    display: "Hephaestus",
    descKey: "omo.agentDesc.hephaestus",
    tooltipKey: "omo.agentTooltip.hephaestus",
    recommended: "gpt-5.6-sol",
    group: "main",
  },
  {
    key: "prometheus",
    display: "Prometheus",
    descKey: "omo.agentDesc.prometheus",
    tooltipKey: "omo.agentTooltip.prometheus",
    recommended: "claude-opus-5",
    group: "main",
  },
  {
    key: "atlas",
    display: "Atlas",
    descKey: "omo.agentDesc.atlas",
    tooltipKey: "omo.agentTooltip.atlas",
    recommended: "claude-sonnet-4-6",
    group: "main",
  },
  {
    key: "oracle",
    display: "Oracle",
    descKey: "omo.agentDesc.oracle",
    tooltipKey: "omo.agentTooltip.oracle",
    recommended: "gpt-5.6-sol",
    group: "sub",
  },
  {
    key: "librarian",
    display: "Librarian",
    descKey: "omo.agentDesc.librarian",
    tooltipKey: "omo.agentTooltip.librarian",
    recommended: "gpt-5.4-mini-fast",
    group: "sub",
  },
  {
    key: "explore",
    display: "Explore",
    descKey: "omo.agentDesc.explore",
    tooltipKey: "omo.agentTooltip.explore",
    recommended: "gpt-5.4-mini-fast",
    group: "sub",
  },
  {
    key: "multimodal-looker",
    display: "Multimodal-Looker",
    descKey: "omo.agentDesc.multimodalLooker",
    tooltipKey: "omo.agentTooltip.multimodalLooker",
    recommended: "gpt-5.6-sol",
    group: "sub",
  },
  {
    key: "metis",
    display: "Metis",
    descKey: "omo.agentDesc.metis",
    tooltipKey: "omo.agentTooltip.metis",
    recommended: "claude-sonnet-4-6",
    group: "sub",
  },
  {
    key: "momus",
    display: "Momus",
    descKey: "omo.agentDesc.momus",
    tooltipKey: "omo.agentTooltip.momus",
    recommended: "gpt-5.6-sol",
    group: "sub",
  },
  {
    key: "sisyphus-junior",
    display: "Sisyphus-Junior",
    descKey: "omo.agentDesc.sisyphusJunior",
    tooltipKey: "omo.agentTooltip.sisyphusJunior",
    recommended: "claude-sonnet-4-6",
    group: "sub",
  },
];

export const OMO_BUILTIN_CATEGORIES: OmoCategoryDef[] = [
  {
    key: "visual-engineering",
    display: "Visual Engineering",
    descKey: "omo.categoryDesc.visualEngineering",
    tooltipKey: "omo.categoryTooltip.visualEngineering",
    recommended: "gemini-3.6-flash",
  },
  {
    key: "ultrabrain",
    display: "Ultrabrain",
    descKey: "omo.categoryDesc.ultrabrain",
    tooltipKey: "omo.categoryTooltip.ultrabrain",
    recommended: "gpt-5.6-sol",
  },
  {
    key: "deep",
    display: "Deep",
    descKey: "omo.categoryDesc.deep",
    tooltipKey: "omo.categoryTooltip.deep",
    recommended: "gpt-5.6-sol",
  },
  {
    key: "artistry",
    display: "Artistry",
    descKey: "omo.categoryDesc.artistry",
    tooltipKey: "omo.categoryTooltip.artistry",
    recommended: "gemini-3.6-flash",
  },
  {
    key: "quick",
    display: "Quick",
    descKey: "omo.categoryDesc.quick",
    tooltipKey: "omo.categoryTooltip.quick",
    recommended: "gpt-5.4-mini",
  },
  {
    key: "unspecified-low",
    display: "Unspecified Low",
    descKey: "omo.categoryDesc.unspecifiedLow",
    tooltipKey: "omo.categoryTooltip.unspecifiedLow",
    recommended: "claude-sonnet-4-6",
  },
  {
    key: "unspecified-high",
    display: "Unspecified High",
    descKey: "omo.categoryDesc.unspecifiedHigh",
    tooltipKey: "omo.categoryTooltip.unspecifiedHigh",
    recommended: "claude-opus-5",
  },
  {
    key: "writing",
    display: "Writing",
    descKey: "omo.categoryDesc.writing",
    tooltipKey: "omo.categoryTooltip.writing",
    recommended: "k2p5",
  },
];

export function parseOmoOtherFieldsObject(
  raw: string,
): Record<string, unknown> | undefined {
  if (!raw.trim()) return undefined;
  const parsed: unknown = JSON.parse(raw);
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    return undefined;
  }
  return parsed as Record<string, unknown>;
}

// ============================================================================
// OMO Slim (oh-my-opencode-slim) definitions
// ============================================================================

export const OMO_SLIM_BUILTIN_AGENTS: OmoAgentDef[] = [
  {
    key: "orchestrator",
    display: "Orchestrator",
    descKey: "omo.slimAgentDesc.orchestrator",
    tooltipKey: "omo.slimAgentTooltip.orchestrator",
    recommended: "claude-opus-5",
    group: "main",
  },
  {
    key: "oracle",
    display: "Oracle",
    descKey: "omo.slimAgentDesc.oracle",
    tooltipKey: "omo.slimAgentTooltip.oracle",
    recommended: "gpt-5.6-sol",
    group: "sub",
  },
  {
    key: "librarian",
    display: "Librarian",
    descKey: "omo.slimAgentDesc.librarian",
    tooltipKey: "omo.slimAgentTooltip.librarian",
    recommended: "gemini-3.6-flash",
    group: "sub",
  },
  {
    key: "explorer",
    display: "Explorer",
    descKey: "omo.slimAgentDesc.explorer",
    tooltipKey: "omo.slimAgentTooltip.explorer",
    recommended: "grok-build-0.1",
    group: "sub",
  },
  {
    key: "designer",
    display: "Designer",
    descKey: "omo.slimAgentDesc.designer",
    tooltipKey: "omo.slimAgentTooltip.designer",
    recommended: "gemini-3.6-flash",
    group: "sub",
  },
  {
    key: "fixer",
    display: "Fixer",
    descKey: "omo.slimAgentDesc.fixer",
    tooltipKey: "omo.slimAgentTooltip.fixer",
    recommended: "gpt-5.6-sol",
    group: "sub",
  },
  {
    key: "council",
    display: "Council",
    descKey: "omo.slimAgentDesc.council",
    tooltipKey: "omo.slimAgentTooltip.council",
    recommended: "gpt-5.4-mini",
    group: "sub",
  },
];

export const OMO_SLIM_DISABLEABLE_AGENTS = [
  { value: "orchestrator", label: "Orchestrator" },
  { value: "oracle", label: "Oracle" },
  { value: "librarian", label: "Librarian" },
  { value: "explorer", label: "Explorer" },
  { value: "designer", label: "Designer" },
  { value: "fixer", label: "Fixer" },
  { value: "council", label: "Council" },
] as const;

export function buildOmoProfilePreview(
  agents: Record<string, Record<string, unknown>>,
  categories: Record<string, Record<string, unknown>> | undefined,
  otherFieldsStr: string,
  options?: { slim?: boolean },
): Record<string, unknown> {
  const result: Record<string, unknown> = {};
  const isSlim = options?.slim ?? false;

  if (Object.keys(agents).length > 0) result["agents"] = agents;
  if (!isSlim && categories && Object.keys(categories).length > 0)
    result["categories"] = categories;

  try {
    const other = parseOmoOtherFieldsObject(otherFieldsStr);
    if (other) {
      for (const [k, v] of Object.entries(other)) {
        result[k] = v;
      }
    }
  } catch {}

  return result;
}

/** @deprecated Use buildOmoProfilePreview with options.slim=true */
export function buildOmoSlimProfilePreview(
  agents: Record<string, Record<string, unknown>>,
  otherFieldsStr: string,
): Record<string, unknown> {
  return buildOmoProfilePreview(agents, undefined, otherFieldsStr, {
    slim: true,
  });
}
