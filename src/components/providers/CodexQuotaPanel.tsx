import { Clock, RefreshCw, AlertCircle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { codexApi } from "@/lib/api";
import { useCodexUsageStateQuery } from "@/lib/query/queries";

interface CodexQuotaPanelProps {
  providerId: string;
  inline?: boolean;
}

function formatDuration(seconds?: number): string {
  if (!seconds || seconds <= 0) return "0m";
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

export default function CodexQuotaPanel({
  providerId,
  inline = false,
}: CodexQuotaPanelProps) {
  const { t } = useTranslation();
  const { data, isFetching, refetch } = useCodexUsageStateQuery(providerId, {
    enabled: !!providerId,
    refetchIntervalMs: 60000,
  });

  const usage = data?.usage;
  const pct = Math.max(
    usage?.primaryUsedPercent ?? 0,
    usage?.secondaryUsedPercent ?? 0,
  );
  const status = data?.available
    ? t("usage.available", { defaultValue: "可用" })
    : t("usage.cooling", { defaultValue: "冷却中" });
  const cooldown = formatDuration(data?.cooldownSeconds);
  const balance =
    usage?.creditsUnlimited === true
      ? t("usage.unlimited", { defaultValue: "无限" })
      : usage?.creditsBalance?.toFixed(2) ?? "--";
  const resetAt = formatResetAt(
    usage?.primaryResetAt ?? usage?.secondaryResetAt ?? undefined,
  );

  if (!data) return null;

  if (inline) {
    return (
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <span className={data.available ? "text-emerald-600" : "text-amber-600"}>
          {status}
        </span>
        <span>{pct.toFixed(0)}%</span>
        {!data.available && (
          <span className="inline-flex items-center gap-1">
            <Clock size={12} />
            {cooldown}
          </span>
        )}
        <button
          onClick={(e) => {
            e.stopPropagation();
            void codexApi.refreshUsageNow(providerId).then(() => refetch());
          }}
          className="p-1 rounded hover:bg-muted"
          title={t("usage.refreshUsage")}
        >
          <RefreshCw size={12} className={isFetching ? "animate-spin" : ""} />
        </button>
      </div>
    );
  }

  return (
    <div className="mt-3 rounded-xl border border-border-default bg-card px-4 py-3 shadow-sm">
      <div className="flex items-center justify-between mb-2">
        <span className="text-xs font-medium">
          {t("usage.codexQuotaTitle", { defaultValue: "Codex 额度" })}
        </span>
        <button
          onClick={() => void codexApi.refreshUsageNow(providerId).then(() => refetch())}
          className="p-1 rounded hover:bg-muted"
          title={t("usage.refreshUsage")}
        >
          <RefreshCw size={12} className={isFetching ? "animate-spin" : ""} />
        </button>
      </div>

      {usage?.lastError && (
        <div className="mb-2 text-xs text-amber-600 inline-flex items-center gap-1">
          <AlertCircle size={12} />
          {usage.lastError}
        </div>
      )}

      <div className="space-y-2">
        <div className="h-2 w-full rounded bg-muted overflow-hidden">
          <div
            className="h-full bg-blue-500 transition-all"
            style={{ width: `${Math.min(100, Math.max(0, pct))}%` }}
          />
        </div>
        <div className="flex items-center justify-between text-xs text-muted-foreground">
          <span>{status}</span>
          {!data.available && (
            <span>
              {t("usage.codexQuotaResetIn", { defaultValue: "恢复" })}: {cooldown}
            </span>
          )}
        </div>
        <div className="text-xs text-muted-foreground">
          {t("usage.balance", { defaultValue: "余额" })}: {balance}
        </div>
        <div className="text-xs text-muted-foreground">
          {t("usage.codexQuotaResetAt", { defaultValue: "恢复时间" })}: {resetAt}
        </div>
      </div>
    </div>
  );
}
