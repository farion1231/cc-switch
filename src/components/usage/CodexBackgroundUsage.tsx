import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { usageApi } from "@/lib/api/usage";
import { usageKeys } from "@/lib/query/usage";
import { resolveUsageRange } from "@/lib/usageRange";
import type { UsageRangeSelection } from "@/types/usage";

export function CodexBackgroundUsage({
  range,
  model,
  refreshIntervalMs,
}: {
  range: UsageRangeSelection;
  model?: string;
  refreshIntervalMs: number;
}) {
  const { t } = useTranslation();
  const query = useQuery({
    queryKey: [...usageKeys.all, "codex-background", range, model],
    queryFn: () => {
      const { startDate, endDate } = resolveUsageRange(range);
      return usageApi.getCodexBackgroundUsage(startDate, endDate, model);
    },
    refetchInterval: refreshIntervalMs || false,
  });
  if (query.isPending) return null;
  if (query.isError) return <p role="alert">{t("usage.background.error")}</p>;
  if (!query.data.length) return null;
  return (
    <section
      className="rounded-xl border bg-card p-4 space-y-3"
      aria-label={t("usage.background.title")}
    >
      <h3 className="font-medium">{t("usage.background.title")}</h3>
      <p className="text-sm text-muted-foreground">
        {t("usage.background.description")}
      </p>
      <div className="overflow-x-auto">
        <table className="w-full text-sm text-left">
          <thead>
            <tr>
              {[
                "model",
                "feature",
                "status",
                "generations",
                "input",
                "cached",
                "output",
                "cost",
              ].map((key) => (
                <th className="p-2" key={key}>
                  {t(`usage.background.${key}`)}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {query.data.map((row) => (
              <tr
                key={`${row.model}:${row.feature}:${row.status}`}
                className="border-t"
              >
                <td className="p-2">{row.model}</td>
                <td className="p-2">{row.feature}</td>
                <td className="p-2">{row.status}</td>
                <td className="p-2">{row.generations.toLocaleString()}</td>
                <td className="p-2">{row.inputTokens.toLocaleString()}</td>
                <td className="p-2">
                  {row.cachedInputTokens.toLocaleString()}
                </td>
                <td className="p-2">{row.outputTokens.toLocaleString()}</td>
                <td className="p-2">
                  {row.estimatedCostUsd === null
                    ? t("usage.background.unknown")
                    : `$${Number(row.estimatedCostUsd).toFixed(6)}`}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}
