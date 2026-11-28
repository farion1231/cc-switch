import { useTranslation } from "react-i18next";
import { formatHitRatePercent, formatTokensShort } from "./format";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import { cn } from "@/lib/utils";

interface UsageTooltipCardProps {
  heading: string;
  tokens: number;
  requests: number;
  /** 美元；null 表示没有可计价的数据 */
  cost: number | null;
  /** 传了才多显示一行缓存命中率（0–1）；null 表示没有输入、无从算起 */
  hitRate?: number | null;
  breakdown?: { label: string; color: string; value: string }[];
}

/** 热力图格子、趋势柱的悬停卡片：同一时间段的 Token、请求数和费用一起看。 */
export function UsageTooltipCard({
  heading,
  tokens,
  requests,
  cost,
  hitRate,
  breakdown = [],
}: UsageTooltipCardProps) {
  const { t, i18n } = useTranslation();
  const { money } = useMoneyFormat();
  const language = i18n.resolvedLanguage || i18n.language || "en";
  const rows = [
    {
      label: t("usage.trend.tokens"),
      value: formatTokensShort(tokens, language),
    },
    {
      label: t("usage.trend.requests"),
      value: formatTokensShort(requests, language),
    },
    {
      label: t("usage.trend.cost"),
      value: cost == null ? "—" : money(cost, 2),
    },
    ...(hitRate === undefined
      ? []
      : [
          {
            label: t("usage.cacheHitRate"),
            value: hitRate == null ? "—" : formatHitRatePercent(hitRate),
          },
        ]),
  ];

  return (
    <div className="min-w-[180px] rounded-[10px] border border-border bg-surface px-4 py-3 shadow-v7-lg">
      <p className="m-0 mb-2 whitespace-nowrap text-body font-semibold text-fg-1">
        {heading}
      </p>
      <div className="grid grid-cols-[auto_1fr] gap-x-6 gap-y-1 text-body">
        {rows.map((row) => (
          <div key={row.label} className="contents">
            <span className="whitespace-nowrap text-fg-2">{row.label}</span>
            <span className="whitespace-nowrap text-end tabular-nums text-fg-1">
              {row.value}
            </span>
          </div>
        ))}
      </div>
      {breakdown.length > 0 && (
        <ul
          className={cn(
            "mb-0 mt-2 grid list-none gap-x-5 gap-y-1 border-t border-border p-0 pt-2 text-caption",
            breakdown.length > 8 && "grid-cols-2",
          )}
        >
          {breakdown.map(({ label, color, value }) => (
            <li key={label} className="flex items-center gap-2 tabular-nums">
              <span
                aria-hidden="true"
                className="h-2 w-2 shrink-0 rounded-[2px]"
                style={{ background: color }}
              />
              <span className="whitespace-nowrap text-fg-2">{label}</span>
              <span className="ms-auto whitespace-nowrap ps-3 text-fg-1">
                {value}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
