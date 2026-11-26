import { useTranslation } from "react-i18next";
import { SegmentedControl } from "@/components/ui/segmented-control";

export type UsageBreakdown = "total" | "model" | "app";

export function UsageBreakdownControl({
  value,
  onValueChange,
  canBreakDownByModel,
}: {
  value: UsageBreakdown;
  onValueChange: (value: UsageBreakdown) => void;
  canBreakDownByModel: boolean;
}) {
  const { t } = useTranslation();
  return (
    <SegmentedControl<UsageBreakdown>
      aria-label={t("usage.trend.breakdown")}
      size="sm"
      items={[
        { value: "total", label: t("common.all") },
        {
          value: "model",
          label: t("usage.trend.byModel"),
          disabled: !canBreakDownByModel,
        },
        { value: "app", label: t("usage.trend.byApp") },
      ]}
      value={value}
      onValueChange={onValueChange}
    />
  );
}
