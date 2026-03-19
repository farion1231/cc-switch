import { useEffect, useMemo, useRef, useState, useCallback } from "react";
import { useSessionSearch } from "@/hooks/useSessionSearch";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  Copy,
  RefreshCw,
  Search,
  Play,
  Trash2,
  MessageSquare,
  Clock,
  FolderOpen,
  X,
  Download,
  Filter,
  ChevronsUpDown,
} from "lucide-react";
import {
  useDeleteSessionMutation,
  useSessionMessagesQuery,
  useSessionsQuery,
} from "@/lib/query";
import { sessionsApi } from "@/lib/api";
import type { SessionMeta } from "@/types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { ScrollArea } from "@/components/ui/scroll-area";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuCheckboxItem,
  DropdownMenuTrigger,
  DropdownMenuSeparator,
  DropdownMenuItem,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import { extractErrorMessage } from "@/utils/errorUtils";
import { isMac } from "@/lib/platform";
import { ProviderIcon } from "@/components/ProviderIcon";
import { SessionItem } from "./SessionItem";
import { SessionMessageItem } from "./SessionMessageItem";
import { SessionTocDialog, SessionTocSidebar } from "./SessionToc";
import {
  formatSessionTitle,
  formatTimestamp,
  getBaseName,
  getProviderIconName,
  getProviderLabel,
  getSessionKey,
  getRoleLabel,
} from "./utils";

type ProviderFilter =
  | "all"
  | "codex"
  | "claude"
  | "opencode"
  | "openclaw"
  | "gemini";

type RoleFilter = "user" | "assistant" | "tool" | "system";

const ALL_ROLES: RoleFilter[] = ["user", "assistant", "tool", "system"];

export function SessionManagerPage({ appId }: { appId: string }) {
  const { t } = useTranslation();
  const { data, isLoading, refetch } = useSessionsQuery();
  const sessions = data ?? [];
  const detailRef = useRef<HTMLDivElement | null>(null);
  const messagesEndRef = useRef<HTMLDivElement | null>(null);
  const messageRefs = useRef<Map<number, HTMLDivElement>>(new Map());
  const [activeMessageIndex, setActiveMessageIndex] = useState<number | null>(
    null,
  );
  const [tocDialogOpen, setTocDialogOpen] = useState(false);
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<SessionMeta | null>(null);
  const searchInputRef = useRef<HTMLInputElement | null>(null);

  const [search, setSearch] = useState("");
  const [providerFilter, setProviderFilter] = useState<ProviderFilter>(
    appId as ProviderFilter,
  );
  const [selectedKey, setSelectedKey] = useState<string | null>(null);

  // Message-level filters
  const [messageSearch, setMessageSearch] = useState("");
  const [isMessageSearchOpen, setIsMessageSearchOpen] = useState(false);
  const messageSearchRef = useRef<HTMLInputElement | null>(null);
  const [activeRoleFilters, setActiveRoleFilters] =
    useState<RoleFilter[]>(ALL_ROLES);
  const [allCollapsed, setAllCollapsed] = useState(false);

  // FlexSearch full-text search
  const { search: searchSessions } = useSessionSearch({
    sessions,
    providerFilter,
  });

  const filteredSessions = useMemo(() => {
    return searchSessions(search);
  }, [searchSessions, search]);

  useEffect(() => {
    if (filteredSessions.length === 0) {
      setSelectedKey(null);
      return;
    }
    const exists = selectedKey
      ? filteredSessions.some(
          (session) => getSessionKey(session) === selectedKey,
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
        (session) => getSessionKey(session) === selectedKey,
      ) || null
    );
  }, [filteredSessions, selectedKey]);

  const { data: messages = [], isLoading: isLoadingMessages } =
    useSessionMessagesQuery(
      selectedSession?.providerId,
      selectedSession?.sourcePath,
    );
  const deleteSessionMutation = useDeleteSessionMutation();

  // Filter messages by role and search
  const filteredMessages = useMemo(() => {
    let result = messages;

    // Role filter
    if (activeRoleFilters.length < ALL_ROLES.length) {
      result = result.filter((msg) =>
        activeRoleFilters.includes(msg.role.toLowerCase() as RoleFilter),
      );
    }

    // Text search within messages
    if (messageSearch.trim()) {
      const query = messageSearch.toLowerCase();
      result = result.filter((msg) =>
        msg.content.toLowerCase().includes(query),
      );
    }

    return result;
  }, [messages, activeRoleFilters, messageSearch]);

  // TOC from user messages
  const userMessagesToc = useMemo(() => {
    return messages
      .map((msg, index) => ({ msg, index }))
      .filter(({ msg }) => msg.role.toLowerCase() === "user")
      .map(({ msg, index }) => ({
        index,
        preview:
          msg.content.slice(0, 50) + (msg.content.length > 50 ? "..." : ""),
        ts: msg.ts,
      }));
  }, [messages]);

  const scrollToMessage = (index: number) => {
    const el = messageRefs.current.get(index);
    if (el) {
      el.scrollIntoView({ behavior: "smooth", block: "center" });
      setActiveMessageIndex(index);
      setTocDialogOpen(false);
      setTimeout(() => setActiveMessageIndex(null), 2000);
    }
  };

  const handleCopy = async (text: string, successMessage: string) => {
    try {
      await navigator.clipboard.writeText(text);
      toast.success(successMessage);
    } catch (error) {
      toast.error(
        extractErrorMessage(error) ||
          t("common.error", { defaultValue: "Copy failed" }),
      );
    }
  };

  const handleResume = async () => {
    if (!selectedSession?.resumeCommand) return;

    if (!isMac()) {
      await handleCopy(
        selectedSession.resumeCommand,
        t("sessionManager.resumeCommandCopied"),
      );
      return;
    }

    try {
      await sessionsApi.launchTerminal({
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

  const handleDeleteConfirm = async () => {
    if (!deleteTarget?.sourcePath || deleteSessionMutation.isPending) {
      return;
    }

    setDeleteTarget(null);
    await deleteSessionMutation.mutateAsync({
      providerId: deleteTarget.providerId,
      sessionId: deleteTarget.sessionId,
      sourcePath: deleteTarget.sourcePath,
    });
  };

  const handleExportConversation = useCallback(async () => {
    if (!selectedSession || messages.length === 0) return;
    const title = formatSessionTitle(selectedSession);
    const lines = [
      `# ${title}`,
      `> Session ID: ${selectedSession.sessionId}`,
      `> Provider: ${selectedSession.providerId}`,
      selectedSession.projectDir
        ? `> Project: ${selectedSession.projectDir}`
        : "",
      `> Exported: ${new Date().toLocaleString()}`,
      "",
      "---",
      "",
    ];

    for (const msg of messages) {
      const roleLabel = getRoleLabel(msg.role, t);
      const ts = msg.ts ? ` (${formatTimestamp(msg.ts)})` : "";
      lines.push(`## ${roleLabel}${ts}`);
      lines.push("");
      lines.push(msg.content);
      lines.push("");
      lines.push("---");
      lines.push("");
    }

    const text = lines.filter((l) => l !== undefined).join("\n");
    await handleCopy(
      text,
      t("sessionManager.conversationExported", {
        defaultValue: "Conversation exported to clipboard",
      }),
    );
  }, [selectedSession, messages, t, handleCopy]);

  const handleCopyAll = useCallback(async () => {
    if (!selectedSession || messages.length === 0) return;
    const text = messages
      .map((msg) => {
        const roleLabel = getRoleLabel(msg.role, t);
        return `[${roleLabel}]\n${msg.content}`;
      })
      .join("\n\n---\n\n");
    await handleCopy(
      text,
      t("sessionManager.allMessagesCopied", {
        defaultValue: "All messages copied",
      }),
    );
  }, [selectedSession, messages, t, handleCopy]);

  const toggleRoleFilter = (role: RoleFilter) => {
    setActiveRoleFilters((prev) => {
      if (prev.includes(role)) {
        const next = prev.filter((r) => r !== role);
        return next.length === 0 ? ALL_ROLES : next;
      }
      return [...prev, role];
    });
  };

  // Role counts for filter badges
  const roleCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const msg of messages) {
      const role = msg.role.toLowerCase();
      counts[role] = (counts[role] || 0) + 1;
    }
    return counts;
  }, [messages]);

  return (
    <TooltipProvider>
      <div className="mx-auto px-4 sm:px-6 flex flex-col flex-1 min-h-0">
        <div className="flex-1 overflow-hidden flex flex-col gap-4">
          {/* Main content - left/right split */}
          <div className="flex-1 overflow-hidden grid gap-4 md:grid-cols-[320px_1fr]">
            {/* Left: Session list */}
            <Card className="flex flex-col overflow-hidden">
              <CardHeader className="py-2 px-3 border-b">
                {isSearchOpen ? (
                  <div className="relative flex-1">
                    <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 size-3.5 text-muted-foreground" />
                    <Input
                      ref={searchInputRef}
                      value={search}
                      onChange={(event) => setSearch(event.target.value)}
                      placeholder={t("sessionManager.searchPlaceholder")}
                      className="h-8 pl-8 pr-8 text-sm"
                      autoFocus
                      onKeyDown={(e) => {
                        if (e.key === "Escape") {
                          setIsSearchOpen(false);
                          setSearch("");
                        }
                      }}
                      onBlur={() => {
                        if (search.trim() === "") {
                          setIsSearchOpen(false);
                        }
                      }}
                    />
                    <Button
                      variant="ghost"
                      size="icon"
                      className="absolute right-1 top-1/2 -translate-y-1/2 size-6"
                      onClick={() => {
                        setIsSearchOpen(false);
                        setSearch("");
                      }}
                    >
                      <X className="size-3" />
                    </Button>
                  </div>
                ) : (
                  <div className="flex items-center justify-between gap-2">
                    <div className="flex items-center gap-2">
                      <CardTitle className="text-sm font-medium">
                        {t("sessionManager.sessionList")}
                      </CardTitle>
                      <Badge variant="secondary" className="text-xs">
                        {filteredSessions.length}
                      </Badge>
                    </div>
                    <div className="flex items-center gap-1">
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="size-7"
                            onClick={() => {
                              setIsSearchOpen(true);
                              setTimeout(
                                () => searchInputRef.current?.focus(),
                                0,
                              );
                            }}
                          >
                            <Search className="size-3.5" />
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent>
                          {t("sessionManager.searchSessions")}
                        </TooltipContent>
                      </Tooltip>

                      <Select
                        value={providerFilter}
                        onValueChange={(value) =>
                          setProviderFilter(value as ProviderFilter)
                        }
                      >
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <SelectTrigger className="size-7 p-0 justify-center border-0 bg-transparent hover:bg-muted">
                              <ProviderIcon
                                icon={
                                  providerFilter === "all"
                                    ? "apps"
                                    : getProviderIconName(providerFilter)
                                }
                                name={providerFilter}
                                size={14}
                              />
                            </SelectTrigger>
                          </TooltipTrigger>
                          <TooltipContent>
                            {providerFilter === "all"
                              ? t("sessionManager.providerFilterAll")
                              : providerFilter}
                          </TooltipContent>
                        </Tooltip>
                        <SelectContent>
                          <SelectItem value="all">
                            <div className="flex items-center gap-2">
                              <ProviderIcon icon="apps" name="all" size={14} />
                              <span>
                                {t("sessionManager.providerFilterAll")}
                              </span>
                            </div>
                          </SelectItem>
                          <SelectItem value="codex">
                            <div className="flex items-center gap-2">
                              <ProviderIcon
                                icon="openai"
                                name="codex"
                                size={14}
                              />
                              <span>Codex</span>
                            </div>
                          </SelectItem>
                          <SelectItem value="claude">
                            <div className="flex items-center gap-2">
                              <ProviderIcon
                                icon="claude"
                                name="claude"
                                size={14}
                              />
                              <span>Claude Code</span>
                            </div>
                          </SelectItem>
                          <SelectItem value="opencode">
                            <div className="flex items-center gap-2">
                              <ProviderIcon
                                icon="opencode"
                                name="opencode"
                                size={14}
                              />
                              <span>OpenCode</span>
                            </div>
                          </SelectItem>
                          <SelectItem value="openclaw">
                            <div className="flex items-center gap-2">
                              <ProviderIcon
                                icon="openclaw"
                                name="openclaw"
                                size={14}
                              />
                              <span>OpenClaw</span>
                            </div>
                          </SelectItem>
                          <SelectItem value="gemini">
                            <div className="flex items-center gap-2">
                              <ProviderIcon
                                icon="gemini"
                                name="gemini"
                                size={14}
                              />
                              <span>Gemini CLI</span>
                            </div>
                          </SelectItem>
                        </SelectContent>
                      </Select>

                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="size-7"
                            onClick={() => void refetch()}
                          >
                            <RefreshCw className="size-3.5" />
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent>{t("common.refresh")}</TooltipContent>
                      </Tooltip>
                    </div>
                  </div>
                )}
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
                            selectedKey !== null &&
                            getSessionKey(session) === selectedKey;

                          return (
                            <SessionItem
                              key={getSessionKey(session)}
                              session={session}
                              isSelected={isSelected}
                              onSelect={setSelectedKey}
                            />
                          );
                        })}
                      </div>
                    )}
                  </div>
                </ScrollArea>
              </CardContent>
            </Card>

            {/* Right: Session detail */}
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
                  {/* Detail header */}
                  <CardHeader className="py-3 px-4 border-b shrink-0">
                    <div className="flex items-start justify-between gap-4">
                      {/* Left: session info */}
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2 mb-1">
                          <Tooltip>
                            <TooltipTrigger asChild>
                              <span className="shrink-0">
                                <ProviderIcon
                                  icon={getProviderIconName(
                                    selectedSession.providerId,
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

                        {/* Metadata */}
                        <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
                          <div className="flex items-center gap-1">
                            <Clock className="size-3" />
                            <span>
                              {formatTimestamp(
                                selectedSession.lastActiveAt ??
                                  selectedSession.createdAt,
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
                                      t("sessionManager.projectDirCopied"),
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
                                  {t("sessionManager.clickToCopyPath")}
                                </p>
                              </TooltipContent>
                            </Tooltip>
                          )}
                        </div>
                      </div>

                      {/* Right: action buttons */}
                      <div className="flex items-center gap-2 shrink-0">
                        {isMac() && (
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
                                    defaultValue: "Resume Session",
                                  })}
                                </span>
                              </Button>
                            </TooltipTrigger>
                            <TooltipContent>
                              {selectedSession.resumeCommand
                                ? t("sessionManager.resumeTooltip", {
                                    defaultValue:
                                      "Resume this session in terminal",
                                  })
                                : t("sessionManager.noResumeCommand", {
                                    defaultValue:
                                      "This session cannot be resumed",
                                  })}
                            </TooltipContent>
                          </Tooltip>
                        )}
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button
                              size="sm"
                              variant="destructive"
                              className="gap-1.5"
                              onClick={() => setDeleteTarget(selectedSession)}
                              disabled={
                                !selectedSession.sourcePath ||
                                deleteSessionMutation.isPending
                              }
                            >
                              <Trash2 className="size-3.5" />
                              <span className="hidden sm:inline">
                                {deleteSessionMutation.isPending
                                  ? t("sessionManager.deleting", {
                                      defaultValue: "Deleting...",
                                    })
                                  : t("sessionManager.delete", {
                                      defaultValue: "Delete session",
                                    })}
                              </span>
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent>
                            {t("sessionManager.deleteTooltip", {
                              defaultValue:
                                "Permanently delete this local session record",
                            })}
                          </TooltipContent>
                        </Tooltip>
                      </div>
                    </div>

                    {/* Resume command preview */}
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
                                  t("sessionManager.resumeCommandCopied"),
                                )
                              }
                            >
                              <Copy className="size-3.5" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent>
                            {t("sessionManager.copyCommand", {
                              defaultValue: "Copy Command",
                            })}
                          </TooltipContent>
                        </Tooltip>
                      </div>
                    )}
                  </CardHeader>

                  {/* Message toolbar */}
                  <div className="flex items-center gap-2 px-4 py-2 border-b bg-muted/20 shrink-0">
                    <div className="flex items-center gap-2 flex-1 min-w-0">
                      <MessageSquare className="size-3.5 text-muted-foreground shrink-0" />
                      <span className="text-xs font-medium text-muted-foreground shrink-0">
                        {t("sessionManager.conversationHistory", {
                          defaultValue: "Conversation",
                        })}
                      </span>
                      <Badge
                        variant="secondary"
                        className="text-[10px] px-1.5 py-0"
                      >
                        {filteredMessages.length}
                        {filteredMessages.length !== messages.length &&
                          `/${messages.length}`}
                      </Badge>

                      {/* Message search */}
                      {isMessageSearchOpen ? (
                        <div className="relative flex-1 max-w-[240px]">
                          <Search className="absolute left-2 top-1/2 -translate-y-1/2 size-3 text-muted-foreground" />
                          <Input
                            ref={messageSearchRef}
                            value={messageSearch}
                            onChange={(e) => setMessageSearch(e.target.value)}
                            placeholder={t("sessionManager.searchMessages", {
                              defaultValue: "Search messages...",
                            })}
                            className="h-6 pl-7 pr-6 text-xs"
                            autoFocus
                            onKeyDown={(e) => {
                              if (e.key === "Escape") {
                                setIsMessageSearchOpen(false);
                                setMessageSearch("");
                              }
                            }}
                          />
                          <Button
                            variant="ghost"
                            size="icon"
                            className="absolute right-0 top-1/2 -translate-y-1/2 size-5"
                            onClick={() => {
                              setIsMessageSearchOpen(false);
                              setMessageSearch("");
                            }}
                          >
                            <X className="size-2.5" />
                          </Button>
                        </div>
                      ) : (
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button
                              variant="ghost"
                              size="icon"
                              className="size-6"
                              onClick={() => {
                                setIsMessageSearchOpen(true);
                                setTimeout(
                                  () => messageSearchRef.current?.focus(),
                                  0,
                                );
                              }}
                            >
                              <Search className="size-3" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent>
                            {t("sessionManager.searchMessages", {
                              defaultValue: "Search messages",
                            })}
                          </TooltipContent>
                        </Tooltip>
                      )}
                    </div>

                    {/* Right side toolbar */}
                    <div className="flex items-center gap-1 shrink-0">
                      {/* Role filter dropdown */}
                      <DropdownMenu>
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <DropdownMenuTrigger asChild>
                              <Button
                                variant="ghost"
                                size="icon"
                                className={cn(
                                  "size-6",
                                  activeRoleFilters.length < ALL_ROLES.length &&
                                    "text-primary",
                                )}
                              >
                                <Filter className="size-3" />
                              </Button>
                            </DropdownMenuTrigger>
                          </TooltipTrigger>
                          <TooltipContent>
                            {t("sessionManager.filterByRole", {
                              defaultValue: "Filter by role",
                            })}
                          </TooltipContent>
                        </Tooltip>
                        <DropdownMenuContent align="end" className="w-44">
                          {ALL_ROLES.map((role) => (
                            <DropdownMenuCheckboxItem
                              key={role}
                              checked={activeRoleFilters.includes(role)}
                              onCheckedChange={() => toggleRoleFilter(role)}
                            >
                              <div className="flex items-center justify-between w-full">
                                <span className="capitalize">
                                  {getRoleLabel(role, t)}
                                </span>
                                {roleCounts[role] && (
                                  <span className="text-[10px] text-muted-foreground ml-2">
                                    {roleCounts[role]}
                                  </span>
                                )}
                              </div>
                            </DropdownMenuCheckboxItem>
                          ))}
                          <DropdownMenuSeparator />
                          <DropdownMenuItem
                            onClick={() => setActiveRoleFilters(ALL_ROLES)}
                          >
                            {t("sessionManager.showAll", {
                              defaultValue: "Show all",
                            })}
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>

                      {/* Collapse/expand all */}
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="size-6"
                            onClick={() => setAllCollapsed(!allCollapsed)}
                          >
                            <ChevronsUpDown className="size-3" />
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent>
                          {allCollapsed
                            ? t("sessionManager.expandAll", {
                                defaultValue: "Expand all",
                              })
                            : t("sessionManager.collapseAll", {
                                defaultValue: "Collapse all",
                              })}
                        </TooltipContent>
                      </Tooltip>

                      {/* Export / copy all dropdown */}
                      <DropdownMenu>
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <DropdownMenuTrigger asChild>
                              <Button
                                variant="ghost"
                                size="icon"
                                className="size-6"
                              >
                                <Download className="size-3" />
                              </Button>
                            </DropdownMenuTrigger>
                          </TooltipTrigger>
                          <TooltipContent>
                            {t("sessionManager.exportConversation", {
                              defaultValue: "Export",
                            })}
                          </TooltipContent>
                        </Tooltip>
                        <DropdownMenuContent align="end" className="w-52">
                          <DropdownMenuItem
                            onClick={() => void handleCopyAll()}
                          >
                            <Copy className="size-3.5 mr-2" />
                            {t("sessionManager.copyAllMessages", {
                              defaultValue: "Copy all messages",
                            })}
                          </DropdownMenuItem>
                          <DropdownMenuItem
                            onClick={() => void handleExportConversation()}
                          >
                            <Download className="size-3.5 mr-2" />
                            {t("sessionManager.exportAsMarkdown", {
                              defaultValue: "Export as Markdown",
                            })}
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </div>
                  </div>

                  {/* Messages area */}
                  <CardContent className="flex-1 overflow-hidden p-0">
                    <div className="flex h-full w-full overflow-hidden">
                      {/* Message list */}
                      <ScrollArea className="flex-1 w-0 session-messages-scroll">
                        <div className="p-4">
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
                          ) : filteredMessages.length === 0 ? (
                            <div className="flex flex-col items-center justify-center py-12 text-center">
                              <Search className="size-8 text-muted-foreground/50 mb-2" />
                              <p className="text-sm text-muted-foreground">
                                {t("sessionManager.noMatchingMessages", {
                                  defaultValue: "No matching messages",
                                })}
                              </p>
                              <Button
                                variant="link"
                                size="sm"
                                className="mt-1 text-xs"
                                onClick={() => {
                                  setMessageSearch("");
                                  setActiveRoleFilters(ALL_ROLES);
                                }}
                              >
                                {t("sessionManager.clearFilters", {
                                  defaultValue: "Clear filters",
                                })}
                              </Button>
                            </div>
                          ) : (
                            <div className="space-y-3">
                              {filteredMessages.map((message) => {
                                // Find original index for ref mapping
                                const originalIndex = messages.indexOf(message);
                                return (
                                  <SessionMessageItem
                                    key={`${message.role}-${originalIndex}`}
                                    message={message}
                                    index={originalIndex}
                                    isActive={
                                      activeMessageIndex === originalIndex
                                    }
                                    defaultCollapsed={
                                      allCollapsed ? true : undefined
                                    }
                                    setRef={(el) => {
                                      if (el)
                                        messageRefs.current.set(
                                          originalIndex,
                                          el,
                                        );
                                    }}
                                    onCopy={(content) =>
                                      handleCopy(
                                        content,
                                        t("sessionManager.messageCopied", {
                                          defaultValue: "Message copied",
                                        }),
                                      )
                                    }
                                  />
                                );
                              })}
                              <div ref={messagesEndRef} />
                            </div>
                          )}
                        </div>
                      </ScrollArea>

                      {/* TOC sidebar (large screens) */}
                      <SessionTocSidebar
                        items={userMessagesToc}
                        onItemClick={scrollToMessage}
                      />
                    </div>

                    {/* Floating TOC button (small screens) */}
                    <SessionTocDialog
                      items={userMessagesToc}
                      onItemClick={scrollToMessage}
                      open={tocDialogOpen}
                      onOpenChange={setTocDialogOpen}
                    />
                  </CardContent>
                </>
              )}
            </Card>
          </div>
        </div>
      </div>
      <ConfirmDialog
        isOpen={Boolean(deleteTarget)}
        title={t("sessionManager.deleteConfirmTitle", {
          defaultValue: "Delete session",
        })}
        message={
          deleteTarget
            ? t("sessionManager.deleteConfirmMessage", {
                defaultValue:
                  'This will permanently delete the local session "{{title}}"\nSession ID: {{sessionId}}\n\nThis action cannot be undone.',
                title: formatSessionTitle(deleteTarget),
                sessionId: deleteTarget.sessionId,
              })
            : ""
        }
        confirmText={t("sessionManager.deleteConfirmAction", {
          defaultValue: "Delete session",
        })}
        cancelText={t("common.cancel", { defaultValue: "Cancel" })}
        variant="destructive"
        onConfirm={() => void handleDeleteConfirm()}
        onCancel={() => {
          if (!deleteSessionMutation.isPending) {
            setDeleteTarget(null);
          }
        }}
      />
    </TooltipProvider>
  );
}
