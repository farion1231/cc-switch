import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Copy, Folder, RefreshCw, Terminal, Search } from "lucide-react";
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
import { Card } from "@/components/ui/card";
import { extractErrorMessage } from "@/utils/errorUtils";
import { isMac } from "@/lib/platform";
import { cn } from "@/lib/utils";

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

const getProviderLabel = (providerId: string, t: (key: string) => string) => {
  const key = `apps.${providerId}`;
  const translated = t(key);
  return translated === key ? providerId : translated;
};

const getRoleTone = (role: string) => {
  const normalized = role.toLowerCase();
  if (normalized === "assistant") return "text-blue-500";
  if (normalized === "user") return "text-emerald-500";
  if (normalized === "system") return "text-amber-500";
  if (normalized === "tool") return "text-purple-500";
  return "text-muted-foreground";
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

  const [search, setSearch] = useState("");
  const [providerFilter, setProviderFilter] = useState<ProviderFilter>("all");
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [terminalTarget, setTerminalTarget] = useState<TerminalTarget>(() => {
    if (typeof window === "undefined") return "terminal";
    const stored = window.localStorage.getItem(
      TERMINAL_TARGET_KEY,
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
      ? filteredSessions.some((session) => getSessionKey(session) === selectedKey)
      : false;
    if (!exists) {
      setSelectedKey(getSessionKey(filteredSessions[0]));
    }
  }, [filteredSessions, selectedKey]);

  const selectedSession = useMemo(() => {
    if (!selectedKey) return null;
    return (
      filteredSessions.find((session) => getSessionKey(session) === selectedKey) ||
      null
    );
  }, [filteredSessions, selectedKey]);

  const { data: messages = [], isLoading: isLoadingMessages } =
    useSessionMessagesQuery(
      selectedSession?.providerId,
      selectedSession?.sourcePath,
    );

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

    if (terminalTarget === "copy" || !isMac()) {
      await handleCopy(
        selectedSession.resumeCommand,
        t("sessionManager.resumeCommandCopied"),
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
      toast.error(
        extractErrorMessage(error) || t("sessionManager.openFailed"),
      );
    }
  };

  const scrollToDetail = () => {
    detailRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  return (
    <div className="mx-auto max-w-[56rem] px-6 flex flex-col h-[calc(100vh-8rem)]">
      <div className="flex-1 overflow-y-auto pb-12 space-y-6">
        <div className="space-y-2">
          <h2 className="text-2xl font-bold">{t("sessionManager.title")}</h2>
          <p className="text-sm text-muted-foreground">
            {t("sessionManager.subtitle")}
          </p>
        </div>

        <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
          <div className="relative flex-1">
            <Search className="absolute left-3 top-2.5 h-4 w-4 text-muted-foreground" />
            <Input
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            placeholder={t("sessionManager.searchPlaceholder")}
            className="pl-9"
          />
        </div>

        <Select
          value={providerFilter}
          onValueChange={(value) => setProviderFilter(value as ProviderFilter)}
        >
          <SelectTrigger className="w-full sm:w-[160px]">
            <SelectValue placeholder={t("sessionManager.providerFilterAll")} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{t("sessionManager.providerFilterAll")}</SelectItem>
            <SelectItem value="codex">Codex</SelectItem>
            <SelectItem value="claude">Claude Code</SelectItem>
          </SelectContent>
        </Select>

        <Button
          variant="outline"
          onClick={() => void refetch()}
          className="gap-2"
        >
          <RefreshCw className="h-4 w-4" />
          {t("common.refresh")}
        </Button>
        </div>

        <div className="grid gap-4 md:grid-cols-[minmax(220px,1fr)_minmax(0,2fr)]">
          <Card className="p-4">
            <div className="flex items-center justify-between mb-3">
              <span className="text-sm font-medium">
                {t("sessionManager.sessionList")}
              </span>
              <span className="text-xs text-muted-foreground">
                {filteredSessions.length}
              </span>
            </div>

            <div className="space-y-2 max-h-[70vh] overflow-y-auto pr-1">
              {isLoading ? (
                <div className="text-sm text-muted-foreground py-6 text-center">
                  {t("sessionManager.loadingSessions")}
                </div>
              ) : filteredSessions.length === 0 ? (
                <div className="text-sm text-muted-foreground py-6 text-center">
                  {t("sessionManager.noSessions")}
                </div>
              ) : (
                filteredSessions.map((session) => {
                  const isSelected =
                    selectedKey && getSessionKey(session) === selectedKey;
                  const title = formatSessionTitle(session);
                  const lastActive =
                    session.lastActiveAt || session.createdAt || undefined;

                  return (
                    <button
                      key={getSessionKey(session)}
                      type="button"
                      onClick={() => {
                        setSelectedKey(getSessionKey(session));
                        scrollToDetail();
                      }}
                      className={cn(
                        "w-full text-left rounded-lg border px-3 py-2 transition",
                        isSelected
                          ? "border-primary/60 bg-primary/5"
                          : "border-border/60 hover:bg-muted/60",
                      )}
                    >
                      <div className="flex items-center gap-2">
                        <Badge variant="secondary">
                          {getProviderLabel(session.providerId, t)}
                        </Badge>
                        <span className="text-sm font-medium truncate">
                          {title}
                        </span>
                      </div>
                      <div className="text-xs text-muted-foreground mt-1 line-clamp-2">
                        {session.summary || t("sessionManager.noSummary")}
                      </div>
                      <div className="flex items-center justify-between text-[11px] text-muted-foreground mt-2">
                        <span>{lastActive ? formatTimestamp(lastActive) : ""}</span>
                        <span className="truncate max-w-[120px]">
                          {session.projectDir || t("common.unknown")}
                        </span>
                      </div>
                    </button>
                  );
                })
              )}
            </div>
          </Card>

          <Card className="p-4 flex flex-col min-h-[70vh]" ref={detailRef}>
            {!selectedSession ? (
              <div className="flex-1 flex items-center justify-center text-sm text-muted-foreground">
                {t("sessionManager.selectSession")}
              </div>
            ) : (
              <div className="flex-1 flex flex-col gap-4">
                <div className="space-y-3">
                  <div className="flex flex-wrap items-start justify-between gap-3">
                    <div>
                      <div className="flex items-center gap-2 mb-1">
                        <Badge variant="secondary">
                          {getProviderLabel(selectedSession.providerId, t)}
                        </Badge>
                        <span className="text-base font-semibold">
                          {formatSessionTitle(selectedSession)}
                        </span>
                      </div>
                      <div className="text-xs text-muted-foreground">
                        {selectedSession.sessionId}
                      </div>
                    </div>

                    <div className="flex flex-col gap-2 items-end">
                      <Button
                        size="sm"
                        variant="outline"
                        className="gap-2"
                        onClick={() =>
                          selectedSession.resumeCommand &&
                          void handleCopy(
                            selectedSession.resumeCommand,
                            t("sessionManager.resumeCommandCopied"),
                          )
                        }
                        disabled={!selectedSession.resumeCommand}
                      >
                        <Copy className="h-4 w-4" />
                        {t("sessionManager.copyResumeCommand")}
                      </Button>

                      <div className="flex items-center gap-2">
                        <Select
                          value={terminalTarget}
                          onValueChange={(value) =>
                            setTerminalTarget(value as TerminalTarget)
                          }
                        >
                          <SelectTrigger className="h-8 w-[140px]">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="terminal">
                              {t("sessionManager.terminalTargetTerminal")}
                            </SelectItem>
                            <SelectItem value="kitty">
                              {t("sessionManager.terminalTargetKitty")}
                            </SelectItem>
                            <SelectItem value="copy">
                              {t("sessionManager.terminalTargetCopy")}
                            </SelectItem>
                          </SelectContent>
                        </Select>

                        <Button
                          size="sm"
                          className="gap-2"
                          onClick={() => void handleResume()}
                          disabled={!selectedSession.resumeCommand}
                        >
                          <Terminal className="h-4 w-4" />
                          {t("sessionManager.openInTerminal")}
                        </Button>
                      </div>
                    </div>
                  </div>

                  <div className="grid gap-2 text-xs text-muted-foreground">
                    <div>
                      <span className="font-medium text-foreground">
                        {t("sessionManager.lastActive")}:
                      </span>{" "}
                      {formatTimestamp(
                        selectedSession.lastActiveAt ??
                          selectedSession.createdAt,
                      )}
                    </div>
                    <div>
                      <span className="font-medium text-foreground">
                        {t("sessionManager.projectDir")}:
                      </span>{" "}
                      <span className="break-all">
                        {selectedSession.projectDir || t("common.unknown")}
                      </span>
                    </div>
                    {selectedSession.sourcePath ? (
                      <div>
                        <span className="font-medium text-foreground">
                          {t("sessionManager.sourcePath")}:
                        </span>{" "}
                        <span className="break-all">
                          {selectedSession.sourcePath}
                        </span>
                      </div>
                    ) : null}
                  </div>

                  <div className="flex flex-wrap gap-2">
                    <Button
                      size="sm"
                      variant="outline"
                      className="gap-2"
                      onClick={() =>
                        selectedSession.projectDir &&
                        void handleCopy(
                          selectedSession.projectDir,
                          t("sessionManager.projectDirCopied"),
                        )
                      }
                      disabled={!selectedSession.projectDir}
                    >
                      <Folder className="h-4 w-4" />
                      {t("sessionManager.copyProjectDir")}
                    </Button>

                  </div>

                  {selectedSession.resumeCommand ? (
                    <div className="rounded-lg bg-muted/60 px-3 py-2 text-xs font-mono break-all">
                      {selectedSession.resumeCommand}
                    </div>
                  ) : null}
                </div>

                <div className="flex-1 overflow-y-auto pr-1 space-y-3">
                  {isLoadingMessages ? (
                    <div className="text-sm text-muted-foreground py-6 text-center">
                      {t("sessionManager.loadingMessages")}
                    </div>
                  ) : messages.length === 0 ? (
                    <div className="text-sm text-muted-foreground py-6 text-center">
                      {t("sessionManager.emptySession")}
                    </div>
                  ) : (
                    messages.map((message: SessionMessage, index: number) => (
                      <div
                        key={`${message.role}-${index}`}
                        className="rounded-lg border border-border/60 bg-background/60 px-3 py-2"
                      >
                        <div className="flex items-center justify-between text-xs text-muted-foreground">
                          <span
                            className={cn(
                              "font-semibold",
                              getRoleTone(message.role),
                            )}
                          >
                            {message.role}
                          </span>
                          {message.ts ? (
                            <span>{formatTimestamp(message.ts)}</span>
                          ) : null}
                        </div>
                        <div className="mt-2 whitespace-pre-wrap text-sm">
                          {message.content}
                        </div>
                      </div>
                    ))
                  )}
                </div>
              </div>
            )}
          </Card>
        </div>
      </div>
    </div>
  );
}
