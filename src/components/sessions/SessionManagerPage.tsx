import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  Copy,
  RefreshCw,
  Search,
  Play,
  MessageSquare,
  Clock,
  FolderOpen,
  ChevronRight,
  Terminal,
} from "lucide-react";
import { useSessionMessagesQuery, useSessionsQuery } from "@/lib/query";
import { sessionsApi } from "@/lib/api";
import type { SessionMeta, SessionMessage } from "@/types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { extractErrorMessage } from "@/utils/errorUtils";
import { isMac } from "@/lib/platform";
import { cn } from "@/lib/utils";
import { ProviderIcon } from "@/components/ProviderIcon";

const TERMINAL_TARGET_KEY = "session_manager_terminal_target";

type TerminalTarget = "terminal" | "kitty" | "copy";

type ProviderFilter = "all" | "codex" | "claude";

const getSessionKey = (session: SessionMeta) =>
  `${session.providerId}:${session.sessionId}:${session.sourcePath ?? ""}`;

const getBaseName = (value?: string | null) => {
  if (!value) return "";
  const trimmed = value.trim();
  if (!trimmed) return "";
  const normalized = trimmed.replace(/[\\/]+$/, "");
  const parts = normalized.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || trimmed;
};

const formatTimestamp = (value?: number) => {
  if (!value) return "";
  return new Date(value).toLocaleString();
};

const formatRelativeTime = (value?: number) => {
  if (!value) return "";
  const now = Date.now();
  const diff = now - value;
  const minutes = Math.floor(diff / 60000);
  const hours = Math.floor(diff / 3600000);
  const days = Math.floor(diff / 86400000);

  if (minutes < 1) return "刚刚";
  if (minutes < 60) return `${minutes} 分钟前`;
  if (hours < 24) return `${hours} 小时前`;
  if (days < 7) return `${days} 天前`;
  return new Date(value).toLocaleDateString();
};

const getProviderLabel = (providerId: string, t: (key: string) => string) => {
  const key = `apps.${providerId}`;
  const translated = t(key);
  return translated === key ? providerId : translated;
};

// 根据 providerId 获取对应的图标名称
const getProviderIconName = (providerId: string) => {
  if (providerId === "codex") return "openai";
  if (providerId === "claude") return "claude";
  return providerId;
};

const getRoleTone = (role: string) => {
  const normalized = role.toLowerCase();
  if (normalized === "assistant") return "text-blue-500";
  if (normalized === "user") return "text-emerald-500";
  if (normalized === "system") return "text-amber-500";
  if (normalized === "tool") return "text-purple-500";
  return "text-muted-foreground";
};

const getRoleLabel = (role: string) => {
  const normalized = role.toLowerCase();
  if (normalized === "assistant") return "AI";
  if (normalized === "user") return "用户";
  if (normalized === "system") return "系统";
  if (normalized === "tool") return "工具";
  return role;
};

const formatSessionTitle = (session: SessionMeta) => {
  return (
    session.title ||
    getBaseName(session.projectDir) ||
    session.sessionId.slice(0, 8)
  );
};

export function SessionManagerPage() {
  const { t } = useTranslation();
  const { data, isLoading, refetch } = useSessionsQuery();
  const sessions = data ?? [];
  const detailRef = useRef<HTMLDivElement | null>(null);
  const messagesEndRef = useRef<HTMLDivElement | null>(null);

  const [search, setSearch] = useState("");
  const [providerFilter, setProviderFilter] = useState<ProviderFilter>("all");
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [terminalTarget, setTerminalTarget] = useState<TerminalTarget>(() => {
    if (typeof window === "undefined") return "terminal";
    const stored = window.localStorage.getItem(
      TERMINAL_TARGET_KEY
    ) as TerminalTarget | null;
    return stored ?? "terminal";
  });

  useEffect(() => {
    if (typeof window !== "undefined") {
      window.localStorage.setItem(TERMINAL_TARGET_KEY, terminalTarget);
    }
  }, [terminalTarget]);

  const filteredSessions = useMemo(() => {
    const needle = search.trim().toLowerCase();
    const filtered = sessions.filter((session) => {
      if (providerFilter !== "all" && session.providerId !== providerFilter) {
        return false;
      }
      if (!needle) return true;
      const haystack = [
        session.sessionId,
        session.title,
        session.summary,
        session.projectDir,
        session.sourcePath,
      ]
        .filter(Boolean)
        .join(" ")
        .toLowerCase();
      return haystack.includes(needle);
    });

    return [...filtered].sort((a, b) => {
      const aTs = a.lastActiveAt ?? a.createdAt ?? 0;
      const bTs = b.lastActiveAt ?? b.createdAt ?? 0;
      return bTs - aTs;
    });
  }, [providerFilter, search, sessions]);

  useEffect(() => {
    if (filteredSessions.length === 0) {
      setSelectedKey(null);
      return;
    }
    const exists = selectedKey
      ? filteredSessions.some(
          (session) => getSessionKey(session) === selectedKey
        )
      : false;
    if (!exists) {
      setSelectedKey(getSessionKey(filteredSessions[0]));
    }
  }, [filteredSessions, selectedKey]);

  const selectedSession = useMemo(() => {
    if (!selectedKey) return null;
    return (
      filteredSessions.find(
        (session) => getSessionKey(session) === selectedKey
      ) || null
    );
  }, [filteredSessions, selectedKey]);

  const { data: messages = [], isLoading: isLoadingMessages } =
    useSessionMessagesQuery(
      selectedSession?.providerId,
      selectedSession?.sourcePath
    );

  const handleCopy = async (text: string, successMessage: string) => {
    try {
      await navigator.clipboard.writeText(text);
      toast.success(successMessage);
    } catch (error) {
      toast.error(
        extractErrorMessage(error) ||
          t("common.error", { defaultValue: "Copy failed" })
      );
    }
  };

  const handleResume = async () => {
    if (!selectedSession?.resumeCommand) return;

    if (terminalTarget === "copy" || !isMac()) {
      await handleCopy(
        selectedSession.resumeCommand,
        t("sessionManager.resumeCommandCopied")
      );
      return;
    }

    try {
      await sessionsApi.launchTerminal({
        target: terminalTarget,
        command: selectedSession.resumeCommand,
        cwd: selectedSession.projectDir ?? undefined,
      });
      toast.success(t("sessionManager.terminalLaunched"));
    } catch (error) {
      const fallback = selectedSession.resumeCommand;
      await handleCopy(fallback, t("sessionManager.resumeFallbackCopied"));
      toast.error(extractErrorMessage(error) || t("sessionManager.openFailed"));
    }
  };

  const scrollToDetail = () => {
    detailRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  return (
    <TooltipProvider>
      <div className="mx-auto px-4 sm:px-6 flex flex-col h-[calc(100vh-8rem)]">
        <div className="flex-1 overflow-hidden flex flex-col gap-4">
          {/* 搜索和筛选工具栏 */}
          <div className="flex flex-col gap-3 sm:flex-row sm:items-center bg-background/80 backdrop-blur-sm py-2 -mx-1 px-1">
            <div className="relative flex-1">
              <Search className="absolute left-3 top-2.5 size-4 text-muted-foreground" />
              <Input
                value={search}
                onChange={(event) => setSearch(event.target.value)}
                placeholder={t("sessionManager.searchPlaceholder")}
                className="pl-9"
              />
            </div>

            <div className="flex items-center gap-2">
              <Select
                value={providerFilter}
                onValueChange={(value) =>
                  setProviderFilter(value as ProviderFilter)
                }
              >
                <SelectTrigger className="w-[140px]">
                  <SelectValue
                    placeholder={t("sessionManager.providerFilterAll")}
                  />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">
                    {t("sessionManager.providerFilterAll")}
                  </SelectItem>
                  <SelectItem value="codex">Codex</SelectItem>
                  <SelectItem value="claude">Claude Code</SelectItem>
                </SelectContent>
              </Select>

              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="outline"
                    size="icon"
                    onClick={() => void refetch()}
                  >
                    <RefreshCw className="size-4" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>{t("common.refresh")}</TooltipContent>
              </Tooltip>
            </div>
          </div>

          {/* 主内容区域 - 左右分栏 */}
          <div className="flex-1 overflow-hidden grid gap-4 md:grid-cols-[320px_1fr]">
            {/* 左侧会话列表 */}
            <Card className="flex flex-col overflow-hidden">
              <CardHeader className="py-3 px-4 border-b">
                <div className="flex items-center justify-between">
                  <CardTitle className="text-sm font-medium">
                    {t("sessionManager.sessionList")}
                  </CardTitle>
                  <Badge variant="secondary" className="text-xs">
                    {filteredSessions.length}
                  </Badge>
                </div>
              </CardHeader>
              <CardContent className="flex-1 overflow-hidden p-0">
                <ScrollArea className="h-full">
                  <div className="p-2">
                    {isLoading ? (
                      <div className="flex items-center justify-center py-12">
                        <RefreshCw className="size-5 animate-spin text-muted-foreground" />
                      </div>
                    ) : filteredSessions.length === 0 ? (
                      <div className="flex flex-col items-center justify-center py-12 text-center">
                        <MessageSquare className="size-8 text-muted-foreground/50 mb-2" />
                        <p className="text-sm text-muted-foreground">
                          {t("sessionManager.noSessions")}
                        </p>
                      </div>
                    ) : (
                      <div className="space-y-1">
                        {filteredSessions.map((session) => {
                          const isSelected =
                            selectedKey &&
                            getSessionKey(session) === selectedKey;
                          const title = formatSessionTitle(session);
                          const lastActive =
                            session.lastActiveAt ||
                            session.createdAt ||
                            undefined;

                          return (
                            <button
                              key={getSessionKey(session)}
                              type="button"
                              onClick={() => {
                                setSelectedKey(getSessionKey(session));
                                scrollToDetail();
                              }}
                              className={cn(
                                "w-full text-left rounded-lg px-3 py-2.5 transition-all group",
                                isSelected
                                  ? "bg-primary/10 border border-primary/30"
                                  : "hover:bg-muted/60 border border-transparent"
                              )}
                            >
                              {/* 第一行：Provider Icon + 标题 */}
                              <div className="flex items-center gap-2 mb-1">
                                <Tooltip>
                                  <TooltipTrigger asChild>
                                    <span className="shrink-0">
                                      <ProviderIcon
                                        icon={getProviderIconName(
                                          session.providerId
                                        )}
                                        name={session.providerId}
                                        size={18}
                                      />
                                    </span>
                                  </TooltipTrigger>
                                  <TooltipContent>
                                    {getProviderLabel(session.providerId, t)}
                                  </TooltipContent>
                                </Tooltip>
                                <span className="text-sm font-medium truncate flex-1">
                                  {title}
                                </span>
                                <ChevronRight
                                  className={cn(
                                    "size-4 text-muted-foreground/50 shrink-0 transition-transform",
                                    isSelected && "text-primary rotate-90"
                                  )}
                                />
                              </div>

                              {/* 第二行：时间 */}
                              <div className="flex items-center gap-1 text-[11px] text-muted-foreground">
                                <Clock className="size-3" />
                                <span>
                                  {lastActive
                                    ? formatRelativeTime(lastActive)
                                    : t("common.unknown")}
                                </span>
                              </div>
                            </button>
                          );
                        })}
                      </div>
                    )}
                  </div>
                </ScrollArea>
              </CardContent>
            </Card>

            {/* 右侧会话详情 */}
            <Card
              className="flex flex-col overflow-hidden min-h-0"
              ref={detailRef}
            >
              {!selectedSession ? (
                <div className="flex-1 flex flex-col items-center justify-center text-muted-foreground p-8">
                  <MessageSquare className="size-12 mb-3 opacity-30" />
                  <p className="text-sm">{t("sessionManager.selectSession")}</p>
                </div>
              ) : (
                <>
                  {/* 详情头部 */}
                  <CardHeader className="py-3 px-4 border-b shrink-0">
                    <div className="flex items-start justify-between gap-4">
                      {/* 左侧：会话信息 */}
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2 mb-1">
                          <Tooltip>
                            <TooltipTrigger asChild>
                              <span className="shrink-0">
                                <ProviderIcon
                                  icon={getProviderIconName(
                                    selectedSession.providerId
                                  )}
                                  name={selectedSession.providerId}
                                  size={20}
                                />
                              </span>
                            </TooltipTrigger>
                            <TooltipContent>
                              {getProviderLabel(selectedSession.providerId, t)}
                            </TooltipContent>
                          </Tooltip>
                          <h2 className="text-base font-semibold truncate">
                            {formatSessionTitle(selectedSession)}
                          </h2>
                        </div>

                        {/* 元信息 */}
                        <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
                          <div className="flex items-center gap-1">
                            <Clock className="size-3" />
                            <span>
                              {formatTimestamp(
                                selectedSession.lastActiveAt ??
                                  selectedSession.createdAt
                              )}
                            </span>
                          </div>
                          {selectedSession.projectDir && (
                            <Tooltip>
                              <TooltipTrigger asChild>
                                <button
                                  type="button"
                                  onClick={() =>
                                    void handleCopy(
                                      selectedSession.projectDir!,
                                      t("sessionManager.projectDirCopied")
                                    )
                                  }
                                  className="flex items-center gap-1 hover:text-foreground transition-colors"
                                >
                                  <FolderOpen className="size-3" />
                                  <span className="truncate max-w-[200px]">
                                    {getBaseName(selectedSession.projectDir)}
                                  </span>
                                </button>
                              </TooltipTrigger>
                              <TooltipContent
                                side="bottom"
                                className="max-w-xs"
                              >
                                <p className="font-mono text-xs break-all">
                                  {selectedSession.projectDir}
                                </p>
                                <p className="text-muted-foreground mt-1">
                                  点击复制路径
                                </p>
                              </TooltipContent>
                            </Tooltip>
                          )}
                        </div>
                      </div>

                      {/* 右侧：操作按钮组 */}
                      <div className="flex items-center gap-2 shrink-0">
                        <Select
                          value={terminalTarget}
                          onValueChange={(value) =>
                            setTerminalTarget(value as TerminalTarget)
                          }
                        >
                          <SelectTrigger className="h-8 w-[110px] text-xs">
                            <Terminal className="size-3 mr-1" />
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="terminal">
                              {t("sessionManager.terminalTargetTerminal")}
                            </SelectItem>
                            <SelectItem value="kitty">
                              {t("sessionManager.terminalTargetKitty")}
                            </SelectItem>
                          </SelectContent>
                        </Select>

                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button
                              size="sm"
                              className="gap-1.5"
                              onClick={() => void handleResume()}
                              disabled={!selectedSession.resumeCommand}
                            >
                              <Play className="size-3.5" />
                              <span className="hidden sm:inline">
                                {t("sessionManager.resume", {
                                  defaultValue: "恢复会话",
                                })}
                              </span>
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent>
                            {selectedSession.resumeCommand
                              ? t("sessionManager.resumeTooltip", {
                                  defaultValue: "在终端中恢复此会话",
                                })
                              : t("sessionManager.noResumeCommand", {
                                  defaultValue: "此会话无法恢复",
                                })}
                          </TooltipContent>
                        </Tooltip>
                      </div>
                    </div>

                    {/* 恢复命令预览 */}
                    {selectedSession.resumeCommand && (
                      <div className="mt-3 flex items-center gap-2">
                        <div className="flex-1 rounded-md bg-muted/60 px-3 py-1.5 font-mono text-xs text-muted-foreground truncate">
                          {selectedSession.resumeCommand}
                        </div>
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button
                              variant="ghost"
                              size="icon"
                              className="size-7 shrink-0"
                              onClick={() =>
                                void handleCopy(
                                  selectedSession.resumeCommand!,
                                  t("sessionManager.resumeCommandCopied")
                                )
                              }
                            >
                              <Copy className="size-3.5" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent>
                            {t("sessionManager.copyCommand", {
                              defaultValue: "复制命令",
                            })}
                          </TooltipContent>
                        </Tooltip>
                      </div>
                    )}
                  </CardHeader>

                  {/* 消息列表 */}
                  <CardContent className="flex-1 overflow-hidden p-0">
                    <ScrollArea className="h-full">
                      <div className="p-4">
                        <div className="flex items-center gap-2 mb-3">
                          <MessageSquare className="size-4 text-muted-foreground" />
                          <span className="text-sm font-medium">
                            {t("sessionManager.conversationHistory", {
                              defaultValue: "对话记录",
                            })}
                          </span>
                          <Badge variant="secondary" className="text-xs">
                            {messages.length}
                          </Badge>
                        </div>

                        {isLoadingMessages ? (
                          <div className="flex items-center justify-center py-12">
                            <RefreshCw className="size-5 animate-spin text-muted-foreground" />
                          </div>
                        ) : messages.length === 0 ? (
                          <div className="flex flex-col items-center justify-center py-12 text-center">
                            <MessageSquare className="size-8 text-muted-foreground/50 mb-2" />
                            <p className="text-sm text-muted-foreground">
                              {t("sessionManager.emptySession")}
                            </p>
                          </div>
                        ) : (
                          <div className="space-y-3">
                            {messages.map(
                              (message: SessionMessage, index: number) => (
                                <div
                                  key={`${message.role}-${index}`}
                                  className={cn(
                                    "rounded-lg border px-3 py-2.5 relative group",
                                    message.role.toLowerCase() === "user"
                                      ? "bg-primary/5 border-primary/20 ml-8"
                                      : message.role.toLowerCase() ===
                                          "assistant"
                                        ? "bg-blue-500/5 border-blue-500/20 mr-8"
                                        : "bg-muted/40 border-border/60"
                                  )}
                                >
                                  {/* 悬浮复制按钮 */}
                                  <Tooltip>
                                    <TooltipTrigger asChild>
                                      <Button
                                        variant="ghost"
                                        size="icon"
                                        className="absolute top-2 right-2 size-6 opacity-0 group-hover:opacity-100 transition-opacity"
                                        onClick={() =>
                                          void handleCopy(
                                            message.content,
                                            t("sessionManager.messageCopied", {
                                              defaultValue: "已复制消息内容",
                                            })
                                          )
                                        }
                                      >
                                        <Copy className="size-3" />
                                      </Button>
                                    </TooltipTrigger>
                                    <TooltipContent>
                                      {t("sessionManager.copyMessage", {
                                        defaultValue: "复制内容",
                                      })}
                                    </TooltipContent>
                                  </Tooltip>
                                  <div className="flex items-center justify-between text-xs mb-1.5 pr-6">
                                    <span
                                      className={cn(
                                        "font-semibold",
                                        getRoleTone(message.role)
                                      )}
                                    >
                                      {getRoleLabel(message.role)}
                                    </span>
                                    {message.ts && (
                                      <span className="text-muted-foreground">
                                        {formatTimestamp(message.ts)}
                                      </span>
                                    )}
                                  </div>
                                  <div className="whitespace-pre-wrap text-sm leading-relaxed">
                                    {message.content}
                                  </div>
                                </div>
                              )
                            )}
                            <div ref={messagesEndRef} />
                          </div>
                        )}
                      </div>
                    </ScrollArea>
                  </CardContent>
                </>
              )}
            </Card>
          </div>
        </div>
      </div>
    </TooltipProvider>
  );
}
