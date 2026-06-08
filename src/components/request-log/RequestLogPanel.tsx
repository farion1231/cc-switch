import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  Search,
  Trash2,
  Circle,
  Copy,
  X,
  FileText,
  MessageSquare,
  MessageCircle,
  Bot,
  Wrench,
  Settings,
  User,
  Braces,
  ChevronsDownUp,
  ChevronsUpDown,
} from "lucide-react";
import {
  JsonView,
  allExpanded,
  collapseAllNested,
  darkStyles,
  defaultStyles,
} from "react-json-view-lite";
import { useTheme } from "@/components/theme-provider";
import "react-json-view-lite/dist/index.css";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import type {
  ProxyRequestLogEntry,
  RequestLogEventPayload,
  RequestLogSummary,
  RequestLogUpdatedPayload,
} from "@/lib/api/request-log";
import { requestLogApi } from "@/lib/api/request-log";

const PREVIEW_MAX_CHARS = 500;

/** Truncate JSON string for inline preview to avoid rendering large DOM */
function truncateJsonPreview(data: unknown): string {
  if (data == null) return "null";
  if (typeof data !== "object") return String(data);
  if (Array.isArray(data) && data.length > 20) {
    const preview = JSON.stringify(data.slice(0, 10), null, 2);
    return preview + `\n… (${data.length} items total)`;
  }
  const full = JSON.stringify(data, null, 2);
  if (full.length <= PREVIEW_MAX_CHARS) return full;
  return full.slice(0, PREVIEW_MAX_CHARS) + "\n… (truncated)";
}

/** Simplified log entry used in the list */
interface LogListItem {
  id: string;
  timestamp: string;
  appType: string;
  providerName: string;
  method: string;
  endpoint: string;
  model: string;
  isStream: boolean;
  statusCode: number | null;
  latencyMs: number | null;
  hasSystemPrompt: boolean;
  systemPromptPreview: string | null;
  /** Information about the last content item of the user's last message */
  userQuery: { type: string; text: string } | null;
}

function summaryToListItem(s: RequestLogSummary): LogListItem {
  return {
    id: s.id,
    timestamp: s.timestamp,
    appType: s.app_type,
    providerName: s.provider_name,
    method: s.method,
    endpoint: s.endpoint,
    model: s.model,
    isStream: s.is_stream,
    statusCode: s.status_code,
    latencyMs: s.latency_ms,
    hasSystemPrompt: s.has_system_prompt,
    systemPromptPreview: s.system_prompt_preview,
    userQuery:
      s.user_query != null && s.user_query_type != null
        ? { type: s.user_query_type, text: s.user_query }
        : null,
  };
}

function fromEventPayload(payload: RequestLogEventPayload): LogListItem {
  return {
    id: payload.id,
    timestamp: payload.timestamp,
    appType: payload.app_type,
    providerName: payload.provider_name,
    method: payload.method,
    endpoint: payload.endpoint,
    model: payload.model,
    isStream: payload.is_stream,
    statusCode: payload.status_code,
    latencyMs: payload.latency_ms,
    hasSystemPrompt: payload.has_system_prompt,
    systemPromptPreview: payload.system_prompt_preview,
    userQuery:
      payload.user_query != null && payload.user_query_type != null
        ? { type: payload.user_query_type, text: payload.user_query }
        : null,
  };
}

function formatTimestamp(iso: string): string {
  try {
    const date = new Date(iso);
    return date.toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  } catch {
    return iso;
  }
}

function statusColor(code: number | null): string {
  if (code == null) return "text-muted-foreground";
  if (code >= 200 && code < 300) return "text-emerald-500";
  if (code >= 400 && code < 500) return "text-amber-500";
  return "text-red-500";
}

const APP_TYPE_OPTIONS = [
  { value: "all", label: "All" },
  { value: "claude", label: "Claude" },
  { value: "codex", label: "Codex" },
  { value: "gemini", label: "Gemini" },
  { value: "opencode", label: "OpenCode" },
  { value: "openclaw", label: "OpenClaw" },
  { value: "hermes", label: "Hermes" },
];

const LogListItemRow = React.memo(function LogListItemRow({
  log,
  isSelected,
  onSelect,
}: {
  log: LogListItem;
  isSelected: boolean;
  onSelect: (id: string) => void;
}) {
  return (
    <button
      type="button"
      className={cn(
        "w-full text-left px-3 py-2 hover:bg-muted/50 transition-colors cursor-pointer border-b",
        isSelected && "bg-muted",
      )}
      onClick={() => onSelect(log.id)}
    >
      <div className="flex items-center gap-2 mb-0.5">
        <span className="text-[10px] text-muted-foreground font-mono tabular-nums">
          {formatTimestamp(log.timestamp)}
        </span>
        <Badge variant="outline" className="text-[10px] px-1 py-0 h-4 font-normal">
          {log.appType}
        </Badge>
        <span className={cn("text-[10px] font-mono font-semibold", statusColor(log.statusCode))}>
          {log.statusCode ?? "…"}
        </span>
        {log.isStream && (
          <Badge variant="secondary" className="text-[10px] px-1 py-0 h-4 font-normal">
            SSE
          </Badge>
        )}
        {log.hasSystemPrompt && (
          <FileText className="w-3 h-3 text-blue-500 flex-shrink-0" />
        )}
        <span className="ml-auto text-[10px] text-muted-foreground tabular-nums">
          {log.latencyMs != null ? `${log.latencyMs}ms` : ""}
        </span>
      </div>
      <div className="flex items-center gap-1.5">
        <span className="text-[10px] font-mono text-muted-foreground uppercase">
          {log.method}
        </span>
        <span className="text-xs font-mono truncate">{log.endpoint}</span>
        <span className="text-[10px] text-muted-foreground">·</span>
        <span className="text-[10px] text-muted-foreground truncate shrink-0">
          {log.providerName}
        </span>
        <span className="text-[10px] text-muted-foreground">·</span>
        <span className="text-[10px] text-muted-foreground truncate">{log.model}</span>
      </div>
      {log.userQuery && (
        <div className="flex items-center gap-1.5 mt-0.5">
          <MessageSquare className="w-3 h-3 text-muted-foreground shrink-0" />
          <Badge
            variant="outline"
            className="text-[10px] px-1 py-0 h-4 font-mono font-normal shrink-0"
          >
            {log.userQuery.type}
          </Badge>
          {log.userQuery.text ? (
            <span className="text-[11px] text-foreground/70 truncate">
              {log.userQuery.text}
            </span>
          ) : (
            <span className="text-[10px] text-muted-foreground italic">
              [{log.userQuery.type} content]
            </span>
          )}
        </div>
      )}
    </button>
  );
});

export function RequestLogPanel() {
  const { t } = useTranslation();
  const { theme } = useTheme();
  const isDark =
    theme === "dark" ||
    (theme === "system" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches);
  const jsonViewStyle = isDark ? darkStyles : defaultStyles;
  const [captureEnabled, setCaptureEnabled] = useState(false);
  const [logs, setLogs] = useState<LogListItem[]>([]);
  const [selectedLogId, setSelectedLogId] = useState<string | null>(null);
  const [detailEntry, setDetailEntry] = useState<ProxyRequestLogEntry | null>(
    null,
  );
  const [detailLoading, setDetailLoading] = useState(false);
  const [showFormattedView, setShowFormattedView] = useState(false);
  const [showSystemPromptView, setShowSystemPromptView] = useState(false);
  const [showResponseFormatView, setShowResponseFormatView] = useState(false);
  const [showRequestJsonView, setShowRequestJsonView] = useState(false);
  const [showResponseJsonView, setShowResponseJsonView] = useState(false);
  const [requestJsonExpanded, setRequestJsonExpanded] = useState(false);
  const [responseJsonExpanded, setResponseJsonExpanded] = useState(false);
  const [maxEntries, setMaxEntries] = useState(200);

  // Filtering
  const [searchQuery, setSearchQuery] = useState("");
  const [appTypeFilter, setAppTypeFilter] = useState("all");
  const [systemPromptOnly, setSystemPromptOnly] = useState(false);

  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const maxEntriesRef = useRef(maxEntries);
  maxEntriesRef.current = maxEntries;
  const selectedLogIdRef = useRef(selectedLogId);
  selectedLogIdRef.current = selectedLogId;

  // Initialize: load switch state and existing logs (using lightweight summaries)
  // On unmount (navigate away): disable capture to prevent background memory growth
  useEffect(() => {
    void (async () => {
      try {
        const [enabled, max] = await Promise.all([
          requestLogApi.isCaptureEnabled(),
          requestLogApi.getMaxEntries(),
        ]);
        setCaptureEnabled(enabled);
        setMaxEntries(max);
        const summaries = await requestLogApi.getLogSummaries();
        setLogs(summaries.map(summaryToListItem));
      } catch (error) {
        console.error("Failed to init request log panel:", error);
      }
    })();
    return () => {
      void requestLogApi.setCaptureEnabled(false);
    };
  }, []);

  // Receive new logs in real-time (userQuery is now included in the event payload)
  useTauriEvent<RequestLogEventPayload>("proxy-request-log", (payload) => {
    const item = fromEventPayload(payload);
    setLogs((prev) => {
      const limit = maxEntriesRef.current;
      const next = [item, ...prev];
      return next.length > limit ? next.slice(0, limit) : next;
    });
  });

  // Receive status/latency updates when response completes
  useTauriEvent<RequestLogUpdatedPayload>(
    "proxy-request-log-updated",
    (payload) => {
      setLogs((prev) =>
        prev.map((log) =>
          log.id === payload.id
            ? { ...log, statusCode: payload.status_code, latencyMs: payload.latency_ms }
            : log,
        ),
      );
      // If the currently viewed detail matches, refresh it
      if (selectedLogIdRef.current === payload.id && payload.has_response_body) {
        void requestLogApi.getLogDetail(payload.id).then((detail) => {
          if (detail) setDetailEntry(detail);
        });
      }
    },
  );

  // Toggle capture switch
  const handleToggleCapture = useCallback(
    async (enabled: boolean) => {
      try {
        await requestLogApi.setCaptureEnabled(enabled);
        setCaptureEnabled(enabled);
        if (enabled) {
          const summaries = await requestLogApi.getLogSummaries();
          setLogs(summaries.map(summaryToListItem));
        }
      } catch (error) {
        toast.error(
          t("requestLog.toggleFailed", { defaultValue: "切换捕获失败" }),
        );
      }
    },
    [t],
  );

  // Clear logs
  const handleClear = useCallback(async () => {
    try {
      await requestLogApi.clearLogs();
      setLogs([]);
      setSelectedLogId(null);
      setDetailEntry(null);
    } catch (error) {
      toast.error(t("requestLog.clearFailed", { defaultValue: "清空失败" }));
    }
  }, [t]);

  // View details (response_body backfill now handled by proxy-request-log-updated event)
  const handleSelectLog = useCallback(
    async (id: string) => {
      if (selectedLogIdRef.current === id) {
        setSelectedLogId(null);
        setDetailEntry(null);
        return;
      }
      setSelectedLogId(id);
      setDetailLoading(true);
      try {
        const detail = await requestLogApi.getLogDetail(id);
        setDetailEntry(detail);
      } catch (error) {
        console.error("Failed to load log detail:", error);
        setDetailEntry(null);
      } finally {
        setDetailLoading(false);
      }
    },
    [],
  );

  // Copy to clipboard
  const handleCopy = useCallback(
    async (text: string) => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("copy_text_to_clipboard", { text });
        toast.success(t("common.copied", { defaultValue: "已复制" }));
      } catch {
        // fallback
        await navigator.clipboard.writeText(text);
        toast.success(t("common.copied", { defaultValue: "已复制" }));
      }
    },
    [t],
  );

  // Filtered logs
  const filteredLogs = useMemo(() => {
    let result = logs;

    if (appTypeFilter !== "all") {
      result = result.filter((log) => log.appType === appTypeFilter);
    }

    if (systemPromptOnly) {
      result = result.filter((log) => log.hasSystemPrompt);
    }

    if (searchQuery.trim()) {
      const query = searchQuery.toLowerCase();
      result = result.filter(
        (log) =>
          log.endpoint.toLowerCase().includes(query) ||
          log.model.toLowerCase().includes(query) ||
          log.providerName.toLowerCase().includes(query) ||
          log.systemPromptPreview?.toLowerCase().includes(query) ||
          log.userQuery?.text.toLowerCase().includes(query),
      );
    }

    return result;
  }, [logs, appTypeFilter, systemPromptOnly, searchQuery]);

  const virtualizer = useVirtualizer({
    count: filteredLogs.length,
    getScrollElement: () => scrollContainerRef.current,
    estimateSize: () => 80,
    overscan: 10,
  });

  const onSelectRow = useCallback(
    (id: string) => void handleSelectLog(id),
    [handleSelectLog],
  );

  return (
    <div className="flex flex-col flex-1 min-h-0 overflow-hidden px-6">
      {/* Toolbar */}
      <div className="flex items-center gap-3 py-3 border-b">
        <div className="flex items-center gap-2">
          <Switch
            checked={captureEnabled}
            onCheckedChange={handleToggleCapture}
          />
          <span className="text-sm font-medium">
            {captureEnabled
              ? t("requestLog.capturing", { defaultValue: "捕获中" })
              : t("requestLog.paused", { defaultValue: "已暂停" })}
          </span>
          {captureEnabled && (
            <span className="relative flex h-2 w-2">
              <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75" />
              <span className="relative inline-flex rounded-full h-2 w-2 bg-emerald-500" />
            </span>
          )}
          <span className="text-[10px] text-muted-foreground tabular-nums ml-1">
            {filteredLogs.length !== logs.length
              ? `${filteredLogs.length}/${logs.length}`
              : `${logs.length} 条`}
          </span>
        </div>

        <div className="flex-1" />

        <div className="flex items-center gap-2">
          <div className="relative">
            <Search className="absolute left-2.5 top-2.5 h-3.5 w-3.5 text-muted-foreground" />
            <Input
              placeholder={t("requestLog.searchPlaceholder", {
                defaultValue: "搜索端点、模型、供应商...",
              })}
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="pl-8 h-8 w-52 text-xs"
            />
          </div>

          <Select value={appTypeFilter} onValueChange={setAppTypeFilter}>
            <SelectTrigger className="h-8 w-28 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {APP_TYPE_OPTIONS.map((opt) => (
                <SelectItem key={opt.value} value={opt.value}>
                  {opt.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>

          <Button
            variant={systemPromptOnly ? "secondary" : "ghost"}
            size="sm"
            onClick={() => setSystemPromptOnly(!systemPromptOnly)}
            className="h-8 text-xs gap-1"
            title={t("requestLog.systemPromptOnly", {
              defaultValue: "仅显示含 System Prompt 的请求",
            })}
          >
            <FileText className="w-3.5 h-3.5" />
            Prompt
          </Button>

          <Select
            value={String(maxEntries)}
            onValueChange={(val) => {
              const num = Number(val);
              setMaxEntries(num);
              void requestLogApi.setMaxEntries(num);
              // Frontend also truncates existing logs, keeping only the latest num entries
              setLogs((prev) =>
                prev.length > num ? prev.slice(0, num) : prev,
              );
            }}
          >
            <SelectTrigger className="h-8 w-24 text-xs" title="Maximum entries to keep">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {[1, 10, 50, 100, 200].map((n) => (
                <SelectItem key={n} value={String(n)}>
                  Max {n}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>

          <Button
            variant="ghost"
            size="sm"
            onClick={handleClear}
            className="h-8 text-xs gap-1 text-muted-foreground hover:text-destructive"
            title={t("requestLog.clear", { defaultValue: "清空日志" })}
          >
            <Trash2 className="w-3.5 h-3.5" />
          </Button>
        </div>
      </div>

      {/* Main area */}
      <div className="flex h-0 flex-1 overflow-hidden gap-0">
        {/* Left list */}
        <div
          className={cn(
            "flex flex-col min-h-0 overflow-hidden border-r transition-all",
            selectedLogId ? "w-[45%]" : "w-full",
          )}
        >
          {!captureEnabled && logs.length === 0 ? (
            <div className="flex flex-col items-center justify-center flex-1 text-muted-foreground gap-2 py-12">
              <Circle className="w-8 h-8 opacity-30" />
              <p className="text-sm">
                {t("requestLog.enableToStart", {
                  defaultValue: "开启捕获开关以开始记录代理请求",
                })}
              </p>
            </div>
          ) : filteredLogs.length === 0 ? (
            <div className="flex flex-col items-center justify-center flex-1 text-muted-foreground gap-2 py-12">
              <Search className="w-8 h-8 opacity-30" />
              <p className="text-sm">
                {captureEnabled
                  ? t("requestLog.waitingForRequests", {
                      defaultValue: "等待请求中...",
                    })
                  : t("requestLog.noMatches", {
                      defaultValue: "无匹配的日志",
                    })}
              </p>
            </div>
          ) : (
            <div
              ref={scrollContainerRef}
              className="flex-1 overflow-y-auto"
            >
              <div
                style={{
                  height: virtualizer.getTotalSize(),
                  position: "relative",
                }}
              >
                {virtualizer.getVirtualItems().map((virtualRow) => {
                  const log = filteredLogs[virtualRow.index];
                  return (
                    <div
                      key={log.id}
                      data-index={virtualRow.index}
                      ref={virtualizer.measureElement}
                      style={{
                        position: "absolute",
                        top: 0,
                        left: 0,
                        width: "100%",
                        transform: `translateY(${virtualRow.start}px)`,
                      }}
                    >
                      <LogListItemRow
                        log={log}
                        isSelected={selectedLogId === log.id}
                        onSelect={onSelectRow}
                      />
                    </div>
                  );
                })}
              </div>
            </div>
          )}

        </div>

        {/* Right details */}
        {selectedLogId && (
          <div className="flex-1 flex flex-col min-h-0 min-w-0 overflow-hidden">
            <div className="flex items-center justify-between px-4 py-2 border-b">
              <h3 className="text-sm font-medium">
                {t("requestLog.detail", { defaultValue: "请求详情" })}
              </h3>
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6"
                onClick={() => {
                  setSelectedLogId(null);
                  setDetailEntry(null);
                }}
              >
                <X className="w-3.5 h-3.5" />
              </Button>
            </div>

            {detailLoading ? (
              <div className="flex items-center justify-center flex-1 text-sm text-muted-foreground">
                {t("common.loading", { defaultValue: "加载中..." })}
              </div>
            ) : detailEntry ? (
              <ScrollArea className="flex-1">
                <div className="p-4 space-y-4">
                  {/* Basic information */}
                  <div className="space-y-1.5">
                    <DetailRow label="App" value={detailEntry.app_type} />
                    <DetailRow
                      label="Provider"
                      value={detailEntry.provider_name}
                    />
                    <DetailRow label="Model" value={detailEntry.model} />
                    <DetailRow
                      label="Endpoint"
                      value={`${detailEntry.method} ${detailEntry.endpoint}`}
                    />
                    <DetailRow
                      label="Status"
                      value={detailEntry.status_code?.toString() ?? "pending"}
                    />
                    <DetailRow
                      label="Latency"
                      value={
                        detailEntry.latency_ms != null
                          ? `${detailEntry.latency_ms}ms`
                          : "N/A"
                      }
                    />
                    <DetailRow
                      label="Stream"
                      value={detailEntry.is_stream ? "Yes" : "No"}
                    />
                    {detailEntry.session_id && (
                      <DetailRow
                        label="Session"
                        value={detailEntry.session_id}
                      />
                    )}
                    <DetailRow
                      label="Time"
                      value={new Date(detailEntry.timestamp).toLocaleString()}
                    />
                  </div>

                  {/* System Prompt */}
                  {detailEntry.system_prompt && (
                    <div className="space-y-1.5">
                      <div className="flex items-center justify-between">
                        <h4 className="text-xs font-semibold text-blue-500 flex items-center gap-1.5">
                          <FileText className="w-3.5 h-3.5" />
                          System Prompt
                        </h4>
                        <div className="flex items-center gap-1">
                          <Button
                            variant="ghost"
                            size="sm"
                            className="h-6 text-[10px] gap-1"
                            onClick={() => setShowSystemPromptView(true)}
                          >
                            <FileText className="w-3 h-3" />
                            Format
                          </Button>
                          <Button
                            variant="ghost"
                            size="sm"
                            className="h-6 text-[10px] gap-1"
                            onClick={() =>
                              void handleCopy(detailEntry.system_prompt!)
                            }
                          >
                            <Copy className="w-3 h-3" />
                            {t("common.copy", { defaultValue: "复制" })}
                          </Button>
                        </div>
                      </div>
                      <pre className="text-xs bg-muted/50 rounded-lg p-3 whitespace-pre-wrap break-words max-h-32 overflow-y-auto font-mono leading-relaxed">
                        {detailEntry.system_prompt}
                      </pre>

                      {/* System Prompt dialog */}
                      <Dialog
                        open={showSystemPromptView}
                        onOpenChange={setShowSystemPromptView}
                      >
                        <DialogContent
                          zIndex="top"
                          overlayClassName="bg-black/60"
                          className="max-w-2xl max-h-[85vh] flex flex-col overflow-hidden"
                        >
                          <DialogHeader>
                            <DialogTitle className="flex items-center gap-2">
                              <FileText className="w-4 h-4 text-blue-500" />
                              System Prompt
                            </DialogTitle>
                            <DialogDescription>
                              {detailEntry.model} · {detailEntry.provider_name}
                            </DialogDescription>
                          </DialogHeader>
                          <div className="flex-1 min-h-0 overflow-y-auto px-6 py-4">
                            <pre className="text-xs whitespace-pre-wrap break-words font-mono leading-relaxed">
                              {detailEntry.system_prompt}
                            </pre>
                          </div>
                        </DialogContent>
                      </Dialog>
                    </div>
                  )}

                  {/* Request Body */}
                  <div className="space-y-1.5">
                    <div className="flex items-center justify-between">
                      <h4 className="text-xs font-semibold">Request Body</h4>
                      <div className="flex items-center gap-1">
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-6 text-[10px] gap-1"
                          onClick={() => setShowFormattedView(true)}
                        >
                          <MessageCircle className="w-3 h-3" />
                          Format
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-6 text-[10px] gap-1"
                          onClick={() => setShowRequestJsonView(true)}
                        >
                          <Braces className="w-3 h-3" />
                          JSON
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-6 text-[10px] gap-1"
                          onClick={() =>
                            void handleCopy(
                              JSON.stringify(detailEntry.request_body, null, 2),
                            )
                          }
                        >
                          <Copy className="w-3 h-3" />
                          {t("common.copy", { defaultValue: "复制" })}
                        </Button>
                      </div>
                    </div>
                    <pre className="text-xs bg-muted/50 rounded-lg p-3 whitespace-pre-wrap break-words max-h-32 overflow-y-auto font-mono leading-relaxed">
                      {truncateJsonPreview(detailEntry.request_body)}
                    </pre>
                  </div>

                  {/* Request Body JSON dialog */}
                  <Dialog
                    open={showRequestJsonView}
                    onOpenChange={(open) => {
                      setShowRequestJsonView(open);
                      if (!open) setRequestJsonExpanded(false);
                    }}
                  >
                    <DialogContent
                      zIndex="top"
                      overlayClassName="bg-black/60"
                      className="max-w-3xl max-h-[85vh] flex flex-col overflow-hidden"
                    >
                      <DialogHeader>
                        <DialogTitle className="flex items-center gap-2">
                          <Braces className="w-4 h-4" />
                          Request Body (JSON)
                          <Button
                            variant="ghost"
                            size="sm"
                            className="ml-auto h-7 px-2 text-xs"
                            onClick={() => setRequestJsonExpanded((v) => !v)}
                          >
                            {requestJsonExpanded ? (
                              <ChevronsDownUp className="w-3.5 h-3.5 mr-1" />
                            ) : (
                              <ChevronsUpDown className="w-3.5 h-3.5 mr-1" />
                            )}
                            {requestJsonExpanded ? t("requestLog.collapseAll", { defaultValue: "全部折叠" }) : t("requestLog.expandAll", { defaultValue: "全部展开" })}
                          </Button>
                        </DialogTitle>
                        <DialogDescription>
                          {detailEntry.model} · {detailEntry.provider_name}
                        </DialogDescription>
                      </DialogHeader>
                      <div className="flex-1 min-h-0 overflow-y-auto px-6 py-4 text-xs">
                        <JsonView
                          data={detailEntry.request_body as object}
                          shouldExpandNode={requestJsonExpanded ? allExpanded : collapseAllNested}
                          style={jsonViewStyle}
                        />
                      </div>
                    </DialogContent>
                  </Dialog>

                  {/* Formatted conversation dialog */}
                  <Dialog
                    open={showFormattedView}
                    onOpenChange={setShowFormattedView}
                  >
                    <DialogContent
                      zIndex="top"
                      overlayClassName="bg-black/60"
                      className="max-w-2xl max-h-[85vh] flex flex-col overflow-hidden"
                    >
                      <DialogHeader>
                        <DialogTitle className="flex items-center gap-2">
                          <MessageCircle className="w-4 h-4" />
                          对话历史
                        </DialogTitle>
                        <DialogDescription>
                          {detailEntry.model} · {detailEntry.provider_name}
                        </DialogDescription>
                      </DialogHeader>
                      <div className="flex-1 min-h-0 overflow-y-auto px-6 py-4">
                        <FormattedMessagesView
                          requestBody={detailEntry.request_body}
                        />
                      </div>
                    </DialogContent>
                  </Dialog>

                  {/* Response Body */}
                  <div className="space-y-1.5">
                    <div className="flex items-center justify-between">
                      <h4 className="text-xs font-semibold">Response Body</h4>
                      {detailEntry.response_body != null && (
                        <div className="flex items-center gap-1">
                          <Button
                            variant="ghost"
                            size="sm"
                            className="h-6 text-[10px] gap-1"
                            onClick={() => setShowResponseFormatView(true)}
                          >
                            <MessageCircle className="w-3 h-3" />
                            Format
                          </Button>
                          <Button
                            variant="ghost"
                            size="sm"
                            className="h-6 text-[10px] gap-1"
                            onClick={() => setShowResponseJsonView(true)}
                          >
                            <Braces className="w-3 h-3" />
                            JSON
                          </Button>
                          <Button
                            variant="ghost"
                            size="sm"
                            className="h-6 text-[10px] gap-1"
                            onClick={() =>
                              void handleCopy(
                                JSON.stringify(
                                  detailEntry.response_body,
                                  null,
                                  2,
                                ),
                              )
                            }
                          >
                            <Copy className="w-3 h-3" />
                            {t("common.copy", { defaultValue: "复制" })}
                          </Button>
                        </div>
                      )}
                    </div>
                    {detailEntry.response_body == null ? (
                      <p className="text-xs text-muted-foreground italic">
                        暂无响应数据（流式响应完成后回填）
                      </p>
                    ) : (
                      <pre className="text-xs bg-muted/50 rounded-lg p-3 whitespace-pre-wrap break-words max-h-32 overflow-y-auto font-mono leading-relaxed">
                        {truncateJsonPreview(detailEntry.response_body)}
                      </pre>
                    )}

                    {/* Response Body formatted dialog */}
                    {detailEntry.response_body != null && (
                      <Dialog
                        open={showResponseFormatView}
                        onOpenChange={setShowResponseFormatView}
                      >
                        <DialogContent
                          zIndex="top"
                          overlayClassName="bg-black/60"
                          className="max-w-2xl max-h-[85vh] flex flex-col overflow-hidden"
                        >
                          <DialogHeader>
                            <DialogTitle className="flex items-center gap-2">
                              <Bot className="w-4 h-4" />
                              响应内容
                            </DialogTitle>
                            <DialogDescription>
                              {detailEntry.model} ·{" "}
                              {detailEntry.is_stream
                                ? "SSE Stream"
                                : "Non-Stream"}
                            </DialogDescription>
                          </DialogHeader>
                          <div className="flex-1 min-h-0 overflow-y-auto px-6 py-4">
                            <FormattedResponseView
                              responseBody={detailEntry.response_body}
                              isStream={detailEntry.is_stream}
                            />
                          </div>
                        </DialogContent>
                      </Dialog>
                    )}

                    {/* Response Body JSON dialog */}
                    {detailEntry.response_body != null && (
                      <Dialog
                        open={showResponseJsonView}
                        onOpenChange={(open) => {
                          setShowResponseJsonView(open);
                          if (!open) setResponseJsonExpanded(false);
                        }}
                      >
                        <DialogContent
                          zIndex="top"
                          overlayClassName="bg-black/60"
                          className="max-w-3xl max-h-[85vh] flex flex-col overflow-hidden"
                        >
                          <DialogHeader>
                            <DialogTitle className="flex items-center gap-2">
                              <Braces className="w-4 h-4" />
                              Response Body (JSON)
                              <Button
                                variant="ghost"
                                size="sm"
                                className="ml-auto h-7 px-2 text-xs"
                                onClick={() => setResponseJsonExpanded((v) => !v)}
                              >
                                {responseJsonExpanded ? (
                                  <ChevronsDownUp className="w-3.5 h-3.5 mr-1" />
                                ) : (
                                  <ChevronsUpDown className="w-3.5 h-3.5 mr-1" />
                                )}
                                {responseJsonExpanded ? t("requestLog.collapseAll", { defaultValue: "全部折叠" }) : t("requestLog.expandAll", { defaultValue: "全部展开" })}
                              </Button>
                            </DialogTitle>
                            <DialogDescription>
                              {detailEntry.model} · {detailEntry.provider_name}
                            </DialogDescription>
                          </DialogHeader>
                          <div className="flex-1 min-h-0 overflow-y-auto px-6 py-4 text-xs">
                            <JsonView
                              data={detailEntry.response_body as object}
                              shouldExpandNode={responseJsonExpanded ? allExpanded : collapseAllNested}
                              style={jsonViewStyle}
                            />
                          </div>
                        </DialogContent>
                      </Dialog>
                    )}
                  </div>
                </div>
              </ScrollArea>
            ) : (
              <div className="flex items-center justify-center flex-1 text-sm text-muted-foreground">
                {t("requestLog.detailNotFound", {
                  defaultValue: "日志详情未找到",
                })}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function DetailRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline gap-2">
      <span className="text-[10px] text-muted-foreground w-16 flex-shrink-0 text-right">
        {label}
      </span>
      <span className="text-xs font-mono break-all">{value}</span>
    </div>
  );
}

// ============================================================================
// Formatted conversation view
// ============================================================================

interface ParsedMessage {
  /** Display role, tool_result will be corrected to "tool" */
  role: string;
  contentParts: { type: string; text: string }[];
  toolCalls?: { id: string; name: string; input: unknown }[];
  /** Tool ID for tool_result */
  toolUseId?: string;
}

function parseMessages(requestBody: unknown): ParsedMessage[] | null {
  if (
    requestBody == null ||
    typeof requestBody !== "object" ||
    !("messages" in requestBody)
  )
    return null;
  const messages = (requestBody as Record<string, unknown>).messages;
  if (!Array.isArray(messages)) return null;

  const result: ParsedMessage[] = [];

  for (const msg of messages) {
    const m = msg as Record<string, unknown>;
    const rawRole = typeof m.role === "string" ? m.role : "unknown";

    if (typeof m.content === "string") {
      result.push({
        role: rawRole,
        contentParts: [{ type: "text", text: m.content }],
      });
      continue;
    }

    if (!Array.isArray(m.content)) {
      result.push({ role: rawRole, contentParts: [] });
      continue;
    }

    // Array content: split tool_result into separate messages, group the rest together
    const normalParts: { type: string; text: string }[] = [];
    const toolCalls: { id: string; name: string; input: unknown }[] = [];
    const toolResults: { toolUseId: string; content: string }[] = [];

    for (const part of m.content) {
      if (!part || typeof part !== "object") continue;
      const p = part as Record<string, unknown>;
      const partType = typeof p.type === "string" ? p.type : "unknown";

      if (partType === "tool_result") {
        // tool_result content may be a string or nested array
        let resultText = "";
        if (typeof p.content === "string") {
          resultText = p.content;
        } else if (Array.isArray(p.content)) {
          resultText = (p.content as Record<string, unknown>[])
            .filter((c) => c.type === "text" && typeof c.text === "string")
            .map((c) => c.text as string)
            .join("\n");
        }
        const toolUseId =
          typeof p.tool_use_id === "string" ? p.tool_use_id : "";
        toolResults.push({ toolUseId, content: resultText });
      } else if (partType === "tool_use" && typeof p.name === "string") {
        const toolId = typeof p.id === "string" ? p.id : "";
        toolCalls.push({ id: toolId, name: p.name, input: p.input });
      } else {
        const partText = typeof p.text === "string" ? p.text : "";
        normalParts.push({ type: partType, text: partText });
      }
    }

    // Filter out empty text parts to avoid rendering blank bubbles
    const nonEmptyParts = normalParts.filter(
      (p) => p.type !== "text" || p.text.trim().length > 0,
    );

    // Split each text part into separate messages, tool calls as a single message
    if (nonEmptyParts.length > 1) {
      for (const part of nonEmptyParts) {
        result.push({ role: rawRole, contentParts: [part] });
      }
      if (toolCalls.length > 0) {
        result.push({ role: rawRole, contentParts: [], toolCalls });
      }
    } else if (nonEmptyParts.length > 0 || toolCalls.length > 0) {
      result.push({
        role: rawRole,
        contentParts: nonEmptyParts,
        toolCalls: toolCalls.length > 0 ? toolCalls : undefined,
      });
    }

    // Split tool_result into separate tool messages
    for (const tr of toolResults) {
      result.push({
        role: "tool",
        contentParts: tr.content
          ? [{ type: "tool_result", text: tr.content }]
          : [],
        toolUseId: tr.toolUseId,
      });
    }

    // If the entire message only has tool_result and no other content, don't miss the empty case
    if (
      normalParts.length === 0 &&
      toolCalls.length === 0 &&
      toolResults.length === 0
    ) {
      result.push({ role: rawRole, contentParts: [] });
    }
  }

  return result;
}

const ROLE_CONFIG: Record<
  string,
  { icon: typeof User; label: string; color: string; bubbleColor: string }
> = {
  user: {
    icon: User,
    label: "User",
    color: "text-blue-600 dark:text-blue-400",
    bubbleColor: "bg-blue-500 dark:bg-blue-600 text-white",
  },
  assistant: {
    icon: Bot,
    label: "Assistant",
    color: "text-purple-600 dark:text-purple-400",
    bubbleColor: "bg-muted border border-border",
  },
  tool: {
    icon: Wrench,
    label: "Tool",
    color: "text-amber-600 dark:text-amber-400",
    bubbleColor:
      "bg-amber-50 dark:bg-amber-950/30 border border-amber-200 dark:border-amber-800",
  },
  system: {
    icon: Settings,
    label: "System",
    color: "text-emerald-600 dark:text-emerald-400",
    bubbleColor:
      "bg-emerald-50 dark:bg-emerald-950/30 border border-emerald-200 dark:border-emerald-800",
  },
};

const DEFAULT_ROLE_CONFIG = {
  icon: MessageCircle,
  label: "Unknown",
  color: "text-muted-foreground",
  bubbleColor: "bg-muted border border-border",
};

function isSessionReclaimed(requestBody: unknown): boolean {
  if (!requestBody || typeof requestBody !== "object") return false;
  const body = requestBody as Record<string, unknown>;
  return !("messages" in body) && Object.keys(body).length > 0;
}

function FormattedMessagesView({ requestBody }: { requestBody: unknown }) {
  const messages = parseMessages(requestBody);
  if (!messages || messages.length === 0) {
    if (isSessionReclaimed(requestBody)) {
      return (
        <p className="text-xs text-muted-foreground italic">
          消息内容已被回收（同 Session 有更新的请求，旧请求的 messages 已自动释放以节省内存）
        </p>
      );
    }
    return (
      <p className="text-xs text-muted-foreground italic">无 messages 数据</p>
    );
  }

  return (
    <div className="space-y-3">
      {messages.map((msg, index) => {
        const config = ROLE_CONFIG[msg.role] ?? DEFAULT_ROLE_CONFIG;
        const Icon = config.icon;
        const isUser = msg.role === "user";

        return (
          <div
            key={index}
            className={cn(
              "flex gap-2",
              isUser ? "flex-row-reverse" : "flex-row",
            )}
          >
            {/* Avatar */}
            <div
              className={cn(
                "w-6 h-6 rounded-full flex items-center justify-center shrink-0 mt-0.5",
                isUser
                  ? "bg-blue-500 text-white"
                  : "bg-muted border border-border",
              )}
            >
              <Icon className="w-3.5 h-3.5" />
            </div>

            {/* Bubble */}
            <div
              className={cn(
                "max-w-[80%] min-w-0",
                isUser ? "items-end" : "items-start",
              )}
            >
              <div
                className={cn(
                  "flex items-center gap-1.5 mb-0.5",
                  isUser ? "justify-end" : "justify-start",
                )}
              >
                <span className={cn("text-[10px] font-semibold", config.color)}>
                  {config.label}
                </span>
                <span className="text-[9px] text-muted-foreground">
                  #{index + 1}
                </span>
                {msg.toolUseId && (
                  <span className="text-[9px] text-muted-foreground font-mono truncate max-w-40">
                    {msg.toolUseId}
                  </span>
                )}
              </div>

              <div className={cn("rounded-xl px-3 py-2", config.bubbleColor)}>
                {/* Text content (text and tool_result) */}
                {msg.contentParts
                  .filter(
                    (p) =>
                      (p.type === "text" || p.type === "tool_result") && p.text,
                  )
                  .map((part, partIndex) => (
                    <pre
                      key={partIndex}
                      className="text-xs whitespace-pre-wrap break-words font-mono leading-relaxed max-h-60 overflow-y-auto"
                    >
                      {part.text}
                    </pre>
                  ))}

                {/* Non-text content indicators */}
                {msg.contentParts
                  .filter(
                    (p) =>
                      p.type !== "text" &&
                      p.type !== "tool_use" &&
                      p.type !== "tool_result",
                  )
                  .map((part, partIndex) => (
                    <Badge
                      key={partIndex}
                      variant="outline"
                      className="text-[10px] mt-1 mr-1"
                    >
                      {part.type}
                    </Badge>
                  ))}

                {/* Tool calls */}
                {msg.toolCalls?.map((tool, toolIndex) => (
                  <div key={toolIndex} className="mt-1">
                    <div className="flex items-center gap-1.5">
                      <Wrench className="w-3 h-3 text-amber-500 shrink-0" />
                      <Badge
                        variant="secondary"
                        className="text-[10px] font-mono"
                      >
                        {tool.name}
                      </Badge>
                      {tool.id && (
                        <span className="text-[9px] text-muted-foreground font-mono truncate max-w-48">
                          {tool.id}
                        </span>
                      )}
                    </div>
                    {tool.input != null && (
                      <pre className="text-[11px] mt-1 whitespace-pre-wrap break-words font-mono leading-relaxed max-h-40 overflow-y-auto bg-black/5 dark:bg-white/5 rounded p-2">
                        {typeof tool.input === "string"
                          ? tool.input
                          : JSON.stringify(tool.input, null, 2)}
                      </pre>
                    )}
                  </div>
                ))}

                {/* Tool with no content */}
                {msg.role === "tool" && msg.contentParts.length === 0 && (
                  <span className="text-[10px] text-muted-foreground italic">
                    （工具返回结果）
                  </span>
                )}

                {/* Completely empty content */}
                {msg.contentParts.length === 0 &&
                  !msg.toolCalls &&
                  msg.role !== "tool" && (
                    <span className="text-[10px] text-muted-foreground italic">
                      （空）
                    </span>
                  )}
              </div>
            </div>
          </div>
        );
      })}
    </div>
  );
}

// ============================================================================
// SSE streaming response merge & formatted display
// ============================================================================

interface MergedContentBlock {
  index: number;
  type: string;
  text: string;
  toolId?: string;
  toolName?: string;
  toolInput?: string;
}

interface MergedResponse {
  model: string;
  messageId: string;
  role: string;
  stopReason: string | null;
  blocks: MergedContentBlock[];
  inputTokens: number;
  outputTokens: number;
  thinkingTokens: number;
  cacheCreationTokens: number;
  cacheReadTokens: number;
}

function mergeSSEEvents(events: unknown[]): MergedResponse {
  const result: MergedResponse = {
    model: "",
    messageId: "",
    role: "assistant",
    stopReason: null,
    blocks: [],
    inputTokens: 0,
    outputTokens: 0,
    thinkingTokens: 0,
    cacheCreationTokens: 0,
    cacheReadTokens: 0,
  };
  const blockMap = new Map<number, MergedContentBlock>();

  for (const event of events) {
    if (!event || typeof event !== "object") continue;
    const e = event as Record<string, unknown>;
    const eventType = e.type as string;

    if (eventType === "message_start") {
      const msg = e.message as Record<string, unknown> | undefined;
      if (msg) {
        result.model = (msg.model as string) ?? "";
        result.messageId = (msg.id as string) ?? "";
        result.role = (msg.role as string) ?? "assistant";
        const usage = msg.usage as Record<string, unknown> | undefined;
        if (usage) {
          result.inputTokens = (usage.input_tokens as number) ?? 0;
          result.cacheCreationTokens =
            (usage.cache_creation_input_tokens as number) ?? 0;
          result.cacheReadTokens =
            (usage.cache_read_input_tokens as number) ?? 0;
        }
      }
    } else if (eventType === "content_block_start") {
      const index = e.index as number;
      const block = e.content_block as Record<string, unknown>;
      if (block) {
        const blockType = (block.type as string) ?? "unknown";
        const merged: MergedContentBlock = { index, type: blockType, text: "" };
        if (blockType === "tool_use") {
          merged.toolId = (block.id as string) ?? "";
          merged.toolName = (block.name as string) ?? "";
          merged.toolInput = "";
        }
        blockMap.set(index, merged);
      }
    } else if (eventType === "content_block_delta") {
      const index = e.index as number;
      const delta = e.delta as Record<string, unknown>;
      if (!delta) continue;
      const block = blockMap.get(index);
      if (!block) continue;
      const deltaType = delta.type as string;
      if (deltaType === "text_delta") {
        block.text += (delta.text as string) ?? "";
      } else if (deltaType === "thinking_delta") {
        block.text += (delta.thinking as string) ?? "";
      } else if (deltaType === "input_json_delta") {
        block.toolInput =
          (block.toolInput ?? "") + ((delta.partial_json as string) ?? "");
      }
    } else if (eventType === "message_delta") {
      const delta = e.delta as Record<string, unknown> | undefined;
      if (delta) result.stopReason = (delta.stop_reason as string) ?? null;
      const usage = e.usage as Record<string, unknown> | undefined;
      if (usage) {
        result.outputTokens = (usage.output_tokens as number) ?? 0;
        const details = usage.output_tokens_details as
          | Record<string, unknown>
          | undefined;
        if (details)
          result.thinkingTokens = (details.thinking_tokens as number) ?? 0;
      }
    }
  }

  result.blocks = Array.from(blockMap.values()).sort(
    (a, b) => a.index - b.index,
  );
  for (const block of result.blocks) {
    if (block.type === "tool_use" && block.toolInput) {
      try {
        block.toolInput = JSON.stringify(JSON.parse(block.toolInput), null, 2);
      } catch {
        /* keep raw */
      }
    }
  }
  return result;
}

function formatNonStreamResponse(body: unknown): MergedResponse {
  const result: MergedResponse = {
    model: "",
    messageId: "",
    role: "assistant",
    stopReason: null,
    blocks: [],
    inputTokens: 0,
    outputTokens: 0,
    thinkingTokens: 0,
    cacheCreationTokens: 0,
    cacheReadTokens: 0,
  };
  if (!body || typeof body !== "object") return result;
  const resp = body as Record<string, unknown>;
  result.model = (resp.model as string) ?? "";
  result.messageId = (resp.id as string) ?? "";
  result.role = (resp.role as string) ?? "assistant";
  result.stopReason = (resp.stop_reason as string) ?? null;
  const usage = resp.usage as Record<string, unknown> | undefined;
  if (usage) {
    result.inputTokens = (usage.input_tokens as number) ?? 0;
    result.outputTokens = (usage.output_tokens as number) ?? 0;
    result.cacheCreationTokens =
      (usage.cache_creation_input_tokens as number) ?? 0;
    result.cacheReadTokens = (usage.cache_read_input_tokens as number) ?? 0;
  }
  const content = resp.content;
  if (Array.isArray(content)) {
    content.forEach((item, index) => {
      const c = item as Record<string, unknown>;
      const blockType = (c.type as string) ?? "unknown";
      if (blockType === "text") {
        result.blocks.push({
          index,
          type: "text",
          text: (c.text as string) ?? "",
        });
      } else if (blockType === "thinking") {
        result.blocks.push({
          index,
          type: "thinking",
          text: (c.thinking as string) ?? "",
        });
      } else if (blockType === "tool_use") {
        result.blocks.push({
          index,
          type: "tool_use",
          text: "",
          toolId: (c.id as string) ?? "",
          toolName: (c.name as string) ?? "",
          toolInput: c.input ? JSON.stringify(c.input, null, 2) : "",
        });
      } else {
        result.blocks.push({
          index,
          type: blockType,
          text: JSON.stringify(c, null, 2),
        });
      }
    });
  }
  return result;
}

function FormattedResponseView({
  responseBody,
  isStream,
}: {
  responseBody: unknown;
  isStream: boolean;
}) {
  const merged = useMemo(() => {
    if (isStream && Array.isArray(responseBody))
      return mergeSSEEvents(responseBody);
    return formatNonStreamResponse(responseBody);
  }, [responseBody, isStream]);

  return (
    <div className="space-y-4">
      {/* Metadata */}
      <div className="flex flex-wrap gap-2">
        {merged.model && (
          <Badge variant="secondary" className="text-[10px]">
            {merged.model}
          </Badge>
        )}
        {merged.messageId && (
          <Badge variant="outline" className="text-[10px] font-mono">
            {merged.messageId}
          </Badge>
        )}
        {merged.stopReason && (
          <Badge variant="outline" className="text-[10px]">
            stop: {merged.stopReason}
          </Badge>
        )}
      </div>

      {/* Token statistics */}
      <div className="flex flex-wrap gap-3 text-[10px] text-muted-foreground">
        <span>
          Input:{" "}
          <strong className="text-foreground">
            {merged.inputTokens.toLocaleString()}
          </strong>
        </span>
        <span>
          Output:{" "}
          <strong className="text-foreground">
            {merged.outputTokens.toLocaleString()}
          </strong>
        </span>
        {merged.thinkingTokens > 0 && (
          <span>
            Thinking:{" "}
            <strong className="text-foreground">
              {merged.thinkingTokens.toLocaleString()}
            </strong>
          </span>
        )}
        {merged.cacheCreationTokens > 0 && (
          <span>
            Cache Write:{" "}
            <strong className="text-foreground">
              {merged.cacheCreationTokens.toLocaleString()}
            </strong>
          </span>
        )}
        {merged.cacheReadTokens > 0 && (
          <span>
            Cache Read:{" "}
            <strong className="text-foreground">
              {merged.cacheReadTokens.toLocaleString()}
            </strong>
          </span>
        )}
      </div>

      {/* Content blocks */}
      {merged.blocks.length === 0 ? (
        <p className="text-xs text-muted-foreground italic">无内容块</p>
      ) : (
        <div className="space-y-3">
          {merged.blocks.map((block) => (
            <div key={block.index}>
              {block.type === "thinking" && (
                <div className="rounded-lg border border-amber-200 dark:border-amber-800 bg-amber-50/50 dark:bg-amber-950/20">
                  <div className="flex items-center gap-1.5 px-3 py-1.5 border-b border-amber-200 dark:border-amber-800">
                    <Settings className="w-3 h-3 text-amber-500" />
                    <span className="text-[10px] font-semibold text-amber-600 dark:text-amber-400">
                      Thinking
                    </span>
                  </div>
                  <pre className="text-xs px-3 py-2 whitespace-pre-wrap break-words font-mono leading-relaxed max-h-60 overflow-y-auto">
                    {block.text}
                  </pre>
                </div>
              )}
              {block.type === "text" && (
                <div className="rounded-lg border border-border bg-muted/30">
                  <div className="flex items-center gap-1.5 px-3 py-1.5 border-b border-border">
                    <MessageCircle className="w-3 h-3 text-purple-500" />
                    <span className="text-[10px] font-semibold text-purple-600 dark:text-purple-400">
                      Text
                    </span>
                  </div>
                  <pre className="text-xs px-3 py-2 whitespace-pre-wrap break-words font-mono leading-relaxed max-h-80 overflow-y-auto">
                    {block.text}
                  </pre>
                </div>
              )}
              {block.type === "tool_use" && (
                <div className="rounded-lg border border-blue-200 dark:border-blue-800 bg-blue-50/50 dark:bg-blue-950/20">
                  <div className="flex items-center gap-1.5 px-3 py-1.5 border-b border-blue-200 dark:border-blue-800">
                    <Wrench className="w-3 h-3 text-blue-500" />
                    <Badge
                      variant="secondary"
                      className="text-[10px] font-mono"
                    >
                      {block.toolName}
                    </Badge>
                    {block.toolId && (
                      <span className="text-[9px] text-muted-foreground font-mono truncate max-w-60">
                        {block.toolId}
                      </span>
                    )}
                  </div>
                  {block.toolInput && (
                    <pre className="text-xs px-3 py-2 whitespace-pre-wrap break-words font-mono leading-relaxed max-h-40 overflow-y-auto">
                      {block.toolInput}
                    </pre>
                  )}
                </div>
              )}
              {block.type !== "thinking" &&
                block.type !== "text" &&
                block.type !== "tool_use" && (
                  <div className="rounded-lg border border-border bg-muted/30">
                    <div className="flex items-center gap-1.5 px-3 py-1.5 border-b border-border">
                      <span className="text-[10px] font-semibold text-muted-foreground">
                        {block.type}
                      </span>
                    </div>
                    <pre className="text-xs px-3 py-2 whitespace-pre-wrap break-words font-mono leading-relaxed max-h-40 overflow-y-auto">
                      {block.text}
                    </pre>
                  </div>
                )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
