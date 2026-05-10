import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ChevronDown,
  ChevronUp,
  Copy,
  Loader2,
  StopCircle,
  CheckCircle2,
  XCircle,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import type {
  InstallLogLine,
  InstallLogStatus,
} from "@/hooks/useInstallLogStream";

interface InstallLogPanelProps {
  status: InstallLogStatus;
  lines: InstallLogLine[];
  onCancel?: () => void;
  isCancelling?: boolean;
  /** 默认展开行为：success 折叠、其它展开。可被外层覆盖。 */
  defaultExpanded?: boolean;
}

const streamColor: Record<InstallLogLine["stream"], string> = {
  stdout: "text-foreground/90",
  stderr: "text-red-500 dark:text-red-400",
  info: "text-blue-600 dark:text-blue-400",
  error: "text-red-600 dark:text-red-400 font-medium",
};

export function InstallLogPanel({
  status,
  lines,
  onCancel,
  isCancelling,
  defaultExpanded,
}: InstallLogPanelProps) {
  const { t } = useTranslation();
  const scrollRef = useRef<HTMLDivElement>(null);
  const [copied, setCopied] = useState(false);

  // 控制折叠：running/failed/cancelled 默认展开，success 默认折叠；用户切换后以用户为准
  const autoExpanded = useMemo(() => {
    if (defaultExpanded !== undefined) return defaultExpanded;
    return status !== "success";
  }, [defaultExpanded, status]);
  const [userExpanded, setUserExpanded] = useState<boolean | null>(null);
  const expanded = userExpanded ?? autoExpanded;

  // 当 status 变化时重置 user override，让自动行为接管
  useEffect(() => {
    setUserExpanded(null);
  }, [status]);

  // 自动滚动到底部
  useEffect(() => {
    if (!expanded) return;
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [lines, expanded]);

  const handleCopy = async () => {
    const text = lines.map((l) => `[${l.stream}] ${l.line}`).join("\n");
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // 静默
    }
  };

  const statusBadge = (() => {
    switch (status) {
      case "running":
        return (
          <span className="inline-flex items-center gap-1 text-xs text-muted-foreground">
            <Loader2 className="h-3 w-3 animate-spin" />
            {t("doctor.log.running")}
          </span>
        );
      case "success":
        return (
          <span className="inline-flex items-center gap-1 text-xs text-emerald-600 dark:text-emerald-400">
            <CheckCircle2 className="h-3 w-3" />
            {t("doctor.log.success")}
          </span>
        );
      case "failed":
        return (
          <span className="inline-flex items-center gap-1 text-xs text-red-600 dark:text-red-400">
            <XCircle className="h-3 w-3" />
            {t("doctor.log.failed")}
          </span>
        );
      case "cancelled":
        return (
          <span className="inline-flex items-center gap-1 text-xs text-muted-foreground">
            <StopCircle className="h-3 w-3" />
            {t("doctor.log.cancelled")}
          </span>
        );
      default:
        return null;
    }
  })();

  return (
    <div className="rounded-lg border border-border bg-card/40 overflow-hidden">
      <div className="flex items-center justify-between gap-2 px-3 py-2 border-b border-border bg-muted/30">
        <button
          type="button"
          onClick={() => setUserExpanded(!expanded)}
          className="flex items-center gap-2 text-sm font-medium text-foreground/80 hover:text-foreground transition-colors"
        >
          {expanded ? (
            <ChevronUp className="h-4 w-4" />
          ) : (
            <ChevronDown className="h-4 w-4" />
          )}
          <span>{t("doctor.log.title")}</span>
          <span className="text-xs text-muted-foreground">
            ({lines.length})
          </span>
          {statusBadge}
        </button>
        <div className="flex items-center gap-1">
          {status === "running" && onCancel && (
            <Button
              variant="ghost"
              size="sm"
              onClick={onCancel}
              disabled={isCancelling}
              className="h-7 px-2 text-xs text-red-600 hover:text-red-700 hover:bg-red-500/10"
            >
              <StopCircle className="h-3.5 w-3.5 mr-1" />
              {isCancelling ? t("doctor.log.cancelling") : t("doctor.log.cancel")}
            </Button>
          )}
          <Button
            variant="ghost"
            size="sm"
            onClick={handleCopy}
            disabled={lines.length === 0}
            className="h-7 px-2 text-xs"
          >
            <Copy className="h-3.5 w-3.5 mr-1" />
            {copied ? t("doctor.log.copied") : t("doctor.log.copy")}
          </Button>
        </div>
      </div>
      {expanded && (
        <div
          ref={scrollRef}
          className="font-mono text-xs leading-relaxed bg-zinc-950 text-zinc-100 dark:bg-zinc-900 max-h-52 min-h-[120px] overflow-y-auto px-3 py-2 whitespace-pre-wrap break-all"
        >
          {lines.length === 0 ? (
            <div className="text-zinc-500 italic">{t("doctor.log.empty")}</div>
          ) : (
            lines.map((l) => (
              <div key={l.id} className={streamColor[l.stream]}>
                {l.line}
              </div>
            ))
          )}
        </div>
      )}
    </div>
  );
}
