import { useTranslation } from "react-i18next";
import { Layers, Plug, Route, Settings2, Shuffle } from "lucide-react";
import type { AppMode } from "@/types/proxy";
import { Switch } from "@/components/ui/switch";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { HoverTip } from "@/components/ui/hover-tip";
import {
  SegmentThumb,
  useSlidingIndicator,
} from "@/components/ui/sliding-indicator";
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
  /** 状态行：「前导 + 值」，空间不够先截前导；值仅在独自超出整栏时才截断 */
  status?: { lead: string; value: string };
  failover?: {
    enabled: boolean;
    disabled?: boolean;
    onChange: (enabled: boolean) => void;
  };
  onOpenRouteSettings?: () => void;
}

/**
 * 模式行：三段 tab 只负责查看。白底凸起只给正在看的那段；生效的那段只用模式色文字和圆点，
 * 不铺底（否则彩色那格比正在看的更抢眼）。组右边的「?」说明三种模式的区别。
 * 模式 tab 保留文字、不挂提示；右侧的故障转移、路由设置是纯图标 + HoverTip。
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
  const indicator = useSlidingIndicator<HTMLDivElement>(
    '[aria-pressed="true"]',
    view,
  );

  return (
    <div className="flex min-h-12 shrink-0 items-center gap-4 px-6 pt-3">
      <div
        ref={indicator.ref}
        role="group"
        aria-label={t("mode.tabsLabel")}
        className="relative inline-flex h-9 shrink-0 items-center gap-0.5 rounded-[10px] bg-subtle p-[3px]"
      >
        <SegmentThumb
          rect={indicator.rect}
          animate={indicator.animate}
          className="rounded-[7px]"
        />
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
              className={cn(
                "relative inline-flex h-[30px] min-w-[96px] items-center justify-center gap-1.5 rounded-[7px] px-4 text-body transition-colors duration-150",
                isActive
                  ? MODE_TONE[mode].text
                  : selected
                    ? "text-fg-1"
                    : "text-fg-2 hover:text-fg-1",
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
      <HelpTip title={t("mode.help.title")} className="-ms-2">
        {modes.map((mode) => (
          <span key={mode} className="block">
            {t(`mode.help.${mode}`)}
          </span>
        ))}
      </HelpTip>

      {status && (status.lead || status.value) && (
        <div className="flex min-w-0 flex-1 items-center gap-1 text-caption text-fg-2">
          {status.lead && <span className="truncate">{status.lead}</span>}
          <span className="max-w-full shrink-0 truncate" title={status.value}>
            {status.value}
          </span>
        </div>
      )}
      {!status && <div className="flex-1" />}

      {failover && (
        <HoverTip content={t("mode.failover")}>
          <label className="flex shrink-0 cursor-pointer items-center gap-1.5">
            <Shuffle
              className="h-4 w-4 text-fg-2"
              strokeWidth={1.5}
              aria-hidden="true"
            />
            <Switch
              checked={failover.enabled}
              disabled={failover.disabled}
              onCheckedChange={failover.onChange}
              aria-label={t("mode.failover")}
            />
          </label>
        </HoverTip>
      )}
      {onOpenRouteSettings && (
        <HoverTip content={t("mode.routeSettings")}>
          <Button
            variant="quiet"
            size="compact"
            className="h-8 w-8 shrink-0 px-0"
            aria-label={t("mode.routeSettings")}
            onClick={onOpenRouteSettings}
          >
            <Settings2 className="h-4 w-4" strokeWidth={1.75} />
          </Button>
        </HoverTip>
      )}
    </div>
  );
}
