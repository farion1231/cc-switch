import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ChevronDown, KeyRound, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { CodexAccountsManager } from "@/components/codex/CodexAccountsPanel";
import { CodexQuotaForecastBadge } from "@/components/codex/CodexQuotaForecastBadge";
import { codexAccountsApi } from "@/lib/api";
import type { CodexAccountSummary } from "@/lib/api/codexAccounts";
import {
  useCodexAllQuotas,
  useCodexQuotaForecasts,
} from "@/lib/query/subscription";
import { cn } from "@/lib/utils";
import type { QuotaTier } from "@/types/subscription";

const CODEX_ACCOUNTS_QUERY_KEY = ["codex", "account-snapshots"] as const;

function getRemainingPercent(tier: QuotaTier | undefined): number | null {
  if (!tier) return null;
  return Math.max(0, 100 - Math.round(tier.utilization));
}

function tierTone(tier: QuotaTier | undefined): string {
  if (!tier) return "text-muted-foreground";
  if (tier.utilization >= 90) return "text-red-500";
  if (tier.utilization >= 70) return "text-amber-500";
  return "text-emerald-500";
}

function formatResetTime(
  iso: string | null | undefined,
  t: ReturnType<typeof useTranslation>["t"],
): string | null {
  if (!iso) return null;
  const date = new Date(iso);
  const diffMs = date.getTime() - Date.now();
  if (!Number.isFinite(diffMs) || diffMs <= 0) {
    return t("codexQuota.resetSoon", { defaultValue: "reset soon" });
  }

  const diffMinutes = Math.floor(diffMs / (1000 * 60));
  const diffHours = Math.floor(diffMinutes / 60);
  const diffDays = Math.floor(diffHours / 24);
  if (diffDays > 0) {
    return t("codexQuota.resetInDays", {
      defaultValue: "resets in {{count}}d",
      count: diffDays,
    });
  }
  if (diffHours > 0) {
    return t("codexQuota.resetInHours", {
      defaultValue: "resets in {{count}}h",
      count: diffHours,
    });
  }
  return t("codexQuota.resetInMinutes", {
    defaultValue: "resets in {{count}}m",
    count: Math.max(diffMinutes, 1),
  });
}

function QuotaPill({
  label,
  tier,
}: {
  label: string;
  tier: QuotaTier | undefined;
}) {
  const { t } = useTranslation();
  const remaining = getRemainingPercent(tier);
  const reset = formatResetTime(tier?.resetsAt, t);
  return (
    <span className="inline-flex min-w-0 items-center gap-1 text-xs">
      <span className="text-muted-foreground">{label}</span>
      <span className={tierTone(tier)}>
        {remaining == null ? "--" : `${remaining}%`}
      </span>
      {reset ? <span className="text-muted-foreground">· {reset}</span> : null}
    </span>
  );
}

function accountFallback(
  accounts: CodexAccountSummary[],
): CodexAccountSummary | undefined {
  return accounts.find((account) => account.isActive) ?? accounts[0];
}

export function CodexQuotaPanel() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  const accountsQuery = useQuery({
    queryKey: CODEX_ACCOUNTS_QUERY_KEY,
    queryFn: () => codexAccountsApi.list(),
  });
  const quotasQuery = useCodexAllQuotas({
    enabled: true,
    autoQuery: false,
  });
  const forecastsQuery = useCodexQuotaForecasts();

  const accounts = useMemo(
    () => accountsQuery.data ?? [],
    [accountsQuery.data],
  );
  const quotaEntries = Object.entries(quotasQuery.data ?? {});
  const accountCount = Math.max(accounts.length, quotaEntries.length);
  const featuredAccount = accountFallback(accounts);
  const featuredQuota = featuredAccount
    ? quotasQuery.data?.[featuredAccount.accountKey]
    : quotaEntries[0]?.[1];
  const featuredAccountKey =
    featuredAccount?.accountKey ?? quotaEntries[0]?.[0];
  const featuredForecast = featuredAccountKey
    ? forecastsQuery.data?.[featuredAccountKey]
    : undefined;
  const fiveHour = featuredQuota?.tiers.find(
    (tier) => tier.name === "five_hour",
  );
  const sevenDay = featuredQuota?.tiers.find(
    (tier) => tier.name === "seven_day",
  );

  const refreshAll = async () => {
    await Promise.all([accountsQuery.refetch(), quotasQuery.refetch()]);
    await forecastsQuery.refetch();
  };

  return (
    <Collapsible open={open} onOpenChange={setOpen}>
      <section className="rounded-lg border border-emerald-500/20 bg-card p-4">
        <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <KeyRound className="h-4 w-4 text-emerald-600 dark:text-emerald-400" />
              <h2 className="text-sm font-semibold tracking-normal">
                {t("codexQuota.title", { defaultValue: "Codex usage" })}
              </h2>
              {accountCount > 0 ? (
                <Badge variant="secondary" className="h-5 px-1.5 text-[10px]">
                  {t("codexQuota.accountCount", {
                    defaultValue: "{{count}} accounts",
                    count: accountCount,
                  })}
                </Badge>
              ) : null}
            </div>
            <p className="mt-1 text-xs text-muted-foreground">
              {t("codexQuota.collapsedSubtitle", {
                defaultValue:
                  "Quota summary for saved official accounts. Expand to manage account snapshots.",
              })}
            </p>
          </div>

          <div className="flex shrink-0 items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => void refreshAll()}
              disabled={accountsQuery.isFetching || quotasQuery.isFetching}
            >
              <RefreshCw
                className={
                  accountsQuery.isFetching || quotasQuery.isFetching
                    ? "h-4 w-4 animate-spin"
                    : "h-4 w-4"
                }
              />
              {t("common.refresh", { defaultValue: "Refresh" })}
            </Button>
            <CollapsibleTrigger asChild>
              <Button variant="outline" size="sm">
                <ChevronDown
                  className={cn(
                    "h-4 w-4 transition-transform",
                    open && "rotate-180",
                  )}
                />
                {open
                  ? t("codexQuota.collapseManagement", {
                      defaultValue: "Collapse",
                    })
                  : t("codexQuota.expandManagement", {
                      defaultValue: "Manage accounts",
                    })}
              </Button>
            </CollapsibleTrigger>
          </div>
        </div>

        <div className="mt-3 rounded-md border bg-background/70 px-3 py-3">
          {featuredAccount ? (
            <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
              <div className="min-w-0">
                <div className="flex min-w-0 flex-wrap items-center gap-2">
                  <span className="truncate text-sm font-semibold">
                    {featuredAccount.profileName}
                  </span>
                  {featuredAccount.isActive ? (
                    <Badge className="border-emerald-500/30 bg-emerald-500/10 text-emerald-600 hover:bg-emerald-500/10 dark:text-emerald-400">
                      {t("provider.inUse", { defaultValue: "In use" })}
                    </Badge>
                  ) : null}
                  {featuredAccount.plan ? (
                    <Badge variant="outline">{featuredAccount.plan}</Badge>
                  ) : null}
                </div>
                <div className="mt-1 flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
                  <span>
                    {featuredAccount.emailMasked ||
                      featuredAccount.authMode ||
                      t("codexQuota.accountSnapshot", {
                        defaultValue: "Account snapshot",
                      })}
                  </span>
                  {featuredAccount.lastUsedAt ? (
                    <span>
                      {new Date(
                        featuredAccount.lastUsedAt * 1000,
                      ).toLocaleString()}
                    </span>
                  ) : null}
                </div>
                <div className="mt-2">
                  <CodexQuotaForecastBadge forecast={featuredForecast} />
                </div>
              </div>

              <div className="flex flex-wrap gap-x-4 gap-y-1">
                <QuotaPill
                  label={t("codexQuota.fiveHourRemaining", {
                    defaultValue: "5h left:",
                  })}
                  tier={fiveHour}
                />
                <QuotaPill
                  label={t("codexQuota.sevenDayRemaining", {
                    defaultValue: "7d left:",
                  })}
                  tier={sevenDay}
                />
              </div>
            </div>
          ) : (
            <p className="text-sm text-muted-foreground">
              {accountsQuery.isLoading || quotasQuery.isLoading
                ? t("common.loading", { defaultValue: "Loading..." })
                : t("codexQuota.empty", {
                    defaultValue:
                      "No quota data yet. Save official Codex account snapshots first.",
                  })}
            </p>
          )}
        </div>

        <CollapsibleContent>
          <div className="mt-4 border-t pt-4">
            <CodexAccountsManager embedded />
          </div>
        </CollapsibleContent>
      </section>
    </Collapsible>
  );
}
