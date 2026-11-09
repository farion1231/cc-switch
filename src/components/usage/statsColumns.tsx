import { useTranslation } from "react-i18next";
import { HelpTip } from "@/components/ui/help-tip";
import { cn } from "@/lib/utils";
import {
  formatFirstTokenSeconds,
  formatTokensPerSecond,
  getAggregateTokensPerSecond,
} from "./format";
import { usageTable } from "./usageTable";

/** 供应商 / 模型统计行共有的速度分子分母与首字中位数（后端只统计满足条件的明细请求）。 */
export interface SpeedTotals {
  speedOutputTokens?: number;
  speedGenerationMs?: number;
  estSpeedOutputTokens?: number;
  estSpeedDurationMs?: number;
  firstTokenMedianMs?: number | null;
  estFirstTokenMedianMs?: number | null;
}

/** 一行的汇总速度：Σ输出 ÷ Σ生成时间。 */
export function getStatsSpeed(stat: SpeedTotals): string | null {
  return formatTokensPerSecond(
    getAggregateTokensPerSecond(stat.speedOutputTokens, stat.speedGenerationMs),
  );
}

/**
 * 会话日志导入的请求的汇总估算速度：Σ输出 ÷ Σ估算耗时（含首字等待）。
 * 没有精确速度时才拿它顶上，显示时前面带 ≈。
 */
export function getStatsEstimatedSpeed(stat: SpeedTotals): string | null {
  return formatTokensPerSecond(
    getAggregateTokensPerSecond(
      stat.estSpeedOutputTokens,
      stat.estSpeedDurationMs,
    ),
  );
}

/** 「成功率」「速度」「首字」三列的表头，供应商和模型统计共用；成功率是性能组的第一列，前面多留组间距。 */
export function PerformanceHeaders() {
  const { t } = useTranslation();
  return (
    <>
      <th className={cn(usageTable.th, usageTable.groupGap)}>
        {t("usage.successRate")}
      </th>
      <th className={usageTable.th}>
        <span className="inline-flex items-center gap-0.5">
          {t("usage.speed")}
          <HelpTip title={t("usage.speedSumHelpTitle")} align="end">
            {t("usage.speedSumHelp")}
          </HelpTip>
        </span>
      </th>
      <th className={usageTable.th}>
        <span className="inline-flex items-center gap-0.5">
          {t("usage.firstToken")}
          <HelpTip title={t("usage.firstTokenSumHelpTitle")} align="end">
            {t("usage.firstTokenSumHelp")}
          </HelpTip>
        </span>
      </th>
    </>
  );
}

/** 「成功率」「速度」「首字」三列的单元格；精确值缺失时用会话日志的估算值，带 ≈。 */
export function PerformanceCells({
  stat,
}: {
  stat: SpeedTotals & { successRate: number };
}) {
  const exactSpeed = getStatsSpeed(stat);
  const estimatedSpeed =
    exactSpeed == null ? getStatsEstimatedSpeed(stat) : null;
  const speed = exactSpeed ?? estimatedSpeed;
  const exactFirstToken = formatFirstTokenSeconds(stat.firstTokenMedianMs);
  const estimatedFirstToken =
    exactFirstToken == null
      ? formatFirstTokenSeconds(stat.estFirstTokenMedianMs)
      : null;
  const firstToken = exactFirstToken ?? estimatedFirstToken;
  return (
    <>
      <td className={cn(usageTable.td, usageTable.groupGap)}>
        {stat.successRate.toFixed(stat.successRate >= 99.95 ? 0 : 1)}%
      </td>
      <td className={cn(usageTable.td, speed == null && usageTable.muted)}>
        {speed == null ? (
          "—"
        ) : (
          <>
            {estimatedSpeed != null && "≈"}
            {speed}
            <span className="ms-0.5 text-badge font-normal text-fg-3">
              tok/s
            </span>
          </>
        )}
      </td>
      <td className={cn(usageTable.td, firstToken == null && usageTable.muted)}>
        {estimatedFirstToken != null && "≈"}
        {firstToken ?? "—"}
      </td>
    </>
  );
}
