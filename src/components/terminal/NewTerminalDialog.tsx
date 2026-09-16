import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { FolderOpen, Loader2, History } from "lucide-react";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { buildHostOptions, getDefaultTerminalHost } from "./terminalHosts";
import {
  TERMINAL_TOOL_LABEL,
  TERMINAL_TOOL_OPTIONS,
  type CreateTerminalPayload,
  type TerminalTool,
  type ToolSessionInfo,
} from "@/types/terminal";

export interface NewTerminalDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** 当前选中的应用 id */
  appId: string;
  /** 当前选中的应用展示名 */
  appLabel: string;
  projectDirs: string[];
  defaultProjectDir?: string;
  /** 所属开发项目 id（可选，提交时记录到终端实例） */
  projectId?: string;
  availableTerminals: string[];
  /** 各 AI 工具的启动参数模板（来自设置），新建时预填 */
  launchTemplates?: Partial<Record<TerminalTool, string>>;
  /** 调用系统文件夹选择对话框 */
  onBrowse: () => Promise<string | null>;
  /** 获取工具本机会话上下文（Claude Code / OpenCode） */
  onListSessions?: (
    tool: TerminalTool,
    projectDir: string,
  ) => Promise<ToolSessionInfo[]>;
  /** 提交新建；返回新建实例 id（失败返回 null），调用方据此自动选中新终端。 */
  onSubmit: (payload: CreateTerminalPayload) => Promise<string | null>;
}

/** 终端工具 → 对应的应用 id（用于决定注入哪套环境变量）。 */
const TOOL_APP_MAP: Record<TerminalTool, string | null> = {
  claude: "claude",
  codex: "codex",
  gemini: "gemini",
  opencode: "opencode",
  openclaw: "openclaw",
  hermes: "hermes",
  pi: "pi",
  shell: null,
  custom: null,
};

/** 支持“最近会话”恢复的工具（启动参数里注入 resume/session 标记）。 */
const SESSION_TOOLS: TerminalTool[] = ["claude", "opencode"];

/** 工具 → 会话恢复参数名。 */
const SESSION_ARG_FLAG: Partial<Record<TerminalTool, string>> = {
  claude: "--resume",
  opencode: "--session",
};

/** 需要从参数里剔除的旧会话标记（避免与新的 --resume/--session 冲突）。 */
const SESSION_ARG_TOKENS = new Set([
  "--resume",
  "--session",
  "--continue",
  "-c",
  "-r",
  "-s",
]);

/** 在参数串里替换/追加会话标记。 */
function mergeSessionArg(
  current: string,
  tool: TerminalTool,
  sessionId: string,
): string {
  const flag = SESSION_ARG_FLAG[tool] ?? "--resume";
  const tokens = current.split(/\s+/).filter(Boolean);
  const next: string[] = [];
  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i];
    if (SESSION_ARG_TOKENS.has(token)) {
      // 跳过标记及其值
      i += 1;
      continue;
    }
    next.push(token);
  }
  next.push(flag, sessionId);
  return next.join(" ");
}

function formatSessionTime(iso?: string): string {
  if (!iso) return "";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/** 启动参数记忆：按工具记住上次填写的参数，新建终端时自动预填。 */
const ARGS_MEMORY_KEY = "cc-switch-terminal-args-v1";

function loadArgsMemory(): Partial<Record<TerminalTool, string>> {
  try {
    const raw = window.localStorage.getItem(ARGS_MEMORY_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch {
    return {};
  }
}

function rememberArgs(
  memory: Partial<Record<TerminalTool, string>>,
  tool: TerminalTool,
  args: string,
): Partial<Record<TerminalTool, string>> {
  const next = { ...memory };
  if (args.trim()) next[tool] = args;
  else delete next[tool];
  try {
    window.localStorage.setItem(ARGS_MEMORY_KEY, JSON.stringify(next));
  } catch {
    // 忽略存储失败
  }
  return next;
}

export function NewTerminalDialog({
  open,
  onOpenChange,
  appId,
  appLabel,
  projectDirs,
  defaultProjectDir,
  projectId,
  availableTerminals,
  launchTemplates,
  onBrowse,
  onListSessions,
  onSubmit,
}: NewTerminalDialogProps) {
  const { t } = useTranslation();

  const [name, setName] = useState("");
  const [projectDir, setProjectDir] = useState(defaultProjectDir ?? "");
  const [tool, setTool] = useState<TerminalTool>("claude");
  const [args, setArgs] = useState("");
  const [customCommand, setCustomCommand] = useState("");
  const [terminalHost, setTerminalHost] = useState(getDefaultTerminalHost());
  const [browsing, setBrowsing] = useState(false);
  const [pending, setPending] = useState(false);
  /** 各工具上次使用的启动参数（localStorage 记忆，优先于设置里的模板） */
  const [argsMemory, setArgsMemory] = useState<
    Partial<Record<TerminalTool, string>>
  >(loadArgsMemory);
  /** 重启应用后是否自动恢复到上次会话（claude / opencode） */
  const [autoResume, setAutoResume] = useState(true);

  // 最近会话
  const [sessions, setSessions] = useState<ToolSessionInfo[]>([]);
  const [sessionsLoading, setSessionsLoading] = useState(false);
  const [selectedSessionId, setSelectedSessionId] = useState("__none__");

  const templateFor = (value: TerminalTool) =>
    launchTemplates?.[value]?.trim() ?? "";

  /** 工具的默认启动参数：优先用上次记忆的值，其次用设置中的模板。 */
  const rememberedOrTemplate = (value: TerminalTool) =>
    argsMemory[value]?.trim() ?? templateFor(value);

  // 每次打开都重置为当前上下文的默认值
  useEffect(() => {
    if (!open) return;
    setName(`${appLabel} 1`);
    setProjectDir(defaultProjectDir ?? "");
    setTool("claude");
    setArgs(rememberedOrTemplate("claude"));
    setCustomCommand("");
    setTerminalHost(getDefaultTerminalHost());
    setSessions([]);
    setSelectedSessionId("__none__");
    setAutoResume(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, appLabel, defaultProjectDir]);

  // 工具跟随当前应用：选中 Codex 时默认起 codex，而不是每次手动改
  useEffect(() => {
    if (!open) return;
    const matched = TERMINAL_TOOL_OPTIONS.find((option) => option === appId);
    if (matched) {
      setTool(matched);
      setArgs(rememberedOrTemplate(matched));
      setSessions([]);
      setSelectedSessionId("__none__");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, appId]);

  // 工具切换：重置启动参数为记忆值/模板值，并清空会话选择
  const handleToolChange = (value: TerminalTool) => {
    setTool(value);
    setArgs(rememberedOrTemplate(value));
    setSessions([]);
    setSelectedSessionId("__none__");
  };

  // 会话上下文加载：工具为 claude/opencode 且已选目录时拉取最近会话
  const canLoadSessions =
    SESSION_TOOLS.includes(tool) && projectDir.trim().length > 0;

  useEffect(() => {
    if (!open || !canLoadSessions) {
      setSessions([]);
      setSelectedSessionId("__none__");
      return;
    }
    let cancelled = false;
    setSessionsLoading(true);
    const load = async () => {
      try {
        const items = await onListSessions?.(tool, projectDir.trim());
        if (cancelled) return;
        setSessions(items ?? []);
        setSelectedSessionId("__none__");
      } catch (error) {
        console.error("[NewTerminalDialog] Failed to list sessions:", error);
        if (!cancelled) setSessions([]);
      } finally {
        if (!cancelled) setSessionsLoading(false);
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [open, tool, projectDir, canLoadSessions, onListSessions]);

  const hostOptions = useMemo(
    () => buildHostOptions(availableTerminals),
    [availableTerminals],
  );

  // 目录历史里没有当前值时补一个选项，避免 Select 显示空白
  const dirOptions = useMemo(() => {
    const list = projectDirs.filter(Boolean);
    const current = projectDir.trim();
    if (current && !list.includes(current)) {
      return [current, ...list];
    }
    return list;
  }, [projectDirs, projectDir]);

  const trimmedName = name.trim();
  const trimmedDir = projectDir.trim();
  const trimmedCommand = customCommand.trim();
  const canSubmit =
    trimmedName.length > 0 &&
    trimmedDir.length > 0 &&
    (tool !== "custom" || trimmedCommand.length > 0);

  const handleBrowse = async () => {
    setBrowsing(true);
    try {
      const dir = await onBrowse();
      if (dir) setProjectDir(dir);
    } finally {
      setBrowsing(false);
    }
  };

  const handleSessionSelect = (value: string) => {
    setSelectedSessionId(value);
    if (value === "__none__") return;
    const session = sessions.find((item) => item.sessionId === value);
    if (session) {
      setArgs((prev) => mergeSessionArg(prev, tool, session.sessionId));
    }
  };

  const handleSubmit = async () => {
    if (!canSubmit || pending) return;
    setPending(true);
    try {
      // 记住本次使用的启动参数，下次新建同工具终端时自动预填
      if (tool !== "shell" && tool !== "custom") {
        setArgsMemory((prev) => rememberArgs(prev, tool, args));
      }
      const createdId = await onSubmit({
        name: trimmedName,
        app: TOOL_APP_MAP[tool] ?? appId,
        projectDir: trimmedDir,
        tool,
        projectId: projectId || undefined,
        customCommand: tool === "custom" ? trimmedCommand : null,
        args: tool === "shell" || tool === "custom" ? null : args.trim(),
        terminal: terminalHost || null,
        autoResume,
      });
      if (createdId) onOpenChange(false);
    } finally {
      setPending(false);
    }
  };

  const isAiTool = tool !== "shell" && tool !== "custom";

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && !pending) onOpenChange(false);
      }}
    >
      <DialogContent className="flex max-h-[85vh] max-w-lg flex-col overflow-hidden rounded-2xl">
        <DialogHeader className="shrink-0 border-b border-border/60 pb-4">
          <DialogTitle>
            {t("terminalHub.newDialog.title", { defaultValue: "新建终端" })}
          </DialogTitle>
          <DialogDescription>
            {t("terminalHub.newDialog.description", {
              defaultValue: "在指定项目目录下用系统原生终端运行 AI CLI。",
            })}
          </DialogDescription>
        </DialogHeader>

        <div className="min-h-0 flex-1 space-y-5 overflow-y-auto py-3 pr-1">
          <div className="space-y-2">
            <Label htmlFor="terminal-name">
              {t("terminalHub.newDialog.name", { defaultValue: "终端名称" })}
            </Label>
            <Input
              id="terminal-name"
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder={t("terminalHub.newDialog.namePlaceholder", {
                defaultValue: "例如：主项目 Claude",
              })}
            />
          </div>

          <div className="space-y-2">
            <Label>
              {t("terminalHub.projectDir", { defaultValue: "项目目录" })}
            </Label>
            <div className="flex gap-2">
              <Select value={projectDir} onValueChange={setProjectDir}>
                <SelectTrigger className="min-w-0 flex-1">
                  <SelectValue
                    placeholder={t("terminalHub.selectProjectDir", {
                      defaultValue: "选择项目目录",
                    })}
                  />
                </SelectTrigger>
                <SelectContent>
                  {dirOptions.length === 0 ? (
                    <SelectItem value="__empty__" disabled>
                      {t("terminalHub.noProjectDir", {
                        defaultValue: "暂无历史目录，请点击浏览…",
                      })}
                    </SelectItem>
                  ) : (
                    dirOptions.map((dir) => (
                      <SelectItem key={dir} value={dir} className="font-mono">
                        {dir}
                      </SelectItem>
                    ))
                  )}
                </SelectContent>
              </Select>
              <Button
                type="button"
                variant="outline"
                onClick={() => void handleBrowse()}
                disabled={browsing}
                className="shrink-0"
              >
                <FolderOpen className="mr-1.5 h-4 w-4" />
                {t("terminalHub.browse", { defaultValue: "浏览…" })}
              </Button>
            </div>
          </div>

          <div className="space-y-3 rounded-xl border border-border/60 bg-muted/20 p-3">
            <div className="space-y-2">
              <Label>
                {t("terminalHub.newDialog.tool", { defaultValue: "执行工具" })}
              </Label>
              <Select
                value={tool}
                onValueChange={(value) =>
                  handleToolChange(value as TerminalTool)
                }
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {TERMINAL_TOOL_OPTIONS.map((option) => (
                    <SelectItem key={option} value={option}>
                      {TERMINAL_TOOL_LABEL[option]}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {isAiTool ? (
              <div className="space-y-2">
                <Label htmlFor="terminal-args">
                  {t("terminalHub.newDialog.launchArgs", {
                    defaultValue: "启动参数",
                  })}
                </Label>
                <Input
                  id="terminal-args"
                  value={args}
                  onChange={(event) => setArgs(event.target.value)}
                  placeholder={t(
                    "terminalHub.newDialog.launchArgsPlaceholder",
                    {
                      defaultValue: "如 --model sonnet --verbose（可选）",
                    },
                  )}
                  className="font-mono"
                />
                <p className="text-[11px] leading-relaxed text-muted-foreground">
                  {t("terminalHub.newDialog.launchArgsHint", {
                    defaultValue:
                      "启动参数会追加在工具名之后；可在设置中配置各工具的默认模板。",
                  })}
                </p>
                {/* 会话恢复开关：仅对支持 --resume/--session 的工具有意义 */}
                {SESSION_TOOLS.includes(tool) ? (
                  <label className="flex cursor-pointer items-start gap-2 rounded-lg border border-border/60 bg-background/60 px-2.5 py-2">
                    <input
                      type="checkbox"
                      checked={autoResume}
                      onChange={(event) => setAutoResume(event.target.checked)}
                      className="mt-0.5 h-3.5 w-3.5 accent-emerald-600"
                    />
                    <span className="min-w-0">
                      <span className="block text-xs font-medium text-foreground">
                        重启后自动恢复会话
                      </span>
                      <span className="mt-0.5 block text-[11px] leading-relaxed text-muted-foreground">
                        应用重启时自动回滚终端内容，并带上{" "}
                        {tool === "claude" ? "--resume" : "--session"}{" "}
                        恢复上次的对话；关闭则每次都开全新会话。
                      </span>
                    </span>
                  </label>
                ) : null}
              </div>
            ) : null}

            {tool === "custom" ? (
              <div className="space-y-2">
                <Label htmlFor="terminal-command">
                  {t("terminalHub.newDialog.customCommand", {
                    defaultValue: "自定义命令",
                  })}
                </Label>
                <Input
                  id="terminal-command"
                  value={customCommand}
                  onChange={(event) => setCustomCommand(event.target.value)}
                  placeholder="npm run dev"
                  className="font-mono"
                />
              </div>
            ) : null}
          </div>

          {canLoadSessions ? (
            <div className="space-y-2 rounded-xl border border-border/60 bg-muted/20 p-3">
              <div className="flex items-center gap-2">
                <History className="h-3.5 w-3.5 text-muted-foreground" />
                <Label className="text-xs font-medium">
                  {t("terminalHub.newDialog.sessionContext", {
                    defaultValue: "最近会话",
                  })}
                </Label>
              </div>
              <div className="flex items-center gap-2">
                <Select
                  value={selectedSessionId}
                  onValueChange={handleSessionSelect}
                  disabled={sessionsLoading}
                >
                  <SelectTrigger className="min-w-0 flex-1">
                    <SelectValue
                      placeholder={
                        sessionsLoading
                          ? t("terminalHub.newDialog.sessionLoading", {
                              defaultValue: "加载会话中…",
                            })
                          : t("terminalHub.newDialog.sessionPlaceholder", {
                              defaultValue: "不恢复会话（新会话）",
                            })
                      }
                    />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__none__">
                      {t("terminalHub.newDialog.sessionPlaceholder", {
                        defaultValue: "不恢复会话（新会话）",
                      })}
                    </SelectItem>
                    {sessions.length === 0 && !sessionsLoading ? (
                      <SelectItem value="__empty__" disabled>
                        {t("terminalHub.newDialog.sessionEmpty", {
                          defaultValue: "该目录下未找到可恢复的会话",
                        })}
                      </SelectItem>
                    ) : (
                      sessions.map((session) => (
                        <SelectItem
                          key={session.sessionId}
                          value={session.sessionId}
                        >
                          <span className="flex items-center gap-2">
                            <span className="min-w-0 flex-1 truncate">
                              {session.title || session.sessionId.slice(0, 12)}
                            </span>
                            {session.lastActiveAt ? (
                              <span className="shrink-0 font-mono text-[10px] text-muted-foreground">
                                {formatSessionTime(session.lastActiveAt)}
                              </span>
                            ) : null}
                          </span>
                        </SelectItem>
                      ))
                    )}
                  </SelectContent>
                </Select>
                {sessionsLoading ? (
                  <Loader2 className="h-4 w-4 shrink-0 animate-spin text-muted-foreground" />
                ) : (
                  <History className="h-4 w-4 shrink-0 text-muted-foreground/60" />
                )}
              </div>
              <p className="text-[11px] leading-relaxed text-muted-foreground">
                {t("terminalHub.newDialog.sessionHint", {
                  tool: TERMINAL_TOOL_LABEL[tool],
                  flag: SESSION_ARG_FLAG[tool] ?? "--resume",
                  defaultValue: `选择会话后会自动加入启动参数，恢复 ${TERMINAL_TOOL_LABEL[tool]} 之前的对话上下文。`,
                })}
              </p>
            </div>
          ) : null}

          <div className="space-y-2 rounded-xl border border-border/60 bg-muted/20 p-3">
            <Label>
              {t("terminalHub.newDialog.terminal", {
                defaultValue: "终端程序",
              })}
            </Label>
            <Select value={terminalHost} onValueChange={setTerminalHost}>
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {hostOptions.map((option) => (
                  <SelectItem key={option.value} value={option.value}>
                    {option.labelKey ? t(option.labelKey) : option.value}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>

        <DialogFooter className="shrink-0 gap-2 border-t border-border/60 pt-4">
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={pending}
          >
            {t("common.cancel")}
          </Button>
          <Button
            onClick={() => void handleSubmit()}
            disabled={!canSubmit || pending}
          >
            {t("common.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}