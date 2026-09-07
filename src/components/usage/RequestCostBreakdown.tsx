import { useTranslation } from "react-i18next";
import {
  getFreshInputTokens,
  isUnpricedUsage,
  type RequestLog,
} from "@/types/usage";
import { fmtInt, fmtUsd, parseFiniteNumber } from "./format";
import { UsageMetadataDisplay } from "./UsageMetadataDisplay";

export function RequestCostBreakdown({
  log,
  locale,
}: {
  log: RequestLog;
  locale: string;
}) {
  const { t } = useTranslation();
  const unpriced = isUnpricedUsage(log);
  const parts = [
    ["inputTokens", getFreshInputTokens(log), log.inputCostUsd],
    ["outputTokens", log.outputTokens, log.outputCostUsd],
    ["cacheReadTokens", log.cacheReadTokens, log.cacheReadCostUsd],
    ["cacheCreationTokens", log.cacheCreationTokens, log.cacheCreationCostUsd],
  ] as const;
  const multiplier = parseFiniteNumber(log.costMultiplier);
  const subtotal = parts.reduce(
    (sum, [, , cost]) => sum + (parseFiniteNumber(cost) ?? 0),
    0,
  );
  const total = parseFiniteNumber(log.totalCostUsd);
  const breakdownUnavailable =
    !unpriced && total != null && total > 0 && subtotal === 0;
  const reconciles =
    total != null &&
    multiplier != null &&
    parts.every(([, , cost]) => parseFiniteNumber(cost) != null) &&
    Math.abs(subtotal * multiplier - total) < 0.0000005;
  return (
    <div className="space-y-2 min-w-[320px] max-w-[min(560px,90vw)]">
      <div className="font-semibold">
        {t("usage.costBreakdown")} · {log.pricingModel || log.model}
      </div>
      <UsageMetadataDisplay request={log} showSource />
      {!unpriced && !reconciles && (
        <div className="text-muted-foreground">
          {t(
            breakdownUnavailable
              ? "usage.costBreakdownUnavailable"
              : "usage.costBreakdownMismatch",
          )}
        </div>
      )}
      {log.fastPricingUnavailable && (
        <div className="text-amber-600">
          {t("usage.fastPricingUnavailable")}
        </div>
      )}
      <div className="space-y-1 tabular-nums">
        {parts.map(([label, tokens, rawCost]) => {
          const cost = parseFiniteNumber(rawCost);
          const unit =
            tokens > 0 && cost != null && !unpriced && !breakdownUnavailable
              ? (cost * 1_000_000) / tokens
              : null;
          return (
            <div key={label} className="flex justify-between gap-4">
              <span>{t(`usage.${label}`)}</span>
              <span>
                {fmtInt(tokens, locale)} ×{" "}
                {unit == null ? "--" : fmtUsd(unit, 6)}/M ={" "}
                {unpriced || breakdownUnavailable ? "--" : fmtUsd(rawCost, 6)}
              </span>
            </div>
          );
        })}
      </div>
      <div className="border-t pt-2 flex justify-between gap-4 font-medium tabular-nums">
        <span>{t("usage.totalCost")}</span>
        <span>
          {unpriced
            ? t("usage.unpriced")
            : reconciles
              ? `${fmtUsd(subtotal, 6)} × ${multiplier} = ${fmtUsd(log.totalCostUsd, 6)}`
              : fmtUsd(log.totalCostUsd, 6)}
        </span>
      </div>
      <div className="text-muted-foreground">
        {t("usage.costMultiplier")}: ×{multiplier ?? "--"}
      </div>
    </div>
  );
}
