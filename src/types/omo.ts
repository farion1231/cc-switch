export interface OmoGlobalConfig {
  id: string;
  schemaUrl?: string;
  sisyphusAgent?: Record<string, unknown>;
  disabledAgents: string[];
  disabledMcps: string[];
  disabledHooks: string[];
  disabledSkills: string[];
  lsp?: Record<string, unknown>;
  experimental?: Record<string, unknown>;
  backgroundTask?: Record<string, unknown>;
  browserAutomationEngine?: Record<string, unknown>;
  claudeCode?: Record<string, unknown>;
  otherFields?: Record<string, unknown>;
  updatedAt: string;
}

export interface OmoLocalFileData {
  agents?: Record<string, Record<string, unknown>>;
  categories?: Record<string, Record<string, unknown>>;
  otherFields?: Record<string, unknown>;
  global: OmoGlobalConfig;
  filePath: string;
  lastModified?: string;
}

export interface OmoAgentDef {
  key: string;
  display: string;
  descZh: string;
  descEn: string;
  recommended?: string;
  group: "main" | "sub";
}

export interface OmoCategoryDef {
  key: string;
  display: string;
  descZh: string;
  descEn: string;
  recommended?: string;
}

export const OMO_BUILTIN_AGENTS: OmoAgentDef[] = [
  {
    key: "Sisyphus",
    display: "Sisyphus",
    descZh: "主编排者",
    descEn: "Main orchestrator",
    recommended: "claude-opus-4-6",
    group: "main",
  },
  {
    key: "Hephaestus",
    display: "Hephaestus",
    descZh: "自主深度工作者",
    descEn: "Autonomous deep worker",
    recommended: "gpt-5.3-codex",
    group: "main",
  },
  {
    key: "Prometheus",
    display: "Prometheus",
    descZh: "战略规划者",
    descEn: "Strategic planner",
    recommended: "claude-opus-4-6",
    group: "main",
  },
  {
    key: "Atlas",
    display: "Atlas",
    descZh: "任务管理者",
    descEn: "Task manager",
    recommended: "kimi-k2.5",
    group: "main",
  },
  {
    key: "oracle",
    display: "Oracle",
    descZh: "战略顾问",
    descEn: "Strategic advisor",
    recommended: "gpt-5.3",
    group: "sub",
  },
  {
    key: "librarian",
    display: "Librarian",
    descZh: "多仓库研究员",
    descEn: "Multi-repo researcher",
    recommended: "glm-4.7",
    group: "sub",
  },
  {
    key: "explore",
    display: "Explore",
    descZh: "快速代码搜索",
    descEn: "Fast code search",
    recommended: "grok-code-fast-1",
    group: "sub",
  },
  {
    key: "multimodal-looker",
    display: "Multimodal-Looker",
    descZh: "媒体分析器",
    descEn: "Media analyzer",
    recommended: "gemini-3-flash",
    group: "sub",
  },
  {
    key: "Metis",
    display: "Metis",
    descZh: "规划前分析顾问",
    descEn: "Pre-plan analysis advisor",
    recommended: "claude-opus-4-6",
    group: "sub",
  },
  {
    key: "Momus",
    display: "Momus",
    descZh: "计划审查者",
    descEn: "Plan reviewer",
    recommended: "gpt-5.3",
    group: "sub",
  },
  {
    key: "Sisyphus-Junior",
    display: "Sisyphus-Junior",
    descZh: "委托任务执行器",
    descEn: "Delegated task executor",
    group: "sub",
  },
];

export const OMO_BUILTIN_CATEGORIES: OmoCategoryDef[] = [
  {
    key: "visual-engineering",
    display: "Visual Engineering",
    descZh: "视觉/前端工程",
    descEn: "Visual/frontend engineering",
    recommended: "gemini-3-pro",
  },
  {
    key: "ultrabrain",
    display: "Ultrabrain",
    descZh: "超级思考",
    descEn: "Ultra thinking",
    recommended: "claude-opus-4-6",
  },
  {
    key: "deep",
    display: "Deep",
    descZh: "深度工作",
    descEn: "Deep work",
    recommended: "gpt-5.3-codex",
  },
  {
    key: "artistry",
    display: "Artistry",
    descZh: "创意/文艺",
    descEn: "Creative/artistic",
    recommended: "claude-opus-4-6",
  },
  {
    key: "quick",
    display: "Quick",
    descZh: "快速响应",
    descEn: "Quick response",
    recommended: "gemini-3-flash",
  },
  {
    key: "unspecified-low",
    display: "Unspecified Low",
    descZh: "通用低配",
    descEn: "General low tier",
    recommended: "gemini-3-flash",
  },
  {
    key: "unspecified-high",
    display: "Unspecified High",
    descZh: "通用高配",
    descEn: "General high tier",
    recommended: "gpt-5.3-codex",
  },
  {
    key: "writing",
    display: "Writing",
    descZh: "写作",
    descEn: "Writing",
    recommended: "claude-opus-4-6",
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

export function mergeOmoConfigPreview(
  global: OmoGlobalConfig,
  agents: Record<string, Record<string, unknown>>,
  categories: Record<string, Record<string, unknown>>,
  otherFieldsStr: string,
): Record<string, unknown> {
  const result: Record<string, unknown> = {};

  if (global.schemaUrl) result["$schema"] = global.schemaUrl;

  if (global.sisyphusAgent) result["sisyphus_agent"] = global.sisyphusAgent;
  if (global.disabledAgents?.length)
    result["disabled_agents"] = global.disabledAgents;
  if (global.disabledMcps?.length)
    result["disabled_mcps"] = global.disabledMcps;
  if (global.disabledHooks?.length)
    result["disabled_hooks"] = global.disabledHooks;
  if (global.disabledSkills?.length)
    result["disabled_skills"] = global.disabledSkills;
  if (global.lsp) result["lsp"] = global.lsp;
  if (global.experimental) result["experimental"] = global.experimental;
  if (global.backgroundTask) result["background_task"] = global.backgroundTask;
  if (global.browserAutomationEngine)
    result["browser_automation_engine"] = global.browserAutomationEngine;
  if (global.claudeCode) result["claude_code"] = global.claudeCode;

  if (global.otherFields) {
    for (const [k, v] of Object.entries(global.otherFields)) {
      result[k] = v;
    }
  }

  if (Object.keys(agents).length > 0) result["agents"] = agents;
  if (Object.keys(categories).length > 0) result["categories"] = categories;
  try {
    const other = JSON.parse(otherFieldsStr || "{}");
    for (const [k, v] of Object.entries(other)) {
      result[k] = v;
    }
  } catch {}

  return result;
}
