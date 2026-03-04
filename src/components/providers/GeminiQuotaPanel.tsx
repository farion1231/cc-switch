import { useMemo, useState, type MouseEvent } from "react";
import { Clock3, RefreshCw, AlertCircle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useQueryClient } from "@tanstack/react-query";
import { geminiApi } from "@/lib/api";
import { useGeminiUsageStateQuery } from "@/lib/query/queries";

interface GeminiQuotaPanelProps {
  providerId: string;
  inline?: boolean;
}

function formatDuration(seconds?: number): string {
  if (seconds === undefined) return "--";
  if (seconds <= 0) return "0m";
  const h = Math.floor(seconds / 3600);
  const m = Math.ceil((seconds % 3600) / 60);
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
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

export default function GeminiQuotaPanel({
  providerId,
  inline = false,
}: GeminiQuotaPanelProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [isManualRefreshing, setIsManualRefreshing] = useState(false);

  const { data, isFetching, refetch } = useGeminiUsageStateQuery(providerId, {
    enabled: !!providerId,
    refetchIntervalMs: 60000,
  });

  const loading = isFetching || isManualRefreshing;

  const {
    statusText,
    statusColorClass,
    cooldownSeconds,
    errorText,
    updatedAt,
    hasAccount,
  } = useMemo(() => {
      if (!data) {
        return {
          statusText: "Unbound/待登录",
          statusColorClass: "text-muted-foreground",
          cooldownSeconds: undefined,
          errorText: "",
          updatedAt: undefined,
          hasAccount: false,
        };
      }
      const usage = data?.usage;
      const usageCompat = usage as
        | { available?: boolean; cooldownSeconds?: number }
        | undefined;
      const available = data?.available ?? usageCompat?.available;
      const cooldown = data?.cooldownSeconds ?? usageCompat?.cooldownSeconds;
      const error = usage?.lastError ?? data?.error;
      const hasAccount = Boolean(
        data?.account?.accountId ||
          data?.account?.googleAccountId ||
          data?.account?.id,
      );

      if (error) {
        return {
          statusText: t("usage.queryFailed", { defaultValue: "错误" }),
          statusColorClass: "text-destructive",
          cooldownSeconds: cooldown,
          errorText: error,
          updatedAt: usage?.lastRefreshAt ?? data?.updatedAtMs,
          hasAccount,
        };
      }

      if (available) {
        return {
          statusText: t("usage.available", { defaultValue: "可用" }),
          statusColorClass: "text-emerald-600",
          cooldownSeconds: cooldown,
          errorText: "",
          updatedAt: usage?.lastRefreshAt ?? data?.updatedAtMs,
          hasAccount,
        };
      }

      if (typeof cooldown === "number" && cooldown > 0) {
        return {
          statusText: t("usage.cooling", { defaultValue: "冷却中" }),
          statusColorClass: "text-amber-600",
          cooldownSeconds: cooldown,
          errorText: "",
          updatedAt: usage?.lastRefreshAt ?? data?.updatedAtMs,
          hasAccount,
        };
      }

      return {
        statusText: hasAccount
          ? t("usage.refreshPending", { defaultValue: "待刷新" })
          : "Unbound/待登录",
        statusColorClass: "text-muted-foreground",
        cooldownSeconds: cooldown,
        errorText: "",
        updatedAt: usage?.lastRefreshAt ?? data?.updatedAtMs,
        hasAccount,
      };
    }, [data, t]);

  const handleRefresh = async (stopPropagation = false, e?: MouseEvent) => {
    if (stopPropagation && e) {
      e.stopPropagation();
    }
    if (loading) return;

    setIsManualRefreshing(true);
    try {
      const result = await geminiApi.refreshUsageNow(providerId);
      if (result.refreshedAccounts === 0) {
        toast.warning("该账号未绑定可刷新用量的 Gemini 登录态");
      } else if (result.failedAccounts > 0) {
        toast.warning(
          `Gemini 用量刷新完成：成功 ${result.successAccounts}，失败 ${result.failedAccounts}`,
        );
      } else {
        toast.success(
          `Gemini 用量已刷新：${result.successAccounts}/${result.refreshedAccounts}`,
        );
      }
      await queryClient.invalidateQueries({
        queryKey: ["gemini-usage-state", providerId],
      });
      await refetch();
    } catch (error) {
      const detail = error instanceof Error ? error.message : String(error);
      toast.error(`刷新 Gemini 用量失败${detail ? `: ${detail}` : ""}`);
    } finally {
      setIsManualRefreshing(false);
    }
  };

  if (inline) {
    return (
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <span className={statusColorClass}>{statusText}</span>
        {typeof cooldownSeconds === "number" && cooldownSeconds > 0 && (
          <span className="inline-flex items-center gap-1">
            <Clock3 size={12} />
            {formatDuration(cooldownSeconds)}
          </span>
        )}
        <button
          onClick={(e) => void handleRefresh(true, e)}
          className="p-1 rounded hover:bg-muted"
          title={t("usage.refreshUsage", { defaultValue: "刷新用量" })}
          disabled={loading || !hasAccount}
        >
          <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
        </button>
      </div>
    );
  }

  return (
    <div className="mt-3 rounded-xl border border-border-default bg-card px-4 py-3 shadow-sm">
      <div className="flex items-center justify-between mb-3">
        <span className="text-xs font-medium">Gemini Quota</span>
        <button
          onClick={() => void handleRefresh()}
          className="inline-flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground transition-colors"
          disabled={loading || !hasAccount}
          title={t("usage.refreshUsage", { defaultValue: "刷新用量" })}
        >
          <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
          {t("usage.refreshUsage", { defaultValue: "刷新用量" })}
        </button>
      </div>

      <div className="space-y-2 text-xs">
        <div className="flex items-center justify-between">
          <span className="text-muted-foreground">状态</span>
          <span className={statusColorClass}>{statusText}</span>
        </div>
        <div className="flex items-center justify-between">
          <span className="text-muted-foreground">冷却</span>
          <span>
            {typeof cooldownSeconds === "number"
              ? formatDuration(cooldownSeconds)
              : "--"}
          </span>
        </div>
        <div className="flex items-center justify-between">
          <span className="text-muted-foreground">更新时间</span>
          <span>{formatLastUpdated(updatedAt)}</span>
        </div>
      </div>

      {errorText && (
        <div className="mt-2 flex items-start gap-1.5 rounded-md bg-destructive/10 px-2 py-1.5 text-xs text-destructive">
          <AlertCircle size={12} className="mt-0.5 shrink-0" />
          <span className="break-all">{errorText}</span>
        </div>
      )}
    </div>
  );
}
