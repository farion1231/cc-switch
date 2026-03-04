import { useEffect, useMemo, useRef, useState } from "react";
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
} from "lucide-react";
import {
  useSessionMessagesQuery,
  useSessionsQuery,
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
  const detailRef = useRef<HTMLDivElement | null>(null);
  const messagesEndRef = useRef<HTMLDivElement | null>(null);
  const messageRefs = useRef<Map<number, HTMLDivElement>>(new Map());
  const [activeMessageIndex, setActiveMessageIndex] = useState<number | null>(
    null,
  );
  const [tocDialogOpen, setTocDialogOpen] = useState(false);
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  const searchInputRef = useRef<HTMLInputElement | null>(null);

  const [search, setSearch] = useState("");
  const [providerFilter, setProviderFilter] = useState<ProviderFilter>(
    appId as ProviderFilter,
  );
  const [selectedKey, setSelectedKey] = useState<string | null>(null);

  // 浣跨敤 FlexSearch 鍏ㄦ枃鎼滅储
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

  // 鎻愬彇鐢ㄦ埛娑堟伅鐢ㄤ簬鐩綍
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
      setTocDialogOpen(false); // 鍏抽棴寮圭獥
      // 娓呴櫎楂樹寒鐘舵€?      setTimeout(() => setActiveMessageIndex(null), 2000);
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

  return (
    <TooltipProvider>
      <div className="mx-auto px-4 sm:px-6 flex flex-col h-[calc(100vh-8rem)]">
        <div className="flex-1 overflow-hidden flex flex-col gap-4">
          {/* 涓诲唴瀹瑰尯鍩?- 宸﹀彸鍒嗘爮 */}
          <div className="flex-1 overflow-hidden grid gap-4 md:grid-cols-[320px_1fr]">
            {/* 宸︿晶浼氳瘽鍒楄〃 */}
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

            {/* 鍙充晶浼氳瘽璇︽儏 */}
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
                  {/* 璇︽儏澶撮儴 */}
                  <CardHeader className="py-3 px-4 border-b shrink-0">
                    <div className="flex items-start justify-between gap-4">
                      {/* 宸︿晶锛氫細璇濅俊鎭?*/}
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

                        {/* 鍏冧俊鎭?*/}
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

                      {/* 鍙充晶锛氭搷浣滄寜閽粍 */}
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
                                    defaultValue: "鎭㈠浼氳瘽",
                                  })}
                                </span>
                              </Button>
                            </TooltipTrigger>
                            <TooltipContent>
                              {selectedSession.resumeCommand
                                ? t("sessionManager.resumeTooltip", {
                                    defaultValue: "Resume this session in terminal",
                                  })
                                : t("sessionManager.noResumeCommand", {
                                    defaultValue: "This session cannot be resumed",
                                  })}
                            </TooltipContent>
                          </Tooltip>
                        )}
                      </div>
                    </div>

                    {/* 鎭㈠鍛戒护棰勮 */}
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
                              defaultValue: "澶嶅埗鍛戒护",
                            })}
                          </TooltipContent>
                        </Tooltip>
                      </div>
                    )}
                  </CardHeader>

                  {/* 娑堟伅鍒楄〃鍖哄煙 */}
                  <CardContent className="flex-1 overflow-hidden p-0">
                    <div className="flex h-full">
                      {/* 娑堟伅鍒楄〃 */}
                      <ScrollArea className="flex-1">
                        <div className="p-4">

                          <div className="flex items-center gap-2 mb-3">
                            <MessageSquare className="size-4 text-muted-foreground" />
                            <span className="text-sm font-medium">
                              {t("sessionManager.conversationHistory", {
                                defaultValue: "瀵硅瘽璁板綍",
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
                              {messages.map((message, index) => (
                                <SessionMessageItem
                                  key={`message-${index}-${message.ts ?? "na"}`}
                                  message={message}
                                  index={index}
                                  isActive={activeMessageIndex === index}
                                  setRef={(el) => {
                                    if (el) {
                                      messageRefs.current.set(index, el);
                                    }
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
                              ))}
                              <div ref={messagesEndRef} />
                            </div>
                          )}
                        </div>
                      </ScrollArea>

                      {/* 鍙充晶鐩綍 - 绫讳技灏戞暟娲?(澶у睆骞? */}
                      <SessionTocSidebar
                        items={userMessagesToc}
                        onItemClick={scrollToMessage}
                      />
                    </div>

                    {/* 娴姩鐩綍鎸夐挳 (灏忓睆骞? */}
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
    </TooltipProvider>
  );
}

