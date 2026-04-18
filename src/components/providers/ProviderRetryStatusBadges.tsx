import { AlertTriangle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import type { FailoverRetryPolicy } from "@/types";
import type { ProviderRetryState } from "@/types/proxy";
import { normalizeFailoverRetryPolicy } from "@/lib/failoverRetry";

interface ProviderRetryStatusBadgesProps {
  policy?: FailoverRetryPolicy;
  retryState?: ProviderRetryState;
}

export function ProviderRetryStatusBadges({
  policy,
  retryState,
}: ProviderRetryStatusBadgesProps) {
  const { t } = useTranslation();
  const normalizedPolicy = normalizeFailoverRetryPolicy(policy);
  const isInfinite = normalizedPolicy.mode === "infinite";

  return (
    <>
      {isInfinite && (
        <Badge
          variant="outline"
          className="border-amber-300/80 bg-amber-50 text-amber-900 dark:border-amber-500/40 dark:bg-amber-500/10 dark:text-amber-100"
          title={t("providerRetry.infiniteStaticHint", {
            defaultValue:
              "Infinite retry is enabled. Automatic failover will stay on this provider.",
          })}
        >
          {t("providerRetry.infiniteBadge", {
            defaultValue: "Infinite retry",
          })}
        </Badge>
      )}

      {retryState &&
        (retryState.sticky_infinite ? (
          <Badge
            variant="destructive"
          className="gap-1 border-transparent"
          title={t("providerRetry.stickyRuntimeHint", {
              defaultValue:
                "Currently stuck on this provider and will not auto-fallback.",
            })}
          >
            <AlertTriangle className="h-3 w-3" />
            {t("providerRetry.stickyRuntimeBadge", {
              delay: retryState.current_delay_seconds,
              defaultValue: "Stuck here · {{delay}}s",
            })}
          </Badge>
        ) : (
          <Badge
            variant="secondary"
            className="border border-blue-200 bg-blue-50 text-blue-700 dark:border-blue-500/30 dark:bg-blue-500/10 dark:text-blue-200"
            title={t("providerRetry.runtimeHint", {
              defaultValue:
                retryState.mode === "infinite"
                  ? "Retrying the current provider"
                  : "Provider-level retries before failover",
            })}
          >
            {retryState.mode === "infinite"
              ? t("providerRetry.runtimeInfinite", {
                  delay: retryState.current_delay_seconds,
                  defaultValue: "Retrying · {{delay}}s",
                })
              : t("providerRetry.runtimeFinite", {
                  current: retryState.current_retry,
                  max: retryState.max_retry ?? normalizedPolicy.maxRetries,
                  delay: retryState.current_delay_seconds,
                  defaultValue: "Retry {{current}}/{{max}} · {{delay}}s",
                })}
          </Badge>
        ))}
    </>
  );
}
