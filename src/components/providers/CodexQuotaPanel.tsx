import { useMemo, useState, type MouseEvent } from "react";
import { Clock3, RefreshCw, AlertCircle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useQueryClient } from "@tanstack/react-query";
import { codexApi } from "@/lib/api";
import { useCodexUsageStateQuery } from "@/lib/query/queries";

interface CodexQuotaPanelProps {
  providerId: string;
  inline?: boolean;
}

function clampPercent(v?: number): number {
  if (typeof v !== "number" || Number.isNaN(v)) return 0;
  return Math.max(0, Math.min(100, v));
}

function remainingPercent(usedPercent?: number): number {
  return 100 - clampPercent(usedPercent);
}

function calcResetSeconds(resetAfterSeconds?: number, resetAtEpochSeconds?: number): number | undefined {
  if (typeof resetAfterSeconds === "number" && resetAfterSeconds > 0) {
    return resetAfterSeconds;
  }
  if (typeof resetAtEpochSeconds === "number" && resetAtEpochSeconds > 0) {
    const nowSec = Math.floor(Date.now() / 1000);
    const delta = resetAtEpochSeconds - nowSec;
    return delta > 0 ? delta : 0;
  }
  return undefined;
}

function formatDuration(seconds?: number): string {
  if (seconds === undefined) return "--";
  if (seconds <= 0) return "0m";
  const h = Math.floor(seconds / 3600);
  const m = Math.ceil((seconds % 3600) / 60);
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

function formatResetAt(epochSeconds?: number): string {
  if (!epochSeconds) return "--";
  const d = new Date(epochSeconds * 1000);
  return d.toLocaleString();
}

function formatLastUpdated(tsMs?: number): string {
  if (!tsMs) return "--";
  const diffMs = Date.now() - tsMs;
  if (diffMs < 60_000) return "刚刚";
  const mins = Math.floor(diffMs / 60_000);
  if (mins < 60) return `${mins} 分钟前`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours} 小时前`;
  const days = Math.floor(hours / 24);
  return `${days} 天前`;
}

interface WindowRowProps {
  label: string;
  remainingPct: number;
  resetIn?: number;
  colorClass?: string;
}

function WindowRow({ label, remainingPct, resetIn, colorClass = "bg-emerald-500" }: WindowRowProps) {
  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between text-xs text-muted-foreground">
        <span className="font-medium">{label}</span>
        <span>{`${remainingPct.toFixed(0)}% left`} • {`resets ${formatDuration(resetIn)}`}</span>
      </div>
      <div className="h-2 w-full rounded bg-muted overflow-hidden">
        <div
          className={`h-full transition-all ${colorClass}`}
          style={{ width: `${remainingPct}%` }}
        />
      </div>
    </div>
  );
}

export default function CodexQuotaPanel({
  providerId,
  inline = false,
}: CodexQuotaPanelProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [isManualRefreshing, setIsManualRefreshing] = useState(false);

  const { data, isFetching, refetch } = useCodexUsageStateQuery(providerId, {
    enabled: !!providerId,
    refetchIntervalMs: 60000,
  });

  const usage = data?.usage;

  const {
    primaryRemaining,
    secondaryRemaining,
    combinedRemaining,
    primaryResetIn,
    secondaryResetIn,
    balance,
    status,
  } = useMemo(() => {
    const primaryRemaining = remainingPercent(usage?.primaryUsedPercent);
    const secondaryRemaining = remainingPercent(usage?.secondaryUsedPercent);
    const combinedRemaining = Math.min(primaryRemaining, secondaryRemaining);

    const primaryResetIn = calcResetSeconds(
      usage?.primaryResetAfterSeconds,
      usage?.primaryResetAt,
    );
    const secondaryResetIn = calcResetSeconds(
      usage?.secondaryResetAfterSeconds,
      usage?.secondaryResetAt,
    );

    const balance =
      usage?.creditsUnlimited === true
        ? t("usage.unlimited", { defaultValue: "无限" })
        : usage?.creditsBalance?.toFixed(2) ?? "--";

    const status = data?.available
      ? t("usage.available", { defaultValue: "可用" })
      : t("usage.cooling", { defaultValue: "冷却中" });

    return {
      primaryRemaining,
      secondaryRemaining,
      combinedRemaining,
      primaryResetIn,
      secondaryResetIn,
      balance,
      status,
    };
  }, [data?.available, t, usage]);

  if (!data) return null;

  const loading = isFetching || isManualRefreshing;

  const handleRefresh = async (stopPropagation = false, e?: MouseEvent) => {
    if (stopPropagation && e) {
      e.stopPropagation();
    }
    if (loading) return;

    setIsManualRefreshing(true);
    try {
      const result = await codexApi.refreshUsageNow(providerId);
      if (result.refreshedAccounts === 0) {
        toast.warning("该账号未绑定可刷新用量的 Codex 登录态");
      }
      await queryClient.invalidateQueries({
        queryKey: ["codex-usage-state", providerId],
      });
      await refetch();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      toast.error(`刷新用量失败: ${message}`);
    } finally {
      setIsManualRefreshing(false);
    }
  };

  if (inline) {
    return (
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <span className={data.available ? "text-emerald-600" : "text-amber-600"}>
          {status}
        </span>
        <span>{combinedRemaining.toFixed(0)}%</span>
        {!data.available && (
          <span className="inline-flex items-center gap-1">
            <Clock3 size={12} />
            {formatDuration(data.cooldownSeconds ?? primaryResetIn ?? secondaryResetIn)}
          </span>
        )}
        <button
          onClick={(e) => void handleRefresh(true, e)}
          className="p-1 rounded hover:bg-muted"
          title={t("usage.refreshUsage", { defaultValue: "刷新用量" })}
          disabled={loading}
        >
          <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
        </button>
      </div>
    );
  }

  return (
    <div className="mt-3 rounded-xl border border-border-default bg-card px-4 py-3 shadow-sm">
      <div className="flex items-center justify-between mb-3">
        <div className="flex items-center gap-2">
          <span className="text-xs font-medium">
            {t("usage.codexQuotaTitle", { defaultValue: "Codex 额度" })}
          </span>
          <span
            className={`text-xs ${data.available ? "text-emerald-600" : "text-amber-600"}`}
          >
            {status}
          </span>
        </div>
        <button
          onClick={() => void handleRefresh()}
          className="p-1 rounded hover:bg-muted"
          title={t("usage.refreshUsage", { defaultValue: "刷新用量" })}
          disabled={loading}
        >
          <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
        </button>
      </div>

      {usage?.lastError && (
        <div className="mb-2 text-xs text-amber-600 inline-flex items-center gap-1">
          <AlertCircle size={12} />
          {usage.lastError}
        </div>
      )}

      <div className="space-y-3">
        <WindowRow
          label="5h Limit (5h)"
          remainingPct={primaryRemaining}
          resetIn={primaryResetIn}
          colorClass="bg-emerald-500"
        />

        <WindowRow
          label="Weekly Limit (7d)"
          remainingPct={secondaryRemaining}
          resetIn={secondaryResetIn}
          colorClass="bg-emerald-500"
        />

        <div className="text-xs text-muted-foreground">
          Credits: {balance}
        </div>

        <div className="text-xs text-muted-foreground">
          Last updated: {formatLastUpdated(usage?.lastRefreshAt)}
        </div>

        <div className="text-xs text-muted-foreground">
          Primary reset at: {formatResetAt(usage?.primaryResetAt)} · Secondary reset at: {formatResetAt(usage?.secondaryResetAt)}
        </div>
      </div>
    </div>
  );
}
