import { useMemo } from "react";
import { useCodexAllQuotas } from "@/lib/query/subscription";
import type { SubscriptionQuota } from "@/types/subscription";

function remaining(quota: SubscriptionQuota, tierName: string): number | null {
  const tier = quota.tiers.find((item) => item.name === tierName);
  return tier ? Math.max(0, 100 - Math.round(tier.utilization)) : null;
}

export default function NotchOverlay() {
  const quotasQuery = useCodexAllQuotas({ enabled: true, autoQuery: true });
  const summary = useMemo(() => {
    const quotas = Object.values(quotasQuery.data ?? {}).filter(
      (quota) => quota.success,
    );
    if (quotas.length === 0) return "Codex --";

    const best = quotas
      .map((quota) => ({
        fiveHour: remaining(quota, "five_hour"),
        sevenDay: remaining(quota, "seven_day"),
      }))
      .sort((a, b) => (b.fiveHour ?? -1) - (a.fiveHour ?? -1))[0];

    const fiveHour = best.fiveHour == null ? "--" : `${best.fiveHour}%`;
    const sevenDay = best.sevenDay == null ? "--" : `${best.sevenDay}%`;
    return `Codex 5h ${fiveHour} · 7d ${sevenDay}`;
  }, [quotasQuery.data]);

  return (
    <div className="flex h-full w-full items-center justify-center px-4">
      <div className="max-w-[520px] truncate rounded-full bg-black/70 px-4 py-1 text-center text-xs font-medium text-white shadow-lg">
        {quotasQuery.isLoading ? "Codex loading..." : summary}
      </div>
    </div>
  );
}
