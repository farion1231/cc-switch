/** 终端里要执行的 AI 工具；`shell` 只开交互式 shell，`custom` 执行自定义命令。 */
export type TerminalTool =
  | "claude"
  | "codex"
  | "gemini"
  | "opencode"
  | "openclaw"
  | "hermes"
  | "pi"
  | "shell"
  | "custom";

/** 工具 → 展示名（不参与 i18n，属于专有名词）。 */
export const TERMINAL_TOOL_LABEL: Record<TerminalTool, string> = {
  claude: "Claude Code",
  codex: "Codex CLI",
  gemini: "Gemini CLI",
  opencode: "OpenCode",
  openclaw: "OpenClaw",
  hermes: "Hermes",
  pi: "Pi",
  shell: "Shell",
  custom: "Custom",
};

/** 新建终端弹窗里可选的工具（不含 Pi——它不带交互式 CLI）。 */
export const TERMINAL_TOOL_OPTIONS: TerminalTool[] = [
  "claude",
  "opencode",
  "codex",
  "gemini",
  "openclaw",
  "hermes",
  "shell",
  "custom",
];

export interface TerminalInstance {
  id: string;
  name: string;
  /** 所属应用 id（claude / codex / gemini / opencode …），决定注入哪套环境变量 */
  app: string;
  projectDir: string;
  tool: TerminalTool;
  /** 所属开发项目 id（无绑定则为 undefined） */
  projectId?: string;
  customCommand?: string;
  /** 附加启动参数（如 claude --resume <id> / --model sonnet），追加在工具名之后 */
  args?: string;
  /** 覆盖全局 `preferredTerminal` 的终端 id */
  terminal?: string;
  /** 最近一次拉起的可管理 PID；重启应用后可能已失效 */
  pid?: number;
  createdAt: number;
  lastLaunchAt?: number;
  /**
   * 重启应用后是否自动恢复到上次的工具会话（claude / opencode）。
   * 缺省视为 true；置 false 时每次都开全新会话。
   */
  autoResume?: boolean;
  /** 用户显式指定要恢复的会话 id（优先于自动探测最近会话）。 */
  lastSessionId?: string;
}

export interface TerminalHubState {
  instances: TerminalInstance[];
  /** 最近使用过的项目目录，最近使用的在前 */
  projectDirs: string[];
  lastProjectDir?: string;
  expanded: boolean;
}

export interface TerminalLaunchConfig {
  instanceId: string;
  app: string;
  projectDir: string;
  tool: TerminalTool;
  customCommand?: string | null;
  /** 附加启动参数（追加在工具名之后） */
  args?: string | null;
  terminal?: string | null;
  /** 指定 Provider；为空时取该应用当前激活的 Provider */
  providerId?: string | null;
}

export interface CreateTerminalPayload {
  name: string;
  app: string;
  projectDir: string;
  tool: TerminalTool;
  /** 所属开发项目 id（可选） */
  projectId?: string;
  customCommand?: string | null;
  /** 附加启动参数（追加在工具名之后） */
  args?: string | null;
  terminal?: string | null;
  /** 重启应用后是否自动恢复上次会话（claude / opencode） */
  autoResume?: boolean;
}

/** 工具本机会话上下文条目（用于终端面板快速恢复会话）。 */
export interface ToolSessionInfo {
  sessionId: string;
  title?: string;
  lastActiveAt?: string;
  projectDir?: string;
}

/** 内嵌终端（面板内 PTY）推送的输出事件。 */
export type EmbeddedOutputEvent =
  | { kind: "data"; data: number[] }
  | { kind: "exit"; code: number | null };

/** 取终端实例实际会执行的命令（用于列表展示）。 */
export function terminalCommandLabel(instance: TerminalInstance): string {
  if (instance.tool === "shell") return "$SHELL";
  if (instance.tool === "custom") {
    return instance.customCommand?.trim() || "—";
  }
  const args = instance.args?.trim();
  return args ? `${instance.tool} ${args}` : instance.tool;
}
