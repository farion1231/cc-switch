import { useTranslation } from "react-i18next";
import { ChevronRight, Layers, Plug, Route, Shuffle } from "lucide-react";
import type { AppMode } from "@/types/proxy";
import { Switch } from "@/components/ui/switch";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

const MODE_ICON = { direct: Plug, route: Route, stack: Layers } as const;

/** 模式色只用在 tab、侧栏标签、当前卡片和激活条的 CTA 上（B4.7）。 */
export const MODE_TONE: Record<
  AppMode,
  { soft: string; text: string; dot: string; border: string }
> = {
  direct: {
    soft: "bg-direct-soft",
    text: "text-direct-text",
    dot: "bg-direct",
    border: "border-direct",
  },
  route: {
    soft: "bg-route-soft",
    text: "text-route-text",
    dot: "bg-route",
    border: "border-route",
  },
  stack: {
    soft: "bg-stack-soft",
    text: "text-stack-text",
    dot: "bg-stack",
    border: "border-stack",
  },
};

interface ModeTabsProps {
  modes: AppMode[];
  active: AppMode;
  view: AppMode;
  onView: (mode: AppMode) => void;
  /** 状态行：「前导 + 值」，空间不够时先截前导，地址和供应商名保持完整 */
  status?: { lead: string; value: string };
  failover?: {
    enabled: boolean;
    disabled?: boolean;
    onChange: (enabled: boolean) => void;
  };
  onOpenRouteSettings?: () => void;
}

/**
 * 模式行：三段 tab 只负责查看；生效的那段用模式色的淡底和圆点，正在看的那段是白色凸起。
 */
export function ModeTabs({
  modes,
  active,
  view,
  onView,
  status,
  failover,
  onOpenRouteSettings,
}: ModeTabsProps) {
  const { t } = useTranslation();

  return (
    <div className="flex min-h-12 shrink-0 items-center gap-4 px-6 pt-3">
      <div
        role="group"
        aria-label={t("mode.tabsLabel")}
        className="inline-flex h-9 shrink-0 items-center gap-0.5 rounded-[10px] bg-subtle p-[3px]"
      >
        {modes.map((mode) => {
          const Icon = MODE_ICON[mode];
          const isActive = mode === active;
          const selected = mode === view;
          return (
            <button
              key={mode}
              type="button"
              aria-pressed={selected}
              onClick={() => onView(mode)}
              title={
                isActive
                  ? t("mode.activeTitle", { mode: t(`mode.names.${mode}`) })
                  : undefined
              }
              className={cn(
                "inline-flex h-[30px] min-w-[96px] items-center justify-center gap-1.5 rounded-[7px] px-4 text-body transition-[background-color,color,box-shadow] duration-150",
                isActive
                  ? cn(MODE_TONE[mode].soft, MODE_TONE[mode].text)
                  : selected
                    ? "bg-surface text-fg-1"
                    : "text-fg-2 hover:text-fg-1",
                selected && "shadow-v7-sm",
                selected || isActive ? "font-semibold" : "font-medium",
              )}
            >
              <Icon
                className="h-3.5 w-3.5"
                strokeWidth={2}
                aria-hidden="true"
              />
              {t(`mode.names.${mode}`)}
              {isActive && (
                <span
                  aria-label={t("mode.active")}
                  role="img"
                  className={cn(
                    "h-1.5 w-1.5 rounded-full",
                    MODE_TONE[mode].dot,
                  )}
                />
              )}
            </button>
          );
        })}
      </div>

      {status && (status.lead || status.value) && (
        <div className="flex min-w-0 flex-1 items-center gap-1 text-caption text-fg-2">
          {status.lead && <span className="truncate">{status.lead}</span>}
          <span className="shrink-0 truncate">{status.value}</span>
        </div>
      )}
      {!status && <div className="flex-1" />}

      {failover && (
        <label className="flex shrink-0 items-center gap-2 text-body text-fg-1">
          <Shuffle
            className="h-4 w-4 text-fg-2"
            strokeWidth={1.5}
            aria-hidden="true"
          />
          {t("mode.failover")}
          <Switch
            checked={failover.enabled}
            disabled={failover.disabled}
            onCheckedChange={failover.onChange}
            aria-label={t("mode.failover")}
          />
        </label>
      )}
      {onOpenRouteSettings && (
        <Button
          variant="quiet"
          size="compact"
          className="shrink-0"
          onClick={onOpenRouteSettings}
        >
          {t("mode.routeSettings")}
          <ChevronRight className="h-3.5 w-3.5" />
        </Button>
      )}
    </div>
  );
}
