import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { usageApi } from "@/lib/api";
import { resolveUsageRange } from "@/lib/usageRange";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import { formatTokensCompact } from "@/components/usage/format";
import { SettingsRow } from "@/components/settings/SettingsLayout";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { SettingsFormState } from "@/hooks/useSettings";

type TrayUsageTitleMode =
  | NonNullable<SettingsFormState["trayUsageTitle"]>
  | "off";

const MODES: { value: TrayUsageTitleMode; labelKey: string }[] = [
  { value: "off", labelKey: "settings.general.trayUsageTitleOff" },
  {
    value: "todayTokens",
    labelKey: "settings.general.trayUsageTitleTodayTokens",
  },
  { value: "todayCost", labelKey: "settings.general.trayUsageTitleTodayCost" },
  {
    value: "totalTokens",
    labelKey: "settings.general.trayUsageTitleTotalTokens",
  },
  { value: "totalCost", labelKey: "settings.general.trayUsageTitleTotalCost" },
];

/**
 * 设置 → 通用里「菜单栏显示用量」一行（仅 macOS）：下拉选数字维度，
 * 旁边一颗实时预览胶囊，写的就是菜单栏图标左侧那串数字的样子。
 * 数字口径与托盘一致：今日＝本地 0 点起，总＝全部记录。
 */
export function TrayUsageTitleRow({
  settings,
  onAutoSave,
}: {
  settings: SettingsFormState;
  onAutoSave: (updates: Partial<SettingsFormState>) => Promise<boolean>;
}) {
  const { t, i18n } = useTranslation();
  const { money } = useMoneyFormat();
  const mode: TrayUsageTitleMode = settings.trayUsageTitle ?? "off";
  const isTotal = mode === "totalTokens" || mode === "totalCost";

  const { startDate, endDate } = useMemo(
    () => resolveUsageRange({ preset: isTotal ? "all" : "today" }),
    [isTotal],
  );
  const { data: summary } = useQuery({
    queryKey: ["tray-usage-title-preview", isTotal],
    queryFn: () => usageApi.getUsageSummary(startDate, endDate),
    enabled: mode !== "off",
    staleTime: 60_000,
  });

  const preview =
    summary &&
    (mode.endsWith("Tokens")
      ? formatTokensCompact(summary.realTotalTokens, i18n.language)
      : money(summary.totalCost, 2));

  return (
    <SettingsRow
      label={t("settings.general.trayUsageTitle")}
      help={{
        title: t("settings.general.trayUsageTitle"),
        body: t("settings.general.trayUsageTitleHelp"),
      }}
      control={
        <div className="flex items-center gap-2">
          {preview && (
            <span
              aria-hidden
              className="inline-flex items-center rounded-md border border-border bg-subtle px-1.5 py-0.5 text-[11px] font-medium tabular-nums text-fg-2"
            >
              {preview}
            </span>
          )}
          <Select
            value={mode}
            onValueChange={(value) =>
              void onAutoSave({
                trayUsageTitle:
                  value === "off"
                    ? undefined
                    : (value as SettingsFormState["trayUsageTitle"]),
              })
            }
          >
            <SelectTrigger
              className="h-8 w-[160px]"
              aria-label={t("settings.general.trayUsageTitle")}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {MODES.map((item) => (
                <SelectItem key={item.value} value={item.value}>
                  {t(item.labelKey)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      }
    />
  );
}
