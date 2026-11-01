import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { useInFlightRequests } from "@/lib/query/failover";
import { cn } from "@/lib/utils";
import type { InFlightRequest } from "@/types/proxy";

interface InFlightSectionProps {
  /** 与日志表同一套筛选：在飞明细跟着筛选走，不然两块数据对不上口径 */
  appType?: string;
  providerName?: string;
  model?: string;
  /** 只在第一页置顶：翻页说明用户在看历史，别把实时行怼在脸上 */
  active: boolean;
}

/**
 * 请求日志上方的「正在进行」区块。
 *
 * 在飞的请求在这里显示明细并实时计时；请求结束后条目从这里消失、出现在下方的
 * 日志表里——「飞完滚下去」就是这个交接，不需要给日志表本身加任何排序逻辑。
 */
export function InFlightSection({
  appType,
  providerName,
  model,
  active,
}: InFlightSectionProps) {
  const { t } = useTranslation();
  const { data: all } = useInFlightRequests(active);

  const entries = (all ?? []).filter((entry) => {
    if (appType && appType !== "all" && entry.appType !== appType) return false;
    if (providerName && entry.providerName !== providerName) return false;
    if (model && entry.model !== model) return false;
    return true;
  });

  // 秒级计时：有在飞行才走表，没请求时这个 interval 不存在
  const [, setTick] = useState(0);
  useEffect(() => {
    if (entries.length === 0) return;
    const timer = window.setInterval(() => setTick((v) => v + 1), 1000);
    return () => window.clearInterval(timer);
  }, [entries.length]);

  // 「飞完滚下去」的另一半：日志表默认不自动刷新（refreshIntervalMs=0，
  // 手动刷新是刻意省流量的设计），所以请求结束的信号必须由这里发给日志查询，
  // 否则请求会从置顶消失却迟迟不出现在下面的表里，看起来像被吞了。
  // 只失效 ["usage","logs"] 这一支，不碰统计卡片那些聚合查询。
  const queryClient = useQueryClient();
  const shownIdsRef = useRef<Set<number>>(new Set());
  useEffect(() => {
    const shown = shownIdsRef.current;
    const current = new Set(entries.map((entry) => entry.id));
    let finished = false;
    for (const id of shown) {
      if (!current.has(id)) {
        finished = true;
        break;
      }
    }
    shownIdsRef.current = current;
    if (finished) {
      void queryClient.invalidateQueries({ queryKey: ["usage", "logs"] });
    }
  }, [entries, queryClient]);

  if (!active || entries.length === 0) return null;

  return (
    <div className="rounded-lg border border-border/50 bg-card/40 p-2 backdrop-blur-sm">
      <div className="mb-1.5 flex items-center gap-1.5 px-1 text-caption text-fg-2">
        <Loader2 className="h-3 w-3 animate-spin" />
        {t("usage.inFlightLog.title", {
          count: entries.length,
          defaultValue: `${entries.length} 个请求正在进行`,
        })}
      </div>
      <ul className="space-y-1">
        {entries.map((entry) => (
          <InFlightRow key={entry.id} entry={entry} />
        ))}
      </ul>
    </div>
  );
}

function InFlightRow({ entry }: { entry: InFlightRequest }) {
  const { t } = useTranslation();
  const startedMs = new Date(entry.startedAt).getTime();
  // 开始时间解析不出来时显示「—」；编一个 0s 比显示缺失更误导
  const elapsed = Number.isFinite(startedMs)
    ? formatElapsed(Date.now() - startedMs)
    : null;

  return (
    <li className="flex items-center gap-2 rounded px-1 py-0.5 text-caption text-fg-2">
      <span className="inline-block h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-emerald-500" />
      <span className="w-16 shrink-0 font-mono tabular-nums">
        {elapsed ?? "—"}
      </span>
      <span
        className="min-w-0 max-w-[180px] truncate"
        title={entry.providerName}
      >
        {entry.providerName}
      </span>
      <span
        className="min-w-0 flex-1 truncate"
        title={entry.model ?? undefined}
      >
        {entry.model ??
          t("usage.inFlightLog.modelPending", { defaultValue: "模型解析中…" })}
      </span>
      {entry.streaming && (
        <span
          className={cn(
            "shrink-0 rounded bg-emerald-500/10 px-1",
            "text-emerald-600 dark:text-emerald-400",
          )}
        >
          {t("usage.inFlightLog.streaming", { defaultValue: "流式" })}
        </span>
      )}
    </li>
  );
}

function formatElapsed(ms: number): string {
  const totalSeconds = Math.max(0, Math.floor(ms / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  if (minutes === 0) return `${seconds}s`;
  return `${minutes}m${String(seconds).padStart(2, "0")}s`;
}
