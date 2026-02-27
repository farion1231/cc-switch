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
    recommended: "claude-opus-4-6",
    group: "main",
  },
  {
    key: "hephaestus",
    display: "Hephaestus",
    descKey: "omo.agentDesc.hephaestus",
    tooltipKey: "omo.agentTooltip.hephaestus",
    recommended: "gpt-5.3-codex",
    group: "main",
  },
  {
    key: "prometheus",
    display: "Prometheus",
    descKey: "omo.agentDesc.prometheus",
    tooltipKey: "omo.agentTooltip.prometheus",
    recommended: "claude-opus-4-6",
    group: "main",
  },
  {
    key: "atlas",
    display: "Atlas",
    descKey: "omo.agentDesc.atlas",
    tooltipKey: "omo.agentTooltip.atlas",
    recommended: "kimi-k2.5",
    group: "main",
  },
  {
    key: "oracle",
    display: "Oracle",
    descKey: "omo.agentDesc.oracle",
    tooltipKey: "omo.agentTooltip.oracle",
    recommended: "gpt-5.2",
    group: "sub",
  },
  {
    key: "librarian",
    display: "Librarian",
    descKey: "omo.agentDesc.librarian",
    tooltipKey: "omo.agentTooltip.librarian",
    recommended: "gemini-3-flash",
    group: "sub",
  },
  {
    key: "explore",
    display: "Explore",
    descKey: "omo.agentDesc.explore",
    tooltipKey: "omo.agentTooltip.explore",
    recommended: "grok-code-fast-1",
    group: "sub",
  },
  {
    key: "multimodal-looker",
    display: "Multimodal-Looker",
    descKey: "omo.agentDesc.multimodalLooker",
    tooltipKey: "omo.agentTooltip.multimodalLooker",
    recommended: "kimi-k2.5",
    group: "sub",
  },
  {
    key: "metis",
    display: "Metis",
    descKey: "omo.agentDesc.metis",
    tooltipKey: "omo.agentTooltip.metis",
    recommended: "claude-opus-4-6",
    group: "sub",
  },
  {
    key: "momus",
    display: "Momus",
    descKey: "omo.agentDesc.momus",
    tooltipKey: "omo.agentTooltip.momus",
    recommended: "gpt-5.2",
    group: "sub",
  },
  {
    key: "sisyphus-junior",
    display: "Sisyphus-Junior",
    descKey: "omo.agentDesc.sisyphusJunior",
    tooltipKey: "omo.agentTooltip.sisyphusJunior",
    group: "sub",
  },
];

export const OMO_BUILTIN_CATEGORIES: OmoCategoryDef[] = [
  {
    key: "visual-engineering",
    display: "Visual Engineering",
    descKey: "omo.categoryDesc.visualEngineering",
    tooltipKey: "omo.categoryTooltip.visualEngineering",
    recommended: "gemini-3-pro",
  },
  {
    key: "ultrabrain",
    display: "Ultrabrain",
    descKey: "omo.categoryDesc.ultrabrain",
    tooltipKey: "omo.categoryTooltip.ultrabrain",
    recommended: "gpt-5.3-codex",
  },
  {
    key: "deep",
    display: "Deep",
    descKey: "omo.categoryDesc.deep",
    tooltipKey: "omo.categoryTooltip.deep",
    recommended: "gpt-5.3-codex",
  },
  {
    key: "artistry",
    display: "Artistry",
    descKey: "omo.categoryDesc.artistry",
    tooltipKey: "omo.categoryTooltip.artistry",
    recommended: "gemini-3-pro",
  },
  {
    key: "quick",
    display: "Quick",
    descKey: "omo.categoryDesc.quick",
    tooltipKey: "omo.categoryTooltip.quick",
    recommended: "claude-haiku-4-5",
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
    recommended: "claude-opus-4-6",
  },
  {
    key: "writing",
    display: "Writing",
    descKey: "omo.categoryDesc.writing",
    tooltipKey: "omo.categoryTooltip.writing",
    recommended: "gemini-3-flash",
  },
];

export const OMO_DISABLEABLE_AGENTS = [
  { value: "Prometheus (Planner)", label: "Prometheus (Planner)" },
  { value: "Atlas", label: "Atlas" },
  { value: "oracle", label: "Oracle" },
  { value: "librarian", label: "Librarian" },
  { value: "explore", label: "Explore" },
  { value: "multimodal-looker", label: "Multimodal Looker" },
  { value: "frontend-ui-ux-engineer", label: "Frontend UI/UX Engineer" },
  { value: "document-writer", label: "Document Writer" },
  { value: "Sisyphus-Junior", label: "Sisyphus-Junior" },
  { value: "Metis (Plan Consultant)", label: "Metis (Plan Consultant)" },
  { value: "Momus (Plan Reviewer)", label: "Momus (Plan Reviewer)" },
  { value: "OpenCode-Builder", label: "OpenCode-Builder" },
] as const;

export const OMO_DISABLEABLE_MCPS = [
  { value: "context7", label: "context7" },
  { value: "grep_app", label: "grep_app" },
  { value: "websearch", label: "websearch" },
] as const;

export const OMO_DISABLEABLE_HOOKS = [
  { value: "todo-continuation-enforcer", label: "todo-continuation-enforcer" },
  { value: "context-window-monitor", label: "context-window-monitor" },
  { value: "session-recovery", label: "session-recovery" },
  { value: "session-notification", label: "session-notification" },
  { value: "comment-checker", label: "comment-checker" },
  { value: "grep-output-truncator", label: "grep-output-truncator" },
  { value: "tool-output-truncator", label: "tool-output-truncator" },
  {
    value: "directory-agents-injector",
    label: "directory-agents-injector",
  },
  {
    value: "directory-readme-injector",
    label: "directory-readme-injector",
  },
  {
    value: "empty-task-response-detector",
    label: "empty-task-response-detector",
  },
  { value: "think-mode", label: "think-mode" },
  {
    value: "anthropic-context-window-limit-recovery",
    label: "anthropic-context-window-limit-recovery",
  },
  { value: "rules-injector", label: "rules-injector" },
  { value: "background-notification", label: "background-notification" },
  { value: "auto-update-checker", label: "auto-update-checker" },
  { value: "startup-toast", label: "startup-toast" },
  { value: "keyword-detector", label: "keyword-detector" },
  { value: "agent-usage-reminder", label: "agent-usage-reminder" },
  { value: "non-interactive-env", label: "non-interactive-env" },
  { value: "interactive-bash-session", label: "interactive-bash-session" },
  {
    value: "compaction-context-injector",
    label: "compaction-context-injector",
  },
  {
    value: "thinking-block-validator",
    label: "thinking-block-validator",
  },
  { value: "claude-code-hooks", label: "claude-code-hooks" },
  { value: "ralph-loop", label: "ralph-loop" },
  { value: "preemptive-compaction", label: "preemptive-compaction" },
] as const;

export const OMO_DISABLEABLE_SKILLS = [
  { value: "playwright", label: "playwright" },
  { value: "agent-browser", label: "agent-browser" },
  { value: "git-master", label: "git-master" },
] as const;

export const OMO_DEFAULT_SCHEMA_URL =
  "https://raw.githubusercontent.com/code-yeongyu/oh-my-opencode/master/assets/oh-my-opencode.schema.json";

export const OMO_SISYPHUS_AGENT_PLACEHOLDER = `{
  "disabled": false,
  "default_builder_enabled": false,
  "planner_enabled": true,
  "replace_plan": true
}`;

export const OMO_LSP_PLACEHOLDER = `{
  "typescript-language-server": {
    "command": ["typescript-language-server", "--stdio"],
    "extensions": [".ts", ".tsx"],
    "priority": 10
  },
  "pylsp": {
    "disabled": true
  }
}`;

export const OMO_EXPERIMENTAL_PLACEHOLDER = `{
  "truncate_all_tool_outputs": true,
  "aggressive_truncation": true,
  "auto_resume": true
}`;

export const OMO_BACKGROUND_TASK_PLACEHOLDER = `{
  "defaultConcurrency": 5,
  "providerConcurrency": {
    "anthropic": 3,
    "openai": 5,
    "google": 10
  },
  "modelConcurrency": {
    "anthropic/claude-opus-4-6": 2,
    "google/gemini-3-flash": 10
  }
}`;

export const OMO_BROWSER_AUTOMATION_PLACEHOLDER = `{
  "provider": "playwright"
}`;

export const OMO_CLAUDE_CODE_PLACEHOLDER = `{
  "mcp": true,
  "commands": true,
  "skills": true,
  "agents": true,
  "hooks": true,
  "plugins": true
}`;

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
    recommended: "claude-opus-4-6",
    group: "main",
  },
  {
    key: "oracle",
    display: "Oracle",
    descKey: "omo.slimAgentDesc.oracle",
    tooltipKey: "omo.slimAgentTooltip.oracle",
    recommended: "gpt-5.2",
    group: "sub",
  },
  {
    key: "librarian",
    display: "Librarian",
    descKey: "omo.slimAgentDesc.librarian",
    tooltipKey: "omo.slimAgentTooltip.librarian",
    recommended: "gemini-3-flash",
    group: "sub",
  },
  {
    key: "explorer",
    display: "Explorer",
    descKey: "omo.slimAgentDesc.explorer",
    tooltipKey: "omo.slimAgentTooltip.explorer",
    recommended: "grok-code-fast-1",
    group: "sub",
  },
  {
    key: "designer",
    display: "Designer",
    descKey: "omo.slimAgentDesc.designer",
    tooltipKey: "omo.slimAgentTooltip.designer",
    recommended: "gemini-3-pro",
    group: "sub",
  },
  {
    key: "fixer",
    display: "Fixer",
    descKey: "omo.slimAgentDesc.fixer",
    tooltipKey: "omo.slimAgentTooltip.fixer",
    recommended: "gpt-5.3-codex",
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
] as const;

export const OMO_SLIM_DISABLEABLE_MCPS = [
  { value: "context7", label: "context7" },
  { value: "grep_app", label: "grep_app" },
  { value: "websearch", label: "websearch" },
] as const;

export const OMO_SLIM_DISABLEABLE_HOOKS = [
  { value: "auto-update-checker", label: "auto-update-checker" },
  { value: "phase-reminder", label: "phase-reminder" },
  { value: "post-read-nudge", label: "post-read-nudge" },
] as const;

export const OMO_SLIM_DEFAULT_SCHEMA_URL =
  "https://raw.githubusercontent.com/alvinunreal/oh-my-opencode-slim/master/assets/oh-my-opencode-slim.schema.json";

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
