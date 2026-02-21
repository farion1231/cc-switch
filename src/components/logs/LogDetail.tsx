import { useTranslation } from "react-i18next";
import { useRequestDetail } from "@/lib/query/usage";
import { Loader2, Copy, Check, Clock, Zap, FileText, Hash, Server, Gauge, Timer, GaugeCircle, Percent } from "lucide-react";
import { Button } from "@/components/ui/button";
import { fmtUsd } from "../usage/format";
import { useState } from "react";

interface LogDetailProps {
  requestId: string;
}

export function LogDetail({ requestId }: LogDetailProps) {
  const { t } = useTranslation();
  const { data: log, isLoading } = useRequestDetail(requestId);
  const [copiedField, setCopiedField] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<"request" | "response">("request");

  const copyToClipboard = async (text: string, field: string) => {
    await navigator.clipboard.writeText(text);
    setCopiedField(field);
    setTimeout(() => setCopiedField(null), 2000);
  };

  if (isLoading) {
    return (
      <div className="py-12 flex justify-center">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground/50" />
      </div>
    );
  }

  if (!log) {
    return (
      <div className="py-12 text-center text-muted-foreground">
        {t("logs.notFound")}
      </div>
    );
  }

  // Format JSON
  const formatBody = (body?: string) => {
    if (!body) return null;
    try {
      const obj = JSON.parse(body);
      return JSON.stringify(obj, null, 2);
    } catch {
      return body;
    }
  };

  const requestBody = formatBody(log.requestBody);
  const responseBody = formatBody(log.responseBody);

  const currentBody = activeTab === "request" ? requestBody : responseBody;

  // Calculate TPOT (Time Per Output Token) and TPS (Tokens Per Second)
  // TPOT: output time / outputTokens, excluding first token wait time
  const outputTimeMs = log.latencyMs && log.firstTokenMs
    ? log.latencyMs - log.firstTokenMs
    : log.latencyMs || 0;
  const tpot = log.outputTokens > 0 && outputTimeMs > 0
    ? (outputTimeMs / log.outputTokens).toFixed(2)
    : null;
  const tps = log.outputTokens > 0 && outputTimeMs > 0
    ? (log.outputTokens / (outputTimeMs / 1000)).toFixed(2)
    : null;

  // Cache hit rate: cache_read_tokens / (input + output + cache_read) * 100%
  const totalTokens = log.inputTokens + log.outputTokens + (log.cacheReadTokens || 0);
  const cacheHitRate = totalTokens > 0
    ? ((log.cacheReadTokens || 0) / totalTokens * 100).toFixed(1)
    : null;

  // Protocol/app type display
  const getAppTypeLabel = (appType?: string) => {
    switch (appType?.toLowerCase()) {
      case "claude": return "Claude";
      case "codex": return "Codex";
      case "gemini": return "Gemini";
      case "opencode": return "OpenCode";
      case "openclaw": return "OpenClaw";
      default: return appType || "Unknown";
    }
  };

  return (
    <div className="space-y-3 pt-2">
      {/* Top info: status + time */}
      <div className="flex items-center justify-between">
        <span
          className={`px-3 py-1.5 rounded-lg text-sm font-medium ${
            log.statusCode >= 200 && log.statusCode < 300
              ? "bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-400"
              : log.statusCode >= 400
                ? "bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-400"
                : "bg-yellow-100 text-yellow-700 dark:bg-yellow-900/40 dark:text-yellow-400"
          }`}
        >
          {log.statusCode}
        </span>
        <span className="text-sm text-muted-foreground">
          {new Date(log.createdAt * 1000).toLocaleString()}
        </span>
      </div>

      {/* Basic info: protocol | provider | response mode | model */}
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 text-sm">
        <div className="flex items-center gap-1.5">
          <Hash className="h-4 w-4 text-purple-500" />
          <span className="text-muted-foreground">{t("logs.protocol")}:</span>
          <span className="font-medium">{getAppTypeLabel(log.appType)}</span>
        </div>
        <div className="flex items-center gap-1.5">
          <Server className="h-4 w-4 text-blue-500" />
          <span className="text-muted-foreground">{t("logs.provider")}:</span>
          <span>{log.providerName || log.providerId}</span>
        </div>
        <div className="flex items-center gap-1.5">
          <div className={`h-4 w-4 rounded-full ${log.isStreaming ? "bg-cyan-500" : "bg-orange-500"}`} />
          <span className="text-muted-foreground">{t("logs.responseMode")}:</span>
          <span className={log.isStreaming ? "text-cyan-600 dark:text-cyan-400" : "text-orange-600 dark:text-orange-400"}>
            {log.isStreaming ? t("usage.stream") : t("usage.nonStream")}
          </span>
        </div>
        <div className="flex items-center gap-1.5">
          <div className="h-4 w-4 rounded-full bg-green-500" />
          <span className="text-muted-foreground">{t("logs.model")}:</span>
          <span className="font-mono font-medium">{log.model}</span>
        </div>
      </div>

      {/* Token and cost row */}
      <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
        <div className="flex items-center gap-2 px-3 py-2 bg-blue-50 dark:bg-blue-900/20 rounded-lg">
          <Zap className="h-4 w-4 text-blue-500 shrink-0" />
          <div className="min-w-0">
            <div className="text-xs text-muted-foreground">{t("logs.input")}</div>
            <div className="font-semibold text-sm truncate">{log.inputTokens.toLocaleString()}</div>
            <div className="text-xs text-muted-foreground">{fmtUsd(log.inputCostUsd, 4)}</div>
          </div>
        </div>
        <div className="flex items-center gap-2 px-3 py-2 bg-green-50 dark:bg-green-900/20 rounded-lg">
          <FileText className="h-4 w-4 text-green-500 shrink-0" />
          <div className="min-w-0">
            <div className="text-xs text-muted-foreground">{t("logs.output")}</div>
            <div className="font-semibold text-sm truncate">{log.outputTokens.toLocaleString()}</div>
            <div className="text-xs text-muted-foreground">{fmtUsd(log.outputCostUsd, 4)}</div>
          </div>
        </div>
        <div className="flex items-center gap-2 px-3 py-2 bg-amber-50 dark:bg-amber-900/20 rounded-lg">
          <Percent className="h-4 w-4 text-amber-500 shrink-0" />
          <div className="min-w-0">
            <div className="text-xs text-muted-foreground">{t("logs.cacheHitRate")}</div>
            <div className="font-semibold text-sm">{cacheHitRate ? `${cacheHitRate}%` : '-'}</div>
          </div>
        </div>
        <div className="flex items-center gap-2 px-3 py-2 bg-orange-50 dark:bg-orange-900/20 rounded-lg">
          <div className="h-4 w-4 rounded-full bg-orange-500 shrink-0" />
          <div className="min-w-0">
            <div className="text-xs text-muted-foreground">{t("logs.totalCost")}</div>
            <div className="font-semibold text-sm">{fmtUsd(log.totalCostUsd, 4)}</div>
          </div>
        </div>
      </div>

      {/* Performance metrics row */}
      <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
        <div className="flex items-center gap-2 px-3 py-2 bg-indigo-50 dark:bg-indigo-900/20 rounded-lg">
          <GaugeCircle className="h-4 w-4 text-indigo-500 shrink-0" />
          <div className="min-w-0">
            <div className="text-xs text-muted-foreground">{t("logs.tpsLabel")}</div>
            <div className="font-semibold text-sm">{tps ? tps : '-'}</div>
          </div>
        </div>
        <div className="flex items-center gap-2 px-3 py-2 bg-teal-50 dark:bg-teal-900/20 rounded-lg">
          <Gauge className="h-4 w-4 text-teal-500 shrink-0" />
          <div className="min-w-0">
            <div className="text-xs text-muted-foreground">{t("logs.tpot")}</div>
            <div className="font-semibold text-sm">{tpot ? `${tpot}ms` : '-'}</div>
          </div>
        </div>
        <div className="flex items-center gap-2 px-3 py-2 bg-cyan-50 dark:bg-cyan-900/20 rounded-lg">
          <Timer className="h-4 w-4 text-cyan-500 shrink-0" />
          <div className="min-w-0">
            <div className="text-xs text-muted-foreground">{t("logs.ttft")}</div>
            <div className="font-semibold text-sm">{log.firstTokenMs ? `${log.firstTokenMs}ms` : '-'}</div>
          </div>
        </div>
        <div className="flex items-center gap-2 px-3 py-2 bg-rose-50 dark:bg-rose-900/20 rounded-lg">
          <Clock className="h-4 w-4 text-rose-500 shrink-0" />
          <div className="min-w-0">
            <div className="text-xs text-muted-foreground">{t("logs.latency")}</div>
            <div className="font-semibold text-sm">{log.latencyMs ? `${log.latencyMs}ms` : '-'}</div>
          </div>
        </div>
      </div>

      {/* Error Message */}
      {log.errorMessage && (
        <div className="p-3 bg-red-50 dark:bg-red-900/20 rounded-lg border border-red-200 dark:border-red-800 text-sm">
          <div className="text-red-600 dark:text-red-400 font-medium mb-1">{t("logs.error")}</div>
          <div className="text-red-800 dark:text-red-300 font-mono text-xs whitespace-pre-wrap">
            {log.errorMessage}
          </div>
        </div>
      )}

      {/* Request/Response Body */}
      <div className="border rounded-lg overflow-hidden">
        <div className="flex items-center justify-between bg-muted/50 px-3 py-2 border-b">
          <div className="flex gap-1">
            <button
              onClick={() => setActiveTab("request")}
              className={`px-3 py-1 text-sm rounded ${activeTab === "request" ? "bg-background shadow" : "text-muted-foreground"}`}
            >
              {t("logs.requestBody")}
            </button>
            <button
              onClick={() => setActiveTab("response")}
              className={`px-3 py-1 text-sm rounded ${activeTab === "response" ? "bg-background shadow" : "text-muted-foreground"}`}
            >
              {t("logs.responseBody")}
            </button>
          </div>
          {currentBody && (
            <Button
              variant="ghost"
              size="sm"
              className="h-7 text-xs"
              onClick={() => copyToClipboard(currentBody, activeTab)}
            >
              {copiedField === activeTab ? (
                <>
                  <Check className="h-3 w-3 mr-1" />
                  {t("logs.copied")}
                </>
              ) : (
                <>
                  <Copy className="h-3 w-3 mr-1" />
                  {t("logs.copy")}
                </>
              )}
            </Button>
          )}
        </div>
        <div className="bg-muted/20 max-h-[400px] overflow-auto">
          {currentBody ? (
            <pre className="p-3 text-xs font-mono whitespace-pre-wrap">
              {currentBody}
            </pre>
          ) : (
            <div className="p-6 text-center text-muted-foreground text-sm">
              {activeTab === "request"
                ? t("logs.noRequestBody")
                : t("logs.noResponseBody")}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
