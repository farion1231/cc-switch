import { useTranslation } from "react-i18next";
import { fmtUsd, formatTokensShort } from "./format";

interface UsageTooltipCardProps {
  heading: string;
  tokens: number;
  requests: number;
  /** 美元；null 表示没有可计价的数据 */
  cost: number | null;
}

/** 热力图格子、趋势柱的悬停卡片：同一时间段的 Token、请求数和费用一起看。 */
export function UsageTooltipCard({
  heading,
  tokens,
  requests,
  cost,
}: UsageTooltipCardProps) {
  const { t, i18n } = useTranslation();
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
      value: cost == null ? "--" : fmtUsd(cost, 2),
    },
  ];

  return (
    <div className="min-w-[180px] rounded-[10px] border border-border bg-surface px-4 py-3 shadow-v7-lg">
      <p className="m-0 mb-2 whitespace-nowrap text-body font-semibold text-fg-1">
        {heading}
      </p>
      <div className="grid grid-cols-[auto_1fr] gap-x-6 gap-y-1 text-body">
        {rows.map((row) => (
          <div key={row.label} className="contents">
            <span className="text-fg-2">{row.label}</span>
            <span className="text-end tabular-nums text-fg-1">{row.value}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
