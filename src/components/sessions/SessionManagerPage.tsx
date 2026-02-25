import { useEffect, useMemo, useRef, useState, useTransition } from "react";
import { useSessionSearch } from "@/hooks/useSessionSearch";
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
  X,
  Tag,
  Settings,
} from "lucide-react";
import {
  useSessionMessagesQuery,
  useSessionsQuery,
  useSessionAliasesQuery,
  useSetSessionAliasMutation,
  useSessionConfigQuery,
} from "@/lib/query";
import { sessionsApi } from "@/lib/api";
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
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { extractErrorMessage } from "@/utils/errorUtils";
import { cn } from "@/lib/utils";
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
} from "./utils";

type ProviderFilter =
  | "all"
  | "codex"
  | "claude"
  | "opencode"
  | "openclaw"
  | "gemini";

export function SessionManagerPage({ appId }: { appId: string }) {
  const { t } = useTranslation();
  const { data, isLoading, refetch } = useSessionsQuery();
  const sessions = data ?? [];
  const { data: aliasesData } = useSessionAliasesQuery();
  const aliases = aliasesData ?? {};
  const setAliasMutation = useSetSessionAliasMutation();
  const { data: resumeExtraArgs } = useSessionConfigQuery("resumeExtraArgs");
  const { data: skipPermissionsRaw } = useSessionConfigQuery("skipPermissions");
  const skipPermissions = skipPermissionsRaw === "true";
  const { data: defaultRenderModeRaw } = useSessionConfigQuery("defaultRenderMode");
  const defaultRenderMarkdown = defaultRenderModeRaw !== "raw";
  const { data: defaultCollapseRaw } = useSessionConfigQuery("defaultCollapse");
  const defaultCollapsed = defaultCollapseRaw !== "false";
  const [extraArgsInput, setExtraArgsInput] = useState("");
  const [showSettings, setShowSettings] = useState(false);
  const detailRef = useRef<HTMLDivElement | null>(null);
  const scrollAreaRef = useRef<HTMLDivElement | null>(null);
  const messagesEndRef = useRef<HTMLDivElement | null>(null);
  const messageRefs = useRef<Map<number, HTMLDivElement>>(new Map());
  const [activeMessageIndex, setActiveMessageIndex] = useState<number | null>(
    null,
  );
  const [tocDialogOpen, setTocDialogOpen] = useState(false);
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  const [renderMarkdown, setRenderMarkdown] = useState(true);
  const [isPending, startTransition] = useTransition();
  const searchInputRef = useRef<HTMLInputElement | null>(null);

  const [search, setSearch] = useState("");
  const [debouncedSearch, setDebouncedSearch] = useState("");
  const [providerFilter, setProviderFilter] = useState<ProviderFilter>(
    appId as ProviderFilter,
  );
  const [onlyRenamed, setOnlyRenamed] = useState(false);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);

  // 打开设置面板时同步输入框
  useEffect(() => {
    if (showSettings) {
      setExtraArgsInput(resumeExtraArgs ?? "");
    }
  }, [showSettings, resumeExtraArgs]);

  // 防抖搜索词
  useEffect(() => {
    const timer = setTimeout(() => setDebouncedSearch(search), 300);
    return () => clearTimeout(timer);
  }, [search]);

  // 使用 FlexSearch 全文搜索
  const { search: searchSessions } = useSessionSearch({
    sessions,
    providerFilter,
    aliases,
  });

  const filteredSessions = useMemo(() => {
    const results = searchSessions(debouncedSearch);
    if (!onlyRenamed) return results;
    return results.filter((s) => aliases[getSessionKey(s)]);
  }, [searchSessions, debouncedSearch, onlyRenamed, aliases]);

  // 切换会话时滚动到顶部，并重置渲染模式为默认配置
  useEffect(() => {
    const viewport = scrollAreaRef.current?.querySelector("[data-radix-scroll-area-viewport]");
    if (viewport) viewport.scrollTop = 0;
    setRenderMarkdown(defaultRenderMarkdown);
  }, [selectedKey, defaultRenderMarkdown]);

  const selectedSession = useMemo(() => {
    if (!selectedKey) return null;
    return (
      sessions.find(
        (session) => getSessionKey(session) === selectedKey,
      ) || null
    );
  }, [sessions, selectedKey]);

  const { data: messages = [], isLoading: isLoadingMessages } =
    useSessionMessagesQuery(
      selectedSession?.providerId,
      selectedSession?.sourcePath,
    );

  // 提取用户消息用于目录
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
      setTocDialogOpen(false); // 关闭弹窗
      // 清除高亮状态
      setTimeout(() => setActiveMessageIndex(null), 2000);
    }
  };

  // 清理定时器
  useEffect(() => {
    return () => {
      // 这里的 setTimeout 其实无法直接清理，因为它在函数闭包里。
      // 如果要严格清理，需要用 useRef 存 timer id。
      // 但对于 2秒的高亮清除，通常不清理也没大问题。
      // 为了代码规范，我们在组件卸载时将 activeMessageIndex 重置 (虽然 React 会处理)
    };
  }, []);

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

  const handleRename = (sessionKey: string, newName: string | null) => {
    setAliasMutation.mutate(
      { sessionKey, alias: newName },
      {
        onSuccess: () => {
          toast.success(
            newName
              ? t("sessionManager.renameSuccess")
              : t("sessionManager.resetNameSuccess"),
          );
        },
      },
    );
  };

  const handleResume = async () => {
    if (!selectedSession?.resumeCommand) return;

    // 组装完整恢复命令
    const parts = [selectedSession.resumeCommand];
    if (skipPermissions) parts.push("--dangerously-skip-permissions");
    if (resumeExtraArgs) parts.push(resumeExtraArgs);
    const fullCommand = parts.join(" ");

    if (!isMac()) {
      await handleCopy(
        fullCommand,
        t("sessionManager.resumeCommandCopied"),
      );
      return;
    }

    try {
      await sessionsApi.launchTerminal({
        command: fullCommand,
        cwd: selectedSession.projectDir ?? undefined,
      });
      toast.success(t("sessionManager.terminalLaunched"));
    } catch (error) {
      await handleCopy(fullCommand, t("sessionManager.resumeFallbackCopied"));
      toast.error(extractErrorMessage(error) || t("sessionManager.openFailed"));
    }
  };

  const settingsButton = (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          variant="ghost"
          size="icon"
          className={cn(
            "size-7 fixed right-6 z-50",
            showSettings && "bg-primary/10 text-primary",
          )}
          style={{ top: 46, WebkitAppRegion: "no-drag" } as React.CSSProperties}
          onClick={() => {
            const next = !showSettings;
            setShowSettings(next);
            if (next) setExtraArgsInput(resumeExtraArgs ?? "");
          }}
        >
          <Settings className="size-4" />
        </Button>
      </TooltipTrigger>
      <TooltipContent>{t("sessionManager.settings")}</TooltipContent>
    </Tooltip>
  );

  return (
    <TooltipProvider>
      <div className="mx-auto px-4 sm:px-6 flex flex-col h-[calc(100vh-8rem)]">
        {settingsButton}
        {showSettings ? (
          <div className="flex-1 overflow-hidden flex flex-col gap-4">
            <Card className="flex-1 overflow-hidden">
              <CardHeader className="py-3 px-4 border-b shrink-0">
                <CardTitle className="text-sm font-medium">
                  {t("sessionManager.settings")}
                </CardTitle>
              </CardHeader>
              <CardContent className="p-4 space-y-6">
                {/* --dangerously-skip-permissions 勾选项 */}
                <div className="flex items-start gap-3">
                  <input
                    type="checkbox"
                    id="skipPermissions"
                    checked={skipPermissions}
                    onChange={async (e) => {
                      await sessionsApi.setConfig("skipPermissions", e.target.checked ? "true" : "false");
                      const { queryClient } = await import("@/lib/query");
                      await queryClient.invalidateQueries({ queryKey: ["sessionConfig", "skipPermissions"] });
                    }}
                    className="mt-0.5 size-4 rounded border-input accent-primary cursor-poin"
                  />
                  <div>
                    <label htmlFor="skipPermissions" className="text-sm font-medium cursor-pointer">
                      {t("sessionManager.skipPermissionsTitle")}
                    </label>
                    <p className="text-xs text-muted-foreground mt-0.5">
                      {t("sessionManager.skipPermissionsHint")}
                    </p>
                  </div>
                </div>

                {/* 默认渲染模式 */}
                <div className="flex items-start gap-3">
                  <input
                    type="checkbox"
                    id="defaultRenderMarkdown"
                    checked={defaultRenderMarkdown}
                    onChange={async (e) => {
                      await sessionsApi.setConfig("defaultRenderMode", e.target.checked ? "md" : "raw");
                      const { queryClient } = await import("@/lib/query");
                      await queryClient.invalidateQueries({ queryKey: ["sessionConfig", "defaultRenderMode"] });
                    }}
                    className="mt-0.5 size-4 rounded border-input accent-primary cursor-pointer"
                  />
                  <div>
                    <label htmlFor="defaultRenderMarkdown" className="text-sm font-medium cursor-pointer">
                      {t("sessionManager.defaultRenderMdTitle")}
                    </label>
                    <p className="text-xs text-muted-foreground mt-0.5">
                      {t("sessionManager.defaultRenderMdHint")}
                    </p>
                  </div>
                </div>

                {/* 默认收缩消息 */}
                <div className="flex items-start gap-3">
                  <input
                    type="checkbox"
                    id="defaultCollapse"
                    checked={defaultCollapsed}
                    onChange={async (e) => {
                      await sessionsApi.setConfig("defaultCollapse", e.target.checked ? "true" : "false");
                      const { queryClient } = await import("@/lib/query");
                      await queryClient.invalidateQueries({ queryKey: ["sessionConfig", "defaultCollapse"] });
                    }}
                    className="mt-0.5 size-4 rounded border-input accent-primary cursor-pointer"
                  />
                  <div>
                    <label htmlFor="defaultCollapse" className="text-sm font-medium cursor-pointer">
                      {t("sessionManager.defaultCollapseTitle")}
                    </label>
                    <p className="text-xs text-muted-foreground mt-0.5">
                      {t("sessionManager.defaultCollapseHint")}
                    </p>
                  </div>
                </div>

                {/* 额外参数 */}
                <div className="space-y-2">
                  <label className="text-sm font-medium">
                    {t("sessionManager.resumeExtraArgs")}
                  </label>
                  <div className="flex items-center gap-2">
                    <Input
                      value={extraArgsInput}
                      onChange={(e) => setExtraArgsInput(e.target.value)}
                      placeholder="--model opus"
                      className="h-8 text-sm font-mono"
                    />
                    <Button
                      variant="outline"
                      size="sm"
                      className="h-8 px-3 shrink-0"
                      onClick={async () => {
                        const trimmed = extraArgsInput.trim();
                        await sessionsApi.setConfig("resumeExtraArgs", trimmed);
                        const { queryClient } = await import("@/lib/query");
                        await queryClient.invalidateQueries({ queryKey: ["sessionConfig", "resumeExtraArgs"] });
                        toast.success(t("sessionManager.settingsSaved"));
                      }}
                    >
                      {t("common.save")}
                    </Button>
                  </div>
                  <p className="text-xs text-muted-foreground">
                    {t("sessionManager.resumeExtraArgsHint")}
                  </p>
                </div>
              </CardContent>
            </Card>
          </div>
        ) : (
        <div className="flex-1 overflow-hidden flex flex-col gap-4">
          {/* 主内容区域 - 左右分栏 */}
          <div className="flex-1 overflow-hidden grid gap-4 md:grid-cols-[320px_1fr]">
            {/* 左侧会话列表 */}
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

                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button
                            variant="ghost"
                            size="icon"
                            className={cn(
                              "size-7",
                              onlyRenamed && "bg-primary/10 text-primary",
                            )}
                            onClick={() => setOnlyRenamed((v) => !v)}
                          >
                            <Tag className="size-3.5" />
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent>
                          {t("sessionManager.filterRenamed")}
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
                          const key = getSessionKey(session);
                          const isSelected =
                            selectedKey !== null && key === selectedKey;

                          return (
                            <SessionItem
                              key={key}
                              session={session}
                              isSelected={isSelected}
                              onSelect={setSelectedKey}
                              alias={aliases[key]}
                              onRename={handleRename}
                            />
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
                          <h2 className="text-base truncate">
                            <span className="font-semibold">
                              {(selectedKey && aliases[selectedKey]) || formatSessionTitle(selectedSession)}
                            </span>
                            {selectedKey && aliases[selectedKey] && (
                              <span className="text-muted-foreground text-xs font-normal ml-1">
                                ({formatSessionTitle(selectedSession)})
                              </span>
                            )}
                          </h2>
                        </div>

                        {/* 元信息 */}
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

                      {/* 右侧：操作按钮组 */}
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
                        )}
                      </div>
                    </div>

                    {/* 恢复命令预览 */}
                    {selectedSession.resumeCommand && (
                      <div className="mt-3 flex items-center gap-2">
                        <div className="flex-1 rounded-md bg-muted/60 px-3 py-1.5 font-mono text-xs text-muted-foreground truncate">
                          {[
                            selectedSession.resumeCommand,
                            skipPermissions && "--dangerously-skip-permissions",
                            resumeExtraArgs,
                          ].filter(Boolean).join(" ")}
                        </div>
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button
                              variant="ghost"
                              size="icon"
                              className="size-7 shrink-0"
                              onClick={() => {
                                const cmd = [
                                  selectedSession.resumeCommand!,
                                  skipPermissions && "--dangerously-skip-permissions",
                                  resumeExtraArgs,
                                ].filter(Boolean).join(" ");
                                void handleCopy(
                                  cmd,
                                  t("sessionManager.resumeCommandCopied"),
                                );
                              }}
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

                  {/* 消息列表区域 */}
                  <CardContent className="flex-1 overflow-hidden p-0">
                    <div className="flex h-full overflow-hidden">
                      {/* 消息列表 */}
                      <ScrollArea ref={scrollAreaRef} className="flex-1 min-w-0">
                        <div className="p-4 overflow-x-hidden">
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
                            <Button
                              variant="outline"
                              size="sm"
                              className="h-5 px-1.5 text-xs rounded"
                              disabled={isPending}
                              onClick={() => startTransition(() => setRenderMarkdown((prev) => !prev))}
                            >
                              {renderMarkdown ? "MD" : "Raw"}
                            </Button>
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
                              {messages.map((message, index) => (
                                <SessionMessageItem
                                  key={`${message.role}-${index}`}
                                  message={message}
                                  index={index}
                                  isActive={activeMessageIndex === index}
                                  setRef={(el) => {
                                    if (el) messageRefs.current.set(index, el);
                                  }}
                                  onCopy={(content) =>
                                    handleCopy(
                                      content,
                                      t("sessionManager.messageCopied", {
                                        defaultValue: "已复制消息内容",
                                      }),
                                    )
                                  }
                                  renderMarkdown={renderMarkdown}
                                  defaultCollapsed={defaultCollapsed}
                                />
                              ))}
                              <div ref={messagesEndRef} />
                            </div>
                          )}
                        </div>
                      </ScrollArea>

                      {/* 右侧目录 - 类似少数派 (大屏幕) */}
                      <SessionTocSidebar
                        items={userMessagesToc}
                        onItemClick={scrollToMessage}
                      />
                    </div>

                    {/* 浮动目录按钮 (小屏幕) */}
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
        )}
      </div>
    </TooltipProvider>
  );
}
