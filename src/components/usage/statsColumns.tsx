import { useTranslation } from "react-i18next";
import { HelpTip } from "@/components/ui/help-tip";
import { cn } from "@/lib/utils";
import { formatTokensPerSecond, getAggregateTokensPerSecond } from "./format";
import { usageTable } from "./usageTable";

/** 供应商 / 模型统计行共有的速度分子分母（后端只累加满足条件的明细请求）。 */
export interface SpeedTotals {
  speedOutputTokens?: number;
  speedGenerationMs?: number;
  estSpeedOutputTokens?: number;
  estSpeedDurationMs?: number;
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

/** 「成功率」「速度」两列的表头，供应商和模型统计共用。 */
export function SuccessSpeedHeaders() {
  const { t } = useTranslation();
  return (
    <>
      <th className={usageTable.thEnd}>{t("usage.successRate")}</th>
      <th className={usageTable.thEnd}>
        <span className="inline-flex items-center gap-0.5">
          {t("usage.speed")}
          <HelpTip title={t("usage.speedSumHelpTitle")} align="end">
            {t("usage.speedSumHelp")}
          </HelpTip>
        </span>
      </th>
    </>
  );
}

/** 「成功率」「速度」两列的单元格；精确速度缺失时用会话日志的估算值，带 ≈。 */
export function SuccessSpeedCells({
  stat,
}: {
  stat: SpeedTotals & { successRate: number };
}) {
  const exactSpeed = getStatsSpeed(stat);
  const estimatedSpeed =
    exactSpeed == null ? getStatsEstimatedSpeed(stat) : null;
  const speed = exactSpeed ?? estimatedSpeed;
  return (
    <>
      <td className={usageTable.tdEnd}>
        {stat.successRate.toFixed(stat.successRate >= 99.95 ? 0 : 1)}%
      </td>
      <td className={cn(usageTable.tdEnd, speed == null && usageTable.muted)}>
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
    </>
  );
}
