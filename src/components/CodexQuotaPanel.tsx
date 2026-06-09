import { RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { useCodexAllQuotas } from "@/lib/query/subscription";
import type { QuotaTier, SubscriptionQuota } from "@/types/subscription";

function formatTier(tier: QuotaTier | undefined): string {
  if (!tier) return "--";
  const remaining = Math.max(0, 100 - Math.round(tier.utilization));
  return `${remaining}% left`;
}

function quotaTone(quota: SubscriptionQuota | undefined): string {
  if (!quota?.success) return "text-muted-foreground";
  const maxUsage = Math.max(0, ...quota.tiers.map((tier) => tier.utilization));
  if (maxUsage >= 90) return "text-red-500";
  if (maxUsage >= 70) return "text-amber-500";
  return "text-emerald-500";
}

export function CodexQuotaPanel() {
  const { t } = useTranslation();
  const quotasQuery = useCodexAllQuotas({ enabled: true, autoQuery: true });
  const quotas = Object.entries(quotasQuery.data ?? {});

  return (
    <section className="rounded-lg border bg-card p-4">
      <div className="flex items-center justify-between gap-3">
        <div>
          <h2 className="text-sm font-semibold tracking-normal">
            {t("codexQuota.title", { defaultValue: "Codex account usage" })}
          </h2>
          <p className="text-xs text-muted-foreground">
            {t("codexQuota.subtitle", {
              defaultValue: "Live quota snapshots for saved Codex accounts",
            })}
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void quotasQuery.refetch()}
          disabled={quotasQuery.isFetching}
        >
          <RefreshCw
            className={
              quotasQuery.isFetching ? "h-4 w-4 animate-spin" : "h-4 w-4"
            }
          />
          {t("common.refresh", { defaultValue: "Refresh" })}
        </Button>
      </div>

      <div className="mt-3 grid gap-2 md:grid-cols-2">
        {quotas.length === 0 ? (
          <div className="text-sm text-muted-foreground">
            {quotasQuery.isLoading
              ? t("common.loading", { defaultValue: "Loading..." })
              : t("codexQuota.empty", { defaultValue: "No quota data yet" })}
          </div>
        ) : (
          quotas.map(([accountKey, quota]) => {
            const fiveHour = quota.tiers.find(
              (tier) => tier.name === "five_hour",
            );
            const sevenDay = quota.tiers.find(
              (tier) => tier.name === "seven_day",
            );
            return (
              <div
                key={accountKey}
                className="rounded-md border bg-background p-3"
              >
                <div className="flex items-center justify-between gap-2">
                  <span className="truncate text-sm font-medium">
                    {accountKey}
                  </span>
                  <Badge variant={quota.success ? "default" : "secondary"}>
                    {quota.success ? "OK" : "N/A"}
                  </Badge>
                </div>
                <div className={`mt-2 text-xs ${quotaTone(quota)}`}>
                  5h {formatTier(fiveHour)} · 7d {formatTier(sevenDay)}
                </div>
                {quota.error ? (
                  <p className="mt-1 line-clamp-2 text-xs text-muted-foreground">
                    {quota.error}
                  </p>
                ) : null}
              </div>
            );
          })
        )}
      </div>
    </section>
  );
}
