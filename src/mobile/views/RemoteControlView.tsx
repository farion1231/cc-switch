import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  ArrowLeft,
  ChevronDown,
  ChevronRight,
  Folder,
  Loader2,
  Lock,
  Plus,
  Search,
  Send,
  Shield,
  SlidersHorizontal,
  Sparkles,
  Square,
  Terminal,
  Wrench,
  X,
} from "lucide-react";
import {
  ApiError,
  mobileApi,
  type RemoteAgentInfo,
  type RemoteAgentsResponse,
  type RemoteRunSnapshot,
  type RemoteSessionBlock,
  type RemoteSessionMessage,
  type RemoteStreamEvent,
  type SafeRemoteSession,
} from "../api";
import { formatAppName, formatRelativeTime } from "../format";
import { tr, type MobileLang } from "../i18n";

interface RemoteControlViewProps {
  lang: MobileLang;
  refreshSignal: number;
  onAuthExpired: () => void;
}

const STREAM_EVENT_KINDS = [
  "started",
  "init",
  "thought",
  "text",
  "tool_call",
  "tool_result",
  "stderr",
  "completed",
  "error",
  "cancelled",
] as const;

function folderBasename(path?: string): string {
  if (!path) return "";
  const cleaned = path.replace(/[\\/]+$/, "");
  const parts = cleaned.split(/[\\/]/);
  return parts[parts.length - 1] || cleaned;
}

function formatSessionTimestamp(ts?: number): string {
  if (!ts) return "";
  // Session timestamps from backend may be ms or sec.
  const unixSec = ts > 10_000_000_000 ? Math.floor(ts / 1000) : ts;
  return formatRelativeTime(unixSec);
}

interface GroupedStreamBlock {
  key: string;
  kind: "thought" | "text" | "tool_call" | "tool_result" | "stderr" | "error";
  text: string;
  toolName?: string;
  isError?: boolean;
}

function groupStreamEvents(events: RemoteStreamEvent[]): GroupedStreamBlock[] {
  const out: GroupedStreamBlock[] = [];
  for (const ev of events) {
    if (
      ev.kind === "started" ||
      ev.kind === "init" ||
      ev.kind === "completed" ||
      ev.kind === "cancelled"
    ) {
      continue;
    }
    const content = ev.text ?? "";
    if (!content && ev.kind !== "tool_call" && ev.kind !== "tool_result") {
      continue;
    }
    const prev = out[out.length - 1];
    if (
      (ev.kind === "text" || ev.kind === "thought") &&
      prev &&
      prev.kind === ev.kind
    ) {
      prev.text += content;
      continue;
    }
    if (ev.kind === "stderr" && prev && prev.kind === "stderr") {
      prev.text += `\n${content}`;
      continue;
    }
    out.push({
      key: `${ev.seq}-${ev.kind}`,
      kind: ev.kind,
      text: content,
      toolName: ev.toolName,
      isError: ev.isError,
    });
  }
  return out;
}

export function RemoteControlView({
  lang,
  refreshSignal,
  onAuthExpired,
}: RemoteControlViewProps) {
  // Catalog & list state
  const [agentsData, setAgentsData] = useState<RemoteAgentsResponse | null>(
    null,
  );
  const [sessions, setSessions] = useState<SafeRemoteSession[]>([]);
  const [runs, setRuns] = useState<RemoteRunSnapshot[]>([]);
  const [selectedFilter, setSelectedFilter] = useState<string>("all");
  const [searchQuery, setSearchQuery] = useState("");
  const [debouncedSearch, setDebouncedSearch] = useState("");
  const [listLoading, setListLoading] = useState(true);
  const [listError, setListError] = useState<string | null>(null);

  // Active conversation / live run state
  const [activeSession, setActiveSession] = useState<SafeRemoteSession | null>(
    null,
  );
  const [messages, setMessages] = useState<RemoteSessionMessage[]>([]);
  const [messagesLoading, setMessagesLoading] = useState(false);
  const [activeRun, setActiveRun] = useState<RemoteRunSnapshot | null>(null);
  const [streamEvents, setStreamEvents] = useState<RemoteStreamEvent[]>([]);
  const [consoleError, setConsoleError] = useState<string | null>(null);

  // Composer & New Session Sheet state
  const [promptInput, setPromptInput] = useState("");
  const [optionsOpen, setOptionsOpen] = useState(false);
  const [modelInput, setModelInput] = useState("");
  const [modeInput, setModeInput] = useState<string>("");
  const [effortInput, setEffortInput] = useState<string>("");
  const [sandboxInput, setSandboxInput] = useState(false);
  const [sending, setSending] = useState(false);
  const [stopping, setStopping] = useState(false);

  const [newSheetOpen, setNewSheetOpen] = useState(false);
  const [newAgentId, setNewAgentId] = useState<string>("");
  const [newCwd, setNewCwd] = useState("");
  const [newPrompt, setNewPrompt] = useState("");
  const [newModel, setNewModel] = useState("");
  const [newMode, setNewMode] = useState("");
  const [newEffort, setNewEffort] = useState("");
  const [newSandbox, setNewSandbox] = useState(false);

  const lastSeqRef = useRef(0);
  const activeRunIdRef = useRef<string | null>(null);
  const streamEndRef = useRef<HTMLDivElement | null>(null);

  const handleErr = useCallback(
    (err: unknown, setter: (msg: string) => void) => {
      if (err instanceof ApiError && err.status === 401) {
        onAuthExpired();
        return;
      }
      setter(err instanceof Error ? err.message : tr(lang, "networkError"));
    },
    [lang, onAuthExpired],
  );

  const agentMap = useMemo(() => {
    const map = new Map<string, RemoteAgentInfo>();
    for (const a of agentsData?.agents ?? []) {
      map.set(a.id, a);
    }
    return map;
  }, [agentsData]);

  const enabledInstalledAgents = useMemo(
    () => (agentsData?.agents ?? []).filter((a) => a.enabled && a.installed),
    [agentsData],
  );

  const filterAgents = useMemo(() => {
    const all = agentsData?.agents ?? [];
    // Show enabled or installed agents first; fall back to all if none detected yet.
    const relevant = all.filter(
      (a) => a.enabled || a.installed || a.activeRunCount > 0,
    );
    return relevant.length > 0 ? relevant : all;
  }, [agentsData]);

  // Debounce search input so typing only re-queries sessions, not PATH/agent discovery.
  useEffect(() => {
    const timer = window.setTimeout(() => {
      setDebouncedSearch(searchQuery);
    }, 250);
    return () => window.clearTimeout(timer);
  }, [searchQuery]);

  const loadAgentsAndRuns = useCallback(async () => {
    try {
      const [agentsRes, runsRes] = await Promise.all([
        mobileApi.getRemoteAgents(),
        mobileApi.getRemoteRuns(
          selectedFilter === "all" ? undefined : selectedFilter,
        ),
      ]);
      setAgentsData(agentsRes);
      setRuns(runsRes.runs);
    } catch (err) {
      handleErr(err, setListError);
    }
  }, [selectedFilter, handleErr]);

  const loadSessionsOnly = useCallback(async () => {
    setListLoading(true);
    setListError(null);
    try {
      const sessionsRes = await mobileApi.getRemoteSessions({
        app: selectedFilter === "all" ? undefined : selectedFilter,
        limit: 50,
        q: debouncedSearch,
      });
      setSessions(sessionsRes.sessions);
    } catch (err) {
      handleErr(err, setListError);
    } finally {
      setListLoading(false);
    }
  }, [selectedFilter, debouncedSearch, handleErr]);

  const loadList = useCallback(async () => {
    await Promise.all([loadAgentsAndRuns(), loadSessionsOnly()]);
  }, [loadAgentsAndRuns, loadSessionsOnly]);

  useEffect(() => {
    void loadAgentsAndRuns();
  }, [loadAgentsAndRuns, refreshSignal]);

  useEffect(() => {
    void loadSessionsOnly();
  }, [loadSessionsOnly, refreshSignal]);

  const loadTranscript = useCallback(
    async (app: string, sessionId: string): Promise<boolean> => {
      setMessagesLoading(true);
      setConsoleError(null);
      try {
        const res = await mobileApi.getRemoteSessionMessages(app, sessionId);
        setActiveSession(res.session);
        setMessages(res.messages);
        return true;
      } catch (err) {
        handleErr(err, setConsoleError);
        return false;
      } finally {
        setMessagesLoading(false);
      }
    },
    [handleErr],
  );

  const openSession = useCallback(
    async (session: SafeRemoteSession) => {
      setActiveSession(session);
      setMessages([]);
      setStreamEvents([]);
      setConsoleError(null);
      lastSeqRef.current = 0;

      // Reset agent-specific composer options to safe defaults for this agent.
      const info = agentMap.get(session.app);
      setModelInput("");
      setModeInput(info?.modes[0] ?? "");
      setEffortInput("");
      setSandboxInput(false);

      if (session.activeRunId) {
        try {
          const detail = await mobileApi.getRemoteRun(session.activeRunId, 0);
          setActiveRun(detail.run);
          setStreamEvents(detail.events);
          lastSeqRef.current =
            detail.events[detail.events.length - 1]?.seq ?? detail.run.lastSeq;
        } catch {
          setActiveRun(null);
        }
      } else {
        setActiveRun(null);
      }
      await loadTranscript(session.app, session.sessionId);
    },
    [agentMap, loadTranscript],
  );

  const openRunDirectly = useCallback(
    async (run: RemoteRunSnapshot) => {
      setConsoleError(null);
      setStreamEvents([]);
      lastSeqRef.current = 0;
      const syntheticSession: SafeRemoteSession = {
        app: run.app,
        sessionId: run.conversationId ?? run.sessionId ?? "",
        title: run.prompt,
        projectDir: run.cwd,
        createdAt: run.createdAt,
        lastActiveAt: run.updatedAt,
        resumable: true,
        activeRunId: run.status === "running" ? run.runId : undefined,
      };
      setActiveSession(syntheticSession);
      setActiveRun(run);
      const info = agentMap.get(run.app);
      setModelInput(run.model ?? "");
      setModeInput(run.mode ?? info?.modes[0] ?? "");
      setEffortInput(run.effort ?? "");
      setSandboxInput(run.sandbox);

      try {
        const detail = await mobileApi.getRemoteRun(run.runId, 0);
        setActiveRun(detail.run);
        setStreamEvents(detail.events);
        lastSeqRef.current =
          detail.events[detail.events.length - 1]?.seq ?? detail.run.lastSeq;
      } catch (err) {
        handleErr(err, setConsoleError);
      }
      if (syntheticSession.sessionId) {
        await loadTranscript(run.app, syntheticSession.sessionId);
      } else {
        setMessages([]);
      }
    },
    [agentMap, handleErr, loadTranscript],
  );

  // Merge incoming stream events cleanly by sequence number.
  const appendEvents = useCallback((incoming: RemoteStreamEvent[]) => {
    if (incoming.length === 0) return;
    setStreamEvents((prev) => {
      const existingSeqs = new Set(prev.map((e) => e.seq));
      const added = incoming.filter((e) => !existingSeqs.has(e.seq));
      if (added.length === 0) return prev;
      const merged = [...prev, ...added].sort((a, b) => a.seq - b.seq);
      lastSeqRef.current = merged[merged.length - 1]?.seq ?? lastSeqRef.current;
      return merged;
    });
  }, []);

  const syncRunViaPolling = useCallback(
    async (runId: string) => {
      try {
        const detail = await mobileApi.getRemoteRun(runId, lastSeqRef.current);
        appendEvents(detail.events);
        setActiveRun(detail.run);
        const capturedId = detail.run.conversationId ?? detail.run.sessionId;
        if (capturedId) {
          setActiveSession((prev) =>
            prev && !prev.sessionId ? { ...prev, sessionId: capturedId } : prev,
          );
        }
        if (detail.run.status !== "running" && capturedId) {
          const loaded = await loadTranscript(detail.run.app, capturedId);
          // Once persisted transcript refreshes for a completed run, clear live
          // stream state so the completed turn is never rendered twice.
          if (loaded && detail.run.status === "completed") {
            setActiveRun((prev) => (prev?.runId === runId ? null : prev));
            setStreamEvents([]);
          }
          void loadList();
        }
      } catch (err) {
        handleErr(err, setConsoleError);
      }
    },
    [appendEvents, handleErr, loadTranscript, loadList],
  );

  // Live SSE subscription with automatic `?after=<seq>` reconnect & polling fallback.
  useEffect(() => {
    const runId = activeRun?.runId;
    const isRunning = activeRun?.status === "running";
    activeRunIdRef.current = runId ?? null;
    if (!runId || !isRunning) return;

    let closed = false;
    let es: EventSource | null = null;
    let pollTimer: number | null = null;

    const startPollingFallback = () => {
      if (closed || pollTimer !== null) return;
      pollTimer = window.setInterval(() => {
        if (!document.hidden && activeRunIdRef.current === runId) {
          void syncRunViaPolling(runId);
        }
      }, 1500);
    };

    const stopPollingFallback = () => {
      if (pollTimer !== null) {
        window.clearInterval(pollTimer);
        pollTimer = null;
      }
    };

    const connectSse = () => {
      if (closed) return;
      if (typeof EventSource === "undefined") {
        startPollingFallback();
        return;
      }
      es?.close();
      const url = mobileApi.remoteRunStreamUrl(runId, lastSeqRef.current);
      const source = new EventSource(url, { withCredentials: true });
      es = source;

      const onEvent = (raw: Event) => {
        if (closed) return;
        const msg = raw as MessageEvent;
        try {
          const parsed = JSON.parse(msg.data) as RemoteStreamEvent;
          appendEvents([parsed]);
          if (parsed.sessionId) {
            setActiveRun((prev) =>
              prev && prev.runId === runId
                ? { ...prev, conversationId: parsed.sessionId }
                : prev,
            );
            setActiveSession((prev) =>
              prev && !prev.sessionId
                ? { ...prev, sessionId: parsed.sessionId! }
                : prev,
            );
          }
          if (
            parsed.kind === "completed" ||
            parsed.kind === "error" ||
            parsed.kind === "cancelled"
          ) {
            source.close();
            stopPollingFallback();
            void syncRunViaPolling(runId);
          }
        } catch {
          // ignore malformed SSE payload
        }
      };

      for (const kind of STREAM_EVENT_KINDS) {
        source.addEventListener(kind, onEvent);
      }

      source.onerror = () => {
        if (closed) return;
        source.close();
        void syncRunViaPolling(runId);
        startPollingFallback();
      };
    };

    connectSse();

    const onVisibility = () => {
      if (!document.hidden && !closed) {
        void syncRunViaPolling(runId);
        if (!es || es.readyState === EventSource.CLOSED) {
          connectSse();
        }
      }
    };
    document.addEventListener("visibilitychange", onVisibility);

    return () => {
      closed = true;
      es?.close();
      stopPollingFallback();
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [activeRun?.runId, activeRun?.status, appendEvents, syncRunViaPolling]);

  useEffect(() => {
    if (streamEvents.length > 0 && streamEndRef.current?.scrollIntoView) {
      streamEndRef.current.scrollIntoView({ behavior: "smooth", block: "end" });
    }
  }, [streamEvents.length]);

  const handleOpenNewSheet = () => {
    const preferredAgent =
      (selectedFilter !== "all" &&
        enabledInstalledAgents.find((a) => a.id === selectedFilter)?.id) ||
      enabledInstalledAgents[0]?.id ||
      agentsData?.agents[0]?.id ||
      "claude";
    const info = agentMap.get(preferredAgent);
    setNewAgentId(preferredAgent);
    setNewCwd(agentsData?.recentWorkspaces[0] ?? agentsData?.defaultCwd ?? "");
    setNewPrompt("");
    setNewModel("");
    setNewMode(info?.modes[0] ?? "");
    setNewEffort("");
    setNewSandbox(false);
    setNewSheetOpen(true);
  };

  const handleSelectNewAgent = (id: string) => {
    setNewAgentId(id);
    const info = agentMap.get(id);
    setNewMode(info?.modes[0] ?? "");
    setNewEffort("");
    setNewSandbox(false);
  };

  const handleStartNewSession = async () => {
    const trimmed = newPrompt.trim();
    if (!trimmed || !newAgentId) return;
    setSending(true);
    setConsoleError(null);
    try {
      const info = agentMap.get(newAgentId);
      const run = await mobileApi.startRemoteRun({
        app: newAgentId,
        prompt: trimmed,
        cwd: newCwd.trim() || undefined,
        model:
          info?.supportsModel && newModel.trim() ? newModel.trim() : undefined,
        mode: info?.supportsMode && newMode ? newMode : undefined,
        effort: info?.supportsEffort && newEffort ? newEffort : undefined,
        sandbox: info?.supportsSandbox ? newSandbox : false,
      });
      setNewSheetOpen(false);
      setActiveSession({
        app: run.app,
        sessionId: run.conversationId ?? "",
        title: trimmed,
        projectDir: run.cwd,
        createdAt: run.createdAt,
        lastActiveAt: run.updatedAt,
        resumable: true,
        activeRunId: run.runId,
      });
      setMessages([]);
      setStreamEvents([]);
      lastSeqRef.current = 0;
      setActiveRun(run);
      setModelInput(run.model ?? "");
      setModeInput(run.mode ?? info?.modes[0] ?? "");
      setEffortInput(run.effort ?? "");
      setSandboxInput(run.sandbox);
    } catch (err) {
      handleErr(err, setConsoleError);
    } finally {
      setSending(false);
    }
  };

  const handleSendFollowUp = async () => {
    const trimmed = promptInput.trim();
    if (!trimmed || !activeSession) return;
    setSending(true);
    setConsoleError(null);
    try {
      const info = agentMap.get(activeSession.app);
      const run = await mobileApi.startRemoteRun({
        app: activeSession.app,
        prompt: trimmed,
        sessionId: activeSession.sessionId || undefined,
        cwd: activeSession.projectDir || undefined,
        model:
          info?.supportsModel && modelInput.trim()
            ? modelInput.trim()
            : undefined,
        mode: info?.supportsMode && modeInput ? modeInput : undefined,
        effort: info?.supportsEffort && effortInput ? effortInput : undefined,
        sandbox: info?.supportsSandbox ? sandboxInput : false,
      });
      setPromptInput("");
      setStreamEvents([]);
      lastSeqRef.current = 0;
      setActiveRun(run);
    } catch (err) {
      handleErr(err, setConsoleError);
    } finally {
      setSending(false);
    }
  };

  const handleCancelRun = async () => {
    if (!activeRun || activeRun.status !== "running") return;
    setStopping(true);
    try {
      const snap = await mobileApi.cancelRemoteRun(activeRun.runId);
      setActiveRun(snap);
      void syncRunViaPolling(activeRun.runId);
    } catch (err) {
      handleErr(err, setConsoleError);
    } finally {
      setStopping(false);
    }
  };

  // ============================================================
  // Sub-view B: Active Conversation & Live Stream Console
  // ============================================================
  if (activeSession) {
    const agentInfo = agentMap.get(activeSession.app);
    const isRunning = activeRun?.status === "running";
    const canExecute =
      Boolean(agentInfo?.enabled) &&
      Boolean(agentInfo?.installed) &&
      (activeSession.sessionId === "" || activeSession.resumable);
    const groupedStream = groupStreamEvents(streamEvents);
    const hasAnyControls = Boolean(
      agentInfo &&
        (agentInfo.supportsModel ||
          agentInfo.supportsMode ||
          agentInfo.supportsEffort ||
          agentInfo.supportsSandbox),
    );

    return (
      <div className="flex flex-col space-y-3 pb-40">
        {/* Session Header Bar */}
        <div className="flex items-center justify-between gap-2 rounded-2xl border border-zinc-200/80 bg-white px-3 py-2.5 dark:border-zinc-800 dark:bg-[#232328]">
          <button
            type="button"
            onClick={() => {
              setActiveSession(null);
              setActiveRun(null);
              setStreamEvents([]);
              void loadList();
            }}
            className="flex min-h-[36px] shrink-0 items-center gap-1 rounded-xl bg-zinc-100 px-2.5 text-xs font-semibold text-zinc-700 active:scale-95 dark:bg-zinc-800 dark:text-zinc-200"
          >
            <ArrowLeft className="h-3.5 w-3.5" />
            {tr(lang, "remoteBack")}
          </button>

          <div className="min-w-0 flex-1 px-1">
            <div className="flex items-center gap-1.5">
              <span className="shrink-0 rounded-md bg-orange-500/15 px-1.5 py-0.5 text-[10px] font-semibold text-[#f97316]">
                {formatAppName(activeSession.app)}
              </span>
              <span className="truncate text-xs font-semibold text-zinc-900 dark:text-zinc-100">
                {activeSession.title ||
                  activeSession.summary ||
                  tr(lang, "remoteUntitled")}
              </span>
            </div>
            {activeSession.projectDir && (
              <div className="mt-0.5 flex items-center gap-1 truncate text-[11px] text-zinc-500 dark:text-zinc-400">
                <Folder className="h-3 w-3 shrink-0" />
                <span className="truncate">
                  {folderBasename(activeSession.projectDir)}
                </span>
              </div>
            )}
          </div>

          {isRunning && (
            <button
              type="button"
              disabled={stopping}
              onClick={() => void handleCancelRun()}
              className="flex min-h-[36px] shrink-0 items-center gap-1 rounded-xl bg-red-600 px-3 text-xs font-semibold text-white transition active:scale-95 disabled:opacity-60"
            >
              {stopping ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Square className="h-3 w-3 fill-current" />
              )}
              {stopping ? tr(lang, "remoteStopping") : tr(lang, "remoteStop")}
            </button>
          )}
        </div>

        {/* Read-only / Unavailable Banner */}
        {agentInfo && !agentInfo.installed && (
          <div className="flex items-start gap-2 rounded-2xl border border-amber-200/80 bg-amber-50/90 px-3.5 py-2.5 text-xs text-amber-800 dark:border-amber-900/50 dark:bg-amber-950/30 dark:text-amber-300">
            <Lock className="mt-0.5 h-4 w-4 shrink-0" />
            <span>{tr(lang, "remoteNotInstalledAgent")}</span>
          </div>
        )}
        {agentInfo && agentInfo.installed && !agentInfo.enabled && (
          <div className="flex items-start gap-2 rounded-2xl border border-amber-200/80 bg-amber-50/90 px-3.5 py-2.5 text-xs text-amber-800 dark:border-amber-900/50 dark:bg-amber-950/30 dark:text-amber-300">
            <Lock className="mt-0.5 h-4 w-4 shrink-0" />
            <span>{tr(lang, "remoteReadOnlyAgent")}</span>
          </div>
        )}
        {activeSession.sessionId && !activeSession.resumable && (
          <div className="flex items-start gap-2 rounded-2xl border border-zinc-200 bg-zinc-100/80 px-3.5 py-2.5 text-xs text-zinc-600 dark:border-zinc-800 dark:bg-zinc-900 dark:text-zinc-400">
            <Lock className="mt-0.5 h-4 w-4 shrink-0" />
            <span>{tr(lang, "remoteNotResumable")}</span>
          </div>
        )}

        {consoleError && (
          <div className="flex items-center gap-2 rounded-2xl border border-red-200 bg-red-50 px-3.5 py-2.5 text-xs text-red-700 dark:border-red-900/50 dark:bg-red-950/30 dark:text-red-300">
            <AlertCircle className="h-4 w-4 shrink-0" />
            <span>{consoleError}</span>
          </div>
        )}

        {/* Historical Transcript */}
        {messagesLoading ? (
          <div className="space-y-2.5">
            {Array.from({ length: 3 }).map((_, i) => (
              <div
                key={i}
                className="h-16 animate-pulse rounded-2xl bg-zinc-200/60 dark:bg-zinc-800/60"
              />
            ))}
          </div>
        ) : (
          <div className="space-y-3">
            {messages.length === 0 && !activeRun && (
              <div className="rounded-2xl border border-zinc-200/80 bg-white px-4 py-8 text-center text-xs text-zinc-500 dark:border-zinc-800 dark:bg-[#232328]">
                {tr(lang, "remoteNoMessages")}
              </div>
            )}
            {messages
              .filter((m) => !m.injected)
              .map((msg, idx) => (
                <TranscriptMessageCard
                  key={msg.id ?? `${idx}-${msg.role}`}
                  msg={msg}
                  lang={lang}
                />
              ))}
          </div>
        )}

        {/* Active / Recent Live Stream Output */}
        {activeRun && (
          <div className="space-y-2.5 pt-1">
            {/* User prompt bubble for this live run */}
            <div className="flex justify-end">
              <div className="max-w-[85%] whitespace-pre-wrap break-words rounded-2xl rounded-tr-md bg-[#f97316] px-3.5 py-2.5 text-sm leading-relaxed text-white shadow-sm">
                {activeRun.prompt}
              </div>
            </div>

            {/* Run Status Pill */}
            <div className="flex items-center gap-2 px-1">
              <RunStatusBadge status={activeRun.status} lang={lang} />
              {activeRun.model && (
                <span className="truncate text-[11px] text-zinc-500 dark:text-zinc-400">
                  {activeRun.model}
                </span>
              )}
            </div>

            {/* Streamed Blocks */}
            {groupedStream.map((block) => {
              if (block.kind === "thought") {
                return (
                  <CollapsibleCard
                    key={block.key}
                    icon={<Sparkles className="h-3.5 w-3.5 text-amber-500" />}
                    title={tr(lang, "remoteThinking")}
                    content={block.text}
                  />
                );
              }
              if (block.kind === "tool_call") {
                return (
                  <CollapsibleCard
                    key={block.key}
                    icon={<Wrench className="h-3.5 w-3.5 text-sky-500" />}
                    title={`${tr(lang, "remoteToolCall")}${block.toolName ? `: ${block.toolName}` : ""}`}
                    content={block.text}
                    mono
                  />
                );
              }
              if (block.kind === "tool_result") {
                return (
                  <CollapsibleCard
                    key={block.key}
                    icon={
                      <Terminal
                        className={`h-3.5 w-3.5 ${
                          block.isError ? "text-red-500" : "text-emerald-500"
                        }`}
                      />
                    }
                    title={`${tr(lang, "remoteToolResult")}${block.toolName ? `: ${block.toolName}` : ""}`}
                    content={block.text}
                    mono
                  />
                );
              }
              if (block.kind === "stderr") {
                return (
                  <CollapsibleCard
                    key={block.key}
                    icon={
                      <AlertCircle className="h-3.5 w-3.5 text-amber-500" />
                    }
                    title={tr(lang, "remoteStderr")}
                    content={block.text}
                    mono
                  />
                );
              }
              if (block.kind === "error") {
                return (
                  <div
                    key={block.key}
                    className="rounded-2xl border border-red-200 bg-red-50 px-3.5 py-2.5 text-xs text-red-700 dark:border-red-900/50 dark:bg-red-950/30 dark:text-red-300"
                  >
                    {block.text}
                  </div>
                );
              }
              return (
                <div
                  key={block.key}
                  className="whitespace-pre-wrap break-words rounded-2xl rounded-tl-md border border-zinc-200/80 bg-white px-3.5 py-2.5 text-sm leading-relaxed text-zinc-900 dark:border-zinc-800 dark:bg-[#232328] dark:text-zinc-100"
                >
                  {block.text}
                </div>
              );
            })}

            {activeRun.error &&
              !groupedStream.some((b) => b.kind === "error") && (
                <div className="rounded-2xl border border-red-200 bg-red-50 px-3.5 py-2.5 text-xs text-red-700 dark:border-red-900/50 dark:bg-red-950/30 dark:text-red-300">
                  {activeRun.error}
                </div>
              )}

            <div ref={streamEndRef} />
          </div>
        )}

        {/* Sticky Mobile Composer */}
        {canExecute && (
          <div className="fixed inset-x-0 bottom-[calc(60px+env(safe-area-inset-bottom))] z-20 border-t border-zinc-200/80 bg-[#f5f5f7]/95 px-3 py-2.5 backdrop-blur-xl dark:border-zinc-800 dark:bg-[#18181b]/95">
            <div className="mx-auto max-w-lg space-y-2">
              {optionsOpen && agentInfo && (
                <AgentControlStrip
                  agent={agentInfo}
                  lang={lang}
                  model={modelInput}
                  mode={modeInput}
                  effort={effortInput}
                  sandbox={sandboxInput}
                  onChangeModel={setModelInput}
                  onChangeMode={setModeInput}
                  onChangeEffort={setEffortInput}
                  onChangeSandbox={setSandboxInput}
                />
              )}

              <div className="flex items-end gap-2">
                {hasAnyControls && (
                  <button
                    type="button"
                    aria-label={tr(lang, "remoteOptions")}
                    onClick={() => setOptionsOpen((v) => !v)}
                    className={`flex h-11 w-11 shrink-0 items-center justify-center rounded-xl border transition ${
                      optionsOpen
                        ? "border-[#f97316] bg-orange-500/10 text-[#f97316]"
                        : "border-zinc-200 bg-white text-zinc-600 dark:border-zinc-700 dark:bg-[#232328] dark:text-zinc-300"
                    }`}
                  >
                    <SlidersHorizontal className="h-4 w-4" />
                  </button>
                )}

                <textarea
                  rows={1}
                  value={promptInput}
                  disabled={sending || isRunning}
                  onChange={(e) => setPromptInput(e.target.value)}
                  placeholder={tr(lang, "remotePromptPlaceholder")}
                  className="max-h-28 min-h-[44px] flex-1 resize-none rounded-xl border border-zinc-200 bg-white px-3 py-2.5 text-base leading-snug text-zinc-900 placeholder:text-zinc-400 focus:border-[#f97316] focus:outline-none disabled:opacity-60 dark:border-zinc-700 dark:bg-[#232328] dark:text-zinc-100"
                />

                {isRunning ? (
                  <button
                    type="button"
                    disabled={stopping}
                    onClick={() => void handleCancelRun()}
                    aria-label={tr(lang, "remoteStop")}
                    className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-red-600 text-white transition active:scale-95 disabled:opacity-60"
                  >
                    {stopping ? (
                      <Loader2 className="h-4 w-4 animate-spin" />
                    ) : (
                      <Square className="h-4 w-4 fill-current" />
                    )}
                  </button>
                ) : (
                  <button
                    type="button"
                    disabled={sending || !promptInput.trim()}
                    onClick={() => void handleSendFollowUp()}
                    aria-label={tr(lang, "remoteSend")}
                    className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-[#f97316] text-white transition active:scale-95 disabled:opacity-45"
                  >
                    {sending ? (
                      <Loader2 className="h-4 w-4 animate-spin" />
                    ) : (
                      <Send className="h-4 w-4" />
                    )}
                  </button>
                )}
              </div>
            </div>
          </div>
        )}
      </div>
    );
  }

  // ============================================================
  // Sub-view A: Sessions List & Agent Filter
  // ============================================================
  const activeRuns = runs.filter((r) => r.status === "running");
  const selectedNewAgentInfo = agentMap.get(newAgentId);

  return (
    <div className="space-y-4">
      {/* Agent Filter Pills */}
      <div className="-mx-4 flex gap-1.5 overflow-x-auto px-4 pb-1">
        <button
          type="button"
          onClick={() => setSelectedFilter("all")}
          className={`min-h-[36px] shrink-0 rounded-full px-3.5 text-xs font-medium transition ${
            selectedFilter === "all"
              ? "bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900"
              : "bg-white text-zinc-600 ring-1 ring-zinc-200 dark:bg-[#232328] dark:text-zinc-300 dark:ring-zinc-800"
          }`}
        >
          {tr(lang, "all")}
        </button>
        {filterAgents.map((agent) => {
          const active = selectedFilter === agent.id;
          return (
            <button
              key={agent.id}
              type="button"
              onClick={() => setSelectedFilter(agent.id)}
              className={`inline-flex min-h-[36px] shrink-0 items-center gap-1.5 rounded-full px-3 text-xs font-medium transition ${
                active
                  ? "bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900"
                  : "bg-white text-zinc-700 ring-1 ring-zinc-200 dark:bg-[#232328] dark:text-zinc-300 dark:ring-zinc-800"
              }`}
            >
              <span
                className={`h-2 w-2 rounded-full ${
                  agent.enabled && agent.installed
                    ? "bg-emerald-500"
                    : agent.installed
                      ? "bg-zinc-400"
                      : "bg-zinc-300 dark:bg-zinc-600"
                }`}
              />
              {formatAppName(agent.id)}
            </button>
          );
        })}
      </div>

      {/* No Agents Enabled Notice */}
      {agentsData && enabledInstalledAgents.length === 0 && (
        <div className="flex items-start gap-2.5 rounded-2xl border border-amber-200/80 bg-amber-50/90 px-3.5 py-3 text-xs leading-relaxed text-amber-800 dark:border-amber-900/50 dark:bg-amber-950/30 dark:text-amber-300">
          <Lock className="mt-0.5 h-4 w-4 shrink-0" />
          <span>{tr(lang, "remoteNoEnabledAgents")}</span>
        </div>
      )}

      {/* Search + "+ New Session" Row */}
      <div className="flex items-center gap-2">
        <div className="relative flex-1">
          <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-zinc-400" />
          <input
            type="search"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder={tr(lang, "remoteSearchPlaceholder")}
            className="min-h-[44px] w-full rounded-xl border border-zinc-200 bg-white pl-9 pr-3 text-base text-zinc-900 placeholder:text-zinc-400 focus:border-[#f97316] focus:outline-none dark:border-zinc-800 dark:bg-[#232328] dark:text-zinc-100"
          />
        </div>
        <button
          type="button"
          disabled={enabledInstalledAgents.length === 0}
          onClick={handleOpenNewSheet}
          className="flex min-h-[44px] shrink-0 items-center gap-1.5 rounded-xl bg-[#f97316] px-3.5 text-xs font-semibold text-white shadow-sm transition active:scale-95 disabled:opacity-45"
        >
          <Plus className="h-4 w-4" />
          {tr(lang, "remoteNewSession")}
        </button>
      </div>

      {listError && (
        <div className="flex items-center justify-between gap-2 rounded-2xl border border-red-200 bg-red-50 px-4 py-3 text-xs text-red-700 dark:border-red-900/50 dark:bg-red-950/30 dark:text-red-300">
          <span className="flex items-center gap-2">
            <AlertCircle className="h-4 w-4 shrink-0" />
            {listError}
          </span>
          <button
            type="button"
            onClick={() => void loadList()}
            className="shrink-0 rounded-lg bg-red-600 px-3 py-1.5 font-semibold text-white"
          >
            {tr(lang, "retry")}
          </button>
        </div>
      )}

      {/* Currently Running Turns */}
      {activeRuns.length > 0 && (
        <section className="space-y-2">
          <h2 className="px-1 text-xs font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">
            {tr(lang, "remoteActiveRuns")}
          </h2>
          <div className="space-y-2">
            {activeRuns.map((run) => (
              <button
                key={run.runId}
                type="button"
                onClick={() => void openRunDirectly(run)}
                className="flex w-full items-center justify-between gap-3 rounded-2xl border border-emerald-500/40 bg-emerald-500/[0.06] p-3.5 text-left transition active:scale-[0.99] dark:bg-emerald-500/10"
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="h-2 w-2 animate-pulse rounded-full bg-emerald-500" />
                    <span className="rounded-md bg-emerald-500/15 px-1.5 py-0.5 text-[10px] font-semibold text-emerald-700 dark:text-emerald-300">
                      {formatAppName(run.app)}
                    </span>
                    <span className="truncate text-xs font-semibold text-zinc-900 dark:text-zinc-100">
                      {run.prompt}
                    </span>
                  </div>
                  <div className="mt-1 truncate text-[11px] text-zinc-500 dark:text-zinc-400">
                    {folderBasename(run.cwd)}
                  </div>
                </div>
                <ChevronRight className="h-4 w-4 shrink-0 text-zinc-400" />
              </button>
            ))}
          </div>
        </section>
      )}

      {/* Sessions List */}
      {listLoading && !agentsData ? (
        <div className="space-y-2.5">
          {Array.from({ length: 4 }).map((_, i) => (
            <div
              key={i}
              className="h-20 animate-pulse rounded-2xl bg-zinc-200/60 dark:bg-zinc-800/60"
            />
          ))}
        </div>
      ) : sessions.length === 0 ? (
        <div className="rounded-2xl border border-zinc-200/80 bg-white px-4 py-10 text-center text-sm text-zinc-500 dark:border-zinc-800 dark:bg-[#232328]">
          {tr(lang, "remoteNoSessions")}
        </div>
      ) : (
        <div className="space-y-2">
          {sessions.map((session) => {
            const info = agentMap.get(session.app);
            const isWritable =
              Boolean(info?.enabled && info?.installed) && session.resumable;
            return (
              <button
                key={`${session.app}:${session.sessionId}`}
                type="button"
                onClick={() => void openSession(session)}
                className="flex w-full items-center justify-between gap-3 rounded-2xl border border-zinc-200/80 bg-white p-3.5 text-left transition active:scale-[0.99] dark:border-zinc-800 dark:bg-[#232328]"
              >
                <div className="min-w-0 flex-1 space-y-1">
                  <div className="flex items-center gap-1.5">
                    {session.activeRunId && (
                      <span className="h-2 w-2 shrink-0 animate-pulse rounded-full bg-emerald-500" />
                    )}
                    <span className="shrink-0 rounded-md bg-zinc-100 px-1.5 py-0.5 text-[10px] font-semibold text-zinc-700 dark:bg-zinc-800 dark:text-zinc-300">
                      {formatAppName(session.app)}
                    </span>
                    <span className="truncate text-sm font-semibold text-zinc-900 dark:text-zinc-100">
                      {session.title ||
                        session.summary ||
                        tr(lang, "remoteUntitled")}
                    </span>
                  </div>
                  <div className="flex flex-wrap items-center gap-2 text-[11px] text-zinc-500 dark:text-zinc-400">
                    {session.projectDir && (
                      <span className="inline-flex max-w-[180px] items-center gap-1 truncate">
                        <Folder className="h-3 w-3 shrink-0" />
                        <span className="truncate">
                          {folderBasename(session.projectDir)}
                        </span>
                      </span>
                    )}
                    {(session.lastActiveAt || session.createdAt) && (
                      <span>
                        {formatSessionTimestamp(
                          session.lastActiveAt ?? session.createdAt,
                        )}
                      </span>
                    )}
                    {!isWritable && (
                      <span className="rounded bg-zinc-100 px-1.5 py-0.5 text-[10px] dark:bg-zinc-800">
                        {tr(lang, "remoteReadOnlyTag")}
                      </span>
                    )}
                  </div>
                </div>
                <ChevronRight className="h-4 w-4 shrink-0 text-zinc-400" />
              </button>
            );
          })}
        </div>
      )}

      {/* New Session Bottom Sheet */}
      {newSheetOpen && (
        <div
          className="fixed inset-0 z-40 flex items-end justify-center bg-black/45 backdrop-blur-[2px]"
          onClick={() => setNewSheetOpen(false)}
          role="dialog"
          aria-modal="true"
          aria-label={tr(lang, "remoteNewSession")}
        >
          <div
            className="max-h-[88dvh] w-full max-w-lg overflow-y-auto rounded-t-3xl border-t border-zinc-200 bg-white px-4 pb-[calc(1.5rem+env(safe-area-inset-bottom))] pt-3 shadow-2xl dark:border-zinc-800 dark:bg-[#232328]"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="mx-auto mb-3 h-1 w-10 rounded-full bg-zinc-300 dark:bg-zinc-700" />
            <div className="mb-3 flex items-center justify-between">
              <h2 className="text-sm font-semibold text-zinc-900 dark:text-zinc-100">
                {tr(lang, "remoteNewSession")}
              </h2>
              <button
                type="button"
                onClick={() => setNewSheetOpen(false)}
                aria-label={tr(lang, "close")}
                className="flex h-9 w-9 items-center justify-center rounded-full text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800"
              >
                <X className="h-4 w-4" />
              </button>
            </div>

            <div className="space-y-4">
              {/* Choose Enabled Agent */}
              <div className="space-y-1.5">
                <label className="text-xs font-medium text-zinc-500 dark:text-zinc-400">
                  {tr(lang, "remoteChooseAgent")}
                </label>
                <div className="flex flex-wrap gap-1.5">
                  {enabledInstalledAgents.map((agent) => (
                    <button
                      key={agent.id}
                      type="button"
                      onClick={() => handleSelectNewAgent(agent.id)}
                      className={`min-h-[36px] rounded-xl px-3 text-xs font-semibold transition ${
                        newAgentId === agent.id
                          ? "bg-[#f97316] text-white"
                          : "bg-zinc-100 text-zinc-700 dark:bg-zinc-800 dark:text-zinc-200"
                      }`}
                    >
                      {formatAppName(agent.id)}
                    </button>
                  ))}
                </div>
              </div>

              {/* Recent Workspaces + Custom Path */}
              <div className="space-y-1.5">
                <label className="text-xs font-medium text-zinc-500 dark:text-zinc-400">
                  {tr(lang, "remoteWorkspace")}
                </label>
                {(agentsData?.recentWorkspaces.length ?? 0) > 0 && (
                  <div className="flex flex-wrap gap-1.5">
                    {agentsData!.recentWorkspaces.slice(0, 6).map((dir) => (
                      <button
                        key={dir}
                        type="button"
                        onClick={() => setNewCwd(dir)}
                        className={`inline-flex max-w-full items-center gap-1 rounded-lg px-2.5 py-1.5 text-xs transition ${
                          newCwd === dir
                            ? "bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900"
                            : "bg-zinc-100 text-zinc-700 dark:bg-zinc-800 dark:text-zinc-300"
                        }`}
                      >
                        <Folder className="h-3 w-3 shrink-0" />
                        <span className="truncate">{folderBasename(dir)}</span>
                      </button>
                    ))}
                  </div>
                )}
                <input
                  type="text"
                  value={newCwd}
                  onChange={(e) => setNewCwd(e.target.value)}
                  placeholder={tr(lang, "remoteWorkspacePlaceholder")}
                  className="min-h-[44px] w-full rounded-xl border border-zinc-200 bg-zinc-50 px-3 font-mono text-base text-zinc-900 focus:border-[#f97316] focus:outline-none dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100"
                />
              </div>

              {/* Agent-specific Controls */}
              {selectedNewAgentInfo && (
                <AgentControlStrip
                  agent={selectedNewAgentInfo}
                  lang={lang}
                  model={newModel}
                  mode={newMode}
                  effort={newEffort}
                  sandbox={newSandbox}
                  onChangeModel={setNewModel}
                  onChangeMode={setNewMode}
                  onChangeEffort={setNewEffort}
                  onChangeSandbox={setNewSandbox}
                />
              )}

              {/* Prompt */}
              <textarea
                rows={3}
                value={newPrompt}
                onChange={(e) => setNewPrompt(e.target.value)}
                placeholder={tr(lang, "remotePromptPlaceholder")}
                className="w-full rounded-xl border border-zinc-200 bg-zinc-50 px-3 py-2.5 text-base text-zinc-900 placeholder:text-zinc-400 focus:border-[#f97316] focus:outline-none dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100"
              />

              {consoleError && (
                <div className="rounded-xl border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700 dark:border-red-900/50 dark:bg-red-950/30 dark:text-red-300">
                  {consoleError}
                </div>
              )}

              <button
                type="button"
                disabled={sending || !newPrompt.trim() || !newAgentId}
                onClick={() => void handleStartNewSession()}
                className="flex min-h-[48px] w-full items-center justify-center gap-2 rounded-xl bg-[#f97316] px-4 text-sm font-semibold text-white transition active:scale-[0.99] disabled:opacity-50"
              >
                {sending ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <Send className="h-4 w-4" />
                )}
                {sending
                  ? tr(lang, "remoteStarting")
                  : tr(lang, "remoteStartBtn")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function AgentControlStrip({
  agent,
  lang,
  model,
  mode,
  effort,
  sandbox,
  onChangeModel,
  onChangeMode,
  onChangeEffort,
  onChangeSandbox,
}: {
  agent: RemoteAgentInfo;
  lang: MobileLang;
  model: string;
  mode: string;
  effort: string;
  sandbox: boolean;
  onChangeModel: (v: string) => void;
  onChangeMode: (v: string) => void;
  onChangeEffort: (v: string) => void;
  onChangeSandbox: (v: boolean) => void;
}) {
  return (
    <div className="space-y-2.5 rounded-2xl border border-zinc-200/80 bg-zinc-50/90 p-3 dark:border-zinc-800 dark:bg-zinc-900/60">
      {agent.supportsModel && (
        <div className="flex items-center gap-2">
          <span className="w-14 shrink-0 text-[11px] font-medium text-zinc-500 dark:text-zinc-400">
            {tr(lang, "remoteModel")}
          </span>
          <input
            type="text"
            value={model}
            onChange={(e) => onChangeModel(e.target.value)}
            placeholder={tr(lang, "remoteModelPlaceholder")}
            className="min-h-[36px] flex-1 rounded-lg border border-zinc-200 bg-white px-2.5 text-base text-zinc-900 focus:border-[#f97316] focus:outline-none dark:border-zinc-700 dark:bg-[#232328] dark:text-zinc-100"
          />
        </div>
      )}

      {agent.supportsMode && agent.modes.length > 0 && (
        <div className="flex items-center gap-2">
          <span className="w-14 shrink-0 text-[11px] font-medium text-zinc-500 dark:text-zinc-400">
            {tr(lang, "remoteMode")}
          </span>
          <div className="flex flex-wrap gap-1">
            {agent.modes.map((m) => (
              <button
                key={m}
                type="button"
                onClick={() => onChangeMode(m)}
                className={`rounded-lg px-2.5 py-1 text-[11px] font-medium transition ${
                  mode === m
                    ? "bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900"
                    : "bg-white text-zinc-600 ring-1 ring-zinc-200 dark:bg-[#232328] dark:text-zinc-300 dark:ring-zinc-700"
                }`}
              >
                {m}
              </button>
            ))}
          </div>
        </div>
      )}

      {agent.supportsEffort && agent.efforts.length > 0 && (
        <div className="flex items-center gap-2">
          <span className="w-14 shrink-0 text-[11px] font-medium text-zinc-500 dark:text-zinc-400">
            {tr(lang, "remoteEffort")}
          </span>
          <div className="flex flex-wrap gap-1">
            {agent.efforts.map((e) => (
              <button
                key={e}
                type="button"
                onClick={() => onChangeEffort(effort === e ? "" : e)}
                className={`rounded-lg px-2.5 py-1 text-[11px] font-medium transition ${
                  effort === e
                    ? "bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900"
                    : "bg-white text-zinc-600 ring-1 ring-zinc-200 dark:bg-[#232328] dark:text-zinc-300 dark:ring-zinc-700"
                }`}
              >
                {e}
              </button>
            ))}
          </div>
        </div>
      )}

      {agent.supportsSandbox && (
        <div className="flex items-center justify-between">
          <span className="inline-flex items-center gap-1.5 text-[11px] font-medium text-zinc-600 dark:text-zinc-300">
            <Shield className="h-3.5 w-3.5 text-sky-500" />
            {tr(lang, "remoteSandbox")}
          </span>
          <button
            type="button"
            role="switch"
            aria-checked={sandbox}
            onClick={() => onChangeSandbox(!sandbox)}
            className={`relative h-6 w-11 rounded-full transition ${
              sandbox ? "bg-[#f97316]" : "bg-zinc-300 dark:bg-zinc-700"
            }`}
          >
            <span
              className={`absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition ${
                sandbox ? "left-[22px]" : "left-0.5"
              }`}
            />
          </button>
        </div>
      )}
    </div>
  );
}

function RunStatusBadge({
  status,
  lang,
}: {
  status: RemoteRunSnapshot["status"];
  lang: MobileLang;
}) {
  if (status === "running") {
    return (
      <span className="inline-flex items-center gap-1.5 rounded-full bg-emerald-500/15 px-2.5 py-0.5 text-[11px] font-semibold text-emerald-700 dark:text-emerald-300">
        <Loader2 className="h-3 w-3 animate-spin" />
        {tr(lang, "remoteStatusRunning")}
      </span>
    );
  }
  if (status === "completed") {
    return (
      <span className="inline-flex items-center rounded-full bg-zinc-200/80 px-2.5 py-0.5 text-[11px] font-medium text-zinc-700 dark:bg-zinc-800 dark:text-zinc-300">
        {tr(lang, "remoteStatusCompleted")}
      </span>
    );
  }
  if (status === "cancelled") {
    return (
      <span className="inline-flex items-center rounded-full bg-amber-500/15 px-2.5 py-0.5 text-[11px] font-medium text-amber-700 dark:text-amber-300">
        {tr(lang, "remoteStatusCancelled")}
      </span>
    );
  }
  return (
    <span className="inline-flex items-center rounded-full bg-red-500/15 px-2.5 py-0.5 text-[11px] font-medium text-red-700 dark:text-red-300">
      {tr(lang, "remoteStatusError")}
    </span>
  );
}

function CollapsibleCard({
  icon,
  title,
  content,
  mono = false,
}: {
  icon: React.ReactNode;
  title: string;
  content: string;
  mono?: boolean;
}) {
  const [open, setOpen] = useState(false);
  return (
    <div className="overflow-hidden rounded-2xl border border-zinc-200/80 bg-zinc-50/90 dark:border-zinc-800 dark:bg-[#202026]">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex min-h-[40px] w-full items-center justify-between gap-2 px-3 py-2 text-left text-xs font-medium text-zinc-700 dark:text-zinc-300"
      >
        <span className="flex min-w-0 items-center gap-2">
          {icon}
          <span className="truncate">{title}</span>
        </span>
        {open ? (
          <ChevronDown className="h-3.5 w-3.5 shrink-0 text-zinc-400" />
        ) : (
          <ChevronRight className="h-3.5 w-3.5 shrink-0 text-zinc-400" />
        )}
      </button>
      {open && content && (
        <div
          className={`max-h-60 overflow-auto border-t border-zinc-200/70 px-3 py-2 text-xs leading-relaxed text-zinc-600 dark:border-zinc-800 dark:text-zinc-400 ${
            mono
              ? "whitespace-pre-wrap break-all font-mono"
              : "whitespace-pre-wrap break-words"
          }`}
        >
          {content}
        </div>
      )}
    </div>
  );
}

function TranscriptMessageCard({
  msg,
  lang,
}: {
  msg: RemoteSessionMessage;
  lang: MobileLang;
}) {
  const blocks: RemoteSessionBlock[] =
    msg.blocks && msg.blocks.length > 0
      ? msg.blocks
      : msg.content
        ? [{ type: "text", text: msg.content }]
        : [];

  if (blocks.length === 0) return null;

  if (msg.role === "user") {
    const text = blocks
      .map((b) => b.text ?? "")
      .filter(Boolean)
      .join("\n");
    if (!text) return null;
    return (
      <div className="flex justify-end">
        <div className="max-w-[85%] whitespace-pre-wrap break-words rounded-2xl rounded-tr-md bg-zinc-900 px-3.5 py-2.5 text-sm leading-relaxed text-white dark:bg-zinc-100 dark:text-zinc-900">
          {text}
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-1.5">
      {blocks.map((block, idx) => {
        if (block.type === "thinking") {
          return (
            <CollapsibleCard
              key={idx}
              icon={<Sparkles className="h-3.5 w-3.5 text-amber-500" />}
              title={block.summary || tr(lang, "remoteThinking")}
              content={block.text || ""}
            />
          );
        }
        if (block.type === "tool_call") {
          return (
            <CollapsibleCard
              key={idx}
              icon={<Wrench className="h-3.5 w-3.5 text-sky-500" />}
              title={`${block.rawName ?? tr(lang, "remoteToolCall")}${block.title ? `: ${block.title}` : ""}`}
              content={block.inputPreview || block.detail || ""}
              mono
            />
          );
        }
        if (block.type === "tool_result") {
          return (
            <CollapsibleCard
              key={idx}
              icon={
                <Terminal
                  className={`h-3.5 w-3.5 ${
                    block.status === "error"
                      ? "text-red-500"
                      : "text-emerald-500"
                  }`}
                />
              }
              title={tr(lang, "remoteToolResult")}
              content={block.preview || ""}
              mono
            />
          );
        }
        if (block.type === "text" && block.text) {
          return (
            <div
              key={idx}
              className="whitespace-pre-wrap break-words rounded-2xl rounded-tl-md border border-zinc-200/80 bg-white px-3.5 py-2.5 text-sm leading-relaxed text-zinc-900 dark:border-zinc-800 dark:bg-[#232328] dark:text-zinc-100"
            >
              {block.text}
            </div>
          );
        }
        return null;
      })}
    </div>
  );
}
