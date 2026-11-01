import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ChevronDown,
  ChevronRight,
  Loader2,
  RotateCcw,
  ShieldAlert,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  useCircuitBreakerStats,
  useResetCircuitBreaker,
} from "@/lib/query/failover";
import { cn } from "@/lib/utils";
import type { CircuitState, FailureEvidence } from "@/types/proxy";

interface BreakerEvidencePanelProps {
  providerId: string;
  appType: string;
  /** 代理没在跑时熔断器状态无从读取，整块不渲染 */
  enabled?: boolean;
}

/**
 * 供应商的熔断依据与手动恢复。
 *
 * 只做一件事：把「为什么判它不健康」摊开给用户看，并给一个明确的出口。
 * 之前界面上只有一个「熔断」徽章——用户既没法判断是上游真挂了还是本地网络抖了
 * 一下，也没法在确认没问题之后让它立刻回到调度里，只能干等超时。
 */
export function BreakerEvidencePanel({
  providerId,
  appType,
  enabled = true,
}: BreakerEvidencePanelProps) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const { data: stats } = useCircuitBreakerStats(providerId, appType, enabled);
  const reset = useResetCircuitBreaker();

  // 没有熔断器实例 = 这个供应商还没失败过。没什么可解释的，不占地方。
  if (!stats) return null;

  const evidence = stats.failureEvidence ?? [];
  const tripped = stats.state !== "closed";
  const resetLabel = t("proxy.breaker.clearCounters", {
    defaultValue: "清除计数并重新加入调度",
  });

  return (
    <div
      className={cn(
        "mt-2 rounded-md border px-2.5 py-2 text-xs",
        tripped
          ? "border-red-500/30 bg-red-500/5"
          : "border-border bg-background/40",
      )}
    >
      <div className="flex items-center justify-between gap-2">
        <button
          type="button"
          className="flex min-w-0 items-center gap-1.5 text-start text-fg-2 transition-colors hover:text-fg-1"
          onClick={() => setExpanded((v) => !v)}
          aria-expanded={expanded}
          disabled={evidence.length === 0}
          title={
            evidence.length === 0
              ? t("proxy.breaker.noEvidence", {
                  defaultValue: "没有失败证据",
                })
              : undefined
          }
        >
          {expanded ? (
            <ChevronDown className="h-3 w-3 shrink-0" />
          ) : (
            <ChevronRight className="h-3 w-3 shrink-0" />
          )}
          <ShieldAlert className="h-3 w-3 shrink-0" />
          <span className="truncate">
            {t("proxy.breaker.evidenceSummary", {
              // 不用 `count`：i18next 把它当复数键，会剥掉这个变量去查 `_one`/`_other`
              // 后缀，`{{count}}` 反而渲染不出来。
              evidenceCount: evidence.length,
              failures: stats.consecutiveFailures,
              defaultValue:
                "失败证据 {{evidenceCount}} 条 · 连续失败 {{failures}} 次",
            })}
          </span>
        </button>

        <Button
          size="sm"
          variant="outline"
          className="h-6 shrink-0 px-2 text-xs"
          onClick={() => reset.mutate({ providerId, appType })}
          disabled={reset.isPending}
          title={resetLabel}
        >
          {reset.isPending ? (
            <Loader2 className="h-3 w-3 animate-spin" />
          ) : (
            <RotateCcw className="h-3 w-3" />
          )}
          <span className="ms-1">
            {t("proxy.breaker.clearShort", {
              defaultValue: "清除计数",
            })}
          </span>
        </Button>
      </div>

      {expanded && evidence.length > 0 && (
        <ul className="mt-2 space-y-1 border-t border-border/60 pt-2">
          {evidence.map((item, index) => (
            <EvidenceRow
              key={`${item.at}-${index}`}
              evidence={item}
              state={stats.state}
            />
          ))}
        </ul>
      )}
    </div>
  );
}

function EvidenceRow({
  evidence,
  state,
}: {
  evidence: FailureEvidence;
  state: CircuitState;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage || i18n.language || "en";

  return (
    <li className="flex items-start gap-2 text-fg-2">
      <span className="shrink-0 font-mono text-[11px] text-fg-3">
        {formatEvidenceTime(evidence.at, locale)}
      </span>
      <span
        className={cn(
          "shrink-0 rounded px-1 font-mono text-[11px]",
          statusTone(evidence.status),
        )}
      >
        {/* 网络层失败没有状态码，显示成网络错误而不是编一个 0 */}
        {evidence.status ??
          t("proxy.breaker.networkError", {
            defaultValue: "网络",
          })}
      </span>
      <span className="min-w-0 flex-1 break-words" title={evidence.reason}>
        {evidence.reason}
      </span>
      {evidence.tripped && (
        <span className="shrink-0 rounded bg-red-500/15 px-1 text-[11px] text-red-600 dark:text-red-400">
          {state === "half_open"
            ? t("proxy.breaker.trippedProbe", {
                defaultValue: "探测失败",
              })
            : t("proxy.breaker.tripped", { defaultValue: "触发熔断" })}
        </span>
      )}
    </li>
  );
}

function statusTone(status: number | null): string {
  if (status === null)
    return "bg-yellow-500/15 text-yellow-600 dark:text-yellow-400";
  if (status >= 500) return "bg-red-500/15 text-red-600 dark:text-red-400";
  if (status === 429)
    return "bg-orange-500/15 text-orange-600 dark:text-orange-400";
  return "bg-muted text-fg-2";
}

/**
 * 证据里的时间是 RFC 3339（带时区），这里只显示到秒。
 *
 * 解析失败时原样回显：宁可让用户看到一段原始时间戳，也不要显示 "Invalid Date"。
 */
function formatEvidenceTime(at: string, locale: string): string {
  const date = new Date(at);
  if (Number.isNaN(date.getTime())) return at;
  return date.toLocaleTimeString(locale, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}
