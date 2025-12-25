import type { LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";

import { cn } from "@/lib/utils";

import { clamp } from "./utils";

export type SummaryCard = {
  key: string;
  label: string;
  value: string;
  bg: string;
  text: string;
  accent: string;
  Icon: LucideIcon;
};

export type UsageStat = {
  label: string;
  percent: number;
  subLeft?: string;
  hintRight?: string;
};

type TrendsViewProps = {
  summaryCards: SummaryCard[];
  historyBars: number[];
  historyLabels: {
    start: string;
    mid: string;
    end: string;
  };
  costBreakdown: UsageStat[];
  usageDetails: UsageStat[];
};

export const TrendsView = ({
  summaryCards,
  historyBars,
  historyLabels,
  costBreakdown,
  usageDetails,
}: TrendsViewProps) => {
  const { t } = useTranslation();
  const historyMax = Math.max(...historyBars, 1);

  return (
    <div className="flex flex-col gap-3">
      <div className="grid grid-cols-3 gap-2">
        {summaryCards.map(({ key, label, value, bg, text, accent, Icon }) => (
          <div key={key} className={cn("p-2.5 rounded-xl border", bg)}>
            <Icon className={cn("w-4 h-4 mb-1", accent)} />
            <div
              className={cn("text-[15px] leading-5 font-semibold tabular-nums", text)}
            >
              {value}
            </div>
            <div className={cn("text-[12px] leading-4", accent)}>{label}</div>
          </div>
        ))}
      </div>

      <div className="p-3 bg-white border rounded-xl border-slate-200">
        <span className="text-[12px] leading-4 font-medium text-slate-900 mb-2.5 block">
          {t("tray.history.title", { defaultValue: "成本趋势（30 天）" })}
        </span>
        {historyBars.length === 0 ? (
          <div className="h-[110px] flex items-center justify-center text-sm text-slate-500">
            {t("tray.history.empty", { defaultValue: "暂无用量数据" })}
          </div>
        ) : (
          <div className="flex items-end gap-px h-[110px] mb-2">
            {historyBars.map((value, index) => (
              <div
                key={`${value}-${index}`}
                className="flex-1 transition-all rounded-t-sm bg-gradient-to-t from-blue-500 to-blue-300 hover:from-blue-600 hover:to-blue-400"
                style={{
                  height: `${Math.max(12, (value / historyMax) * 100)}%`,
                }}
              />
            ))}
          </div>
        )}
        <div className="flex items-center justify-between text-[12px] leading-4 text-slate-500">
          <span>{historyLabels.start || "Day 1"}</span>
          <span>{historyLabels.mid || "Day 15"}</span>
          <span>{historyLabels.end || "Day 30"}</span>
        </div>
      </div>

      <div className="p-3 bg-white border rounded-xl border-slate-200">
        <span className="text-[12px] leading-4 font-medium text-slate-900 mb-2.5 block">
          {t("tray.trends.costBreakdown", { defaultValue: "成本分布" })}
        </span>
        {costBreakdown.length === 0 ? (
          <p className="text-[12px] text-slate-500">
            {t("tray.history.empty", { defaultValue: "暂无用量数据" })}
          </p>
        ) : (
          <div className="flex flex-col gap-2">
            {costBreakdown.map((item) => (
              <div key={item.label}>
                <div className="flex items-center justify-between mb-1.5">
                  <span className="text-[12px] leading-4 text-slate-600">
                    {item.label}
                  </span>
                  <span className="text-[12px] leading-4 font-medium text-slate-900 tabular-nums">
                    {item.subLeft || `${item.percent}%`}
                  </span>
                </div>
                <div className="h-1.5 bg-slate-100 rounded-full overflow-hidden">
                  <div
                    className="h-full bg-blue-500 rounded-full"
                    style={{ width: `${clamp(item.percent)}%` }}
                  />
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="p-3 bg-white border rounded-xl border-slate-200">
        <span className="text-[12px] leading-4 font-medium text-slate-900 mb-2.5 block">
          {t("tray.trends.currentUsage", { defaultValue: "当前使用量" })}
        </span>
        {usageDetails.length === 0 ? (
          <p className="text-[12px] text-slate-500">
            {t("tray.usage.notConfigured", {
              defaultValue: "尚未配置用量脚本",
            })}
          </p>
        ) : (
          <div className="flex flex-col gap-2.5">
            {usageDetails.map((item) => (
              <div key={item.label}>
                <div className="flex items-center justify-between mb-1.5">
                  <span className="text-[12px] leading-4 text-slate-600">
                    {item.label}
                  </span>
                  <span className="text-[12px] leading-4 font-medium text-slate-900 tabular-nums">
                    {item.percent.toFixed(1)}%
                  </span>
                </div>
                <div className="h-1.5 bg-slate-100 rounded-full overflow-hidden">
                  <div
                    className="h-full bg-green-500 rounded-full"
                    style={{ width: `${clamp(item.percent)}%` }}
                  />
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
};
