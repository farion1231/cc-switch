import { useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { AppId } from "@/lib/api";
import type { VisibleApps } from "@/types";
import { ProviderIcon } from "@/components/ProviderIcon";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { cn } from "@/lib/utils";
import { Monitor, MoreHorizontal, Plus, Terminal, X } from "lucide-react";
import { APP_IDS } from "@/config/appConfig";

const APP_BADGE_ICON: Partial<
  Record<AppId, { icon: typeof Terminal; offsetY?: number }>
> = {
  claude: { icon: Terminal },
  "claude-desktop": { icon: Monitor, offsetY: 0.5 },
};

interface AppSwitcherProps {
  activeApp: AppId;
  onSwitch: (app: AppId) => void;
  visibleApps?: VisibleApps;
  /** hover ×：就地隐藏一个应用，与设置页「主页面显示」同一开关 */
  onHideApp?: (app: AppId) => void;
  /** 末尾「+」：把隐藏的应用加回主页面 */
  onShowApp?: (app: AppId) => void;
}

const STORAGE_KEY = "cc-switch-last-app";

const APP_ICON_NAME: Record<AppId, string> = {
  claude: "claude",
  "claude-desktop": "claude",
  codex: "openai",
  gemini: "gemini",
  grokbuild: "grok",
  opencode: "opencode",
  openclaw: "openclaw",
  hermes: "hermes",
  pi: "pi",
};

const APP_DISPLAY_NAME: Record<AppId, string> = {
  claude: "Claude Code",
  "claude-desktop": "Claude Desktop",
  codex: "Codex",
  gemini: "Gemini",
  grokbuild: "Grok Build",
  opencode: "OpenCode",
  openclaw: "OpenClaw",
  hermes: "Hermes",
  pi: "Pi",
};

/** 应用图标 + 角标（Claude Code / Desktop 用角标区分终端与桌面） */
function AppGlyph({ app, isActive }: { app: AppId; isActive: boolean }) {
  const badgeConfig = APP_BADGE_ICON[app];
  const BadgeIcon = badgeConfig?.icon;
  return (
    <span className="relative inline-flex shrink-0">
      <ProviderIcon
        icon={APP_ICON_NAME[app]}
        name={APP_DISPLAY_NAME[app]}
        size={20}
      />
      {BadgeIcon && (
        <span
          className={cn(
            "absolute -bottom-0.5 -right-0.5 flex items-center justify-center rounded-[3px] border h-[11px] w-[11px]",
            isActive
              ? "bg-background border-border text-foreground"
              : "bg-muted border-background text-muted-foreground group-hover:bg-background group-hover:text-foreground",
          )}
          aria-hidden="true"
        >
          <BadgeIcon
            className="h-[8px] w-[8px]"
            strokeWidth={2.5}
            style={
              badgeConfig?.offsetY
                ? { transform: `translateY(${badgeConfig.offsetY}px)` }
                : undefined
            }
          />
        </span>
      )}
    </span>
  );
}

export function AppSwitcher({
  activeApp,
  onSwitch,
  visibleApps,
  onHideApp,
  onShowApp,
}: AppSwitcherProps) {
  const { t } = useTranslation();
  const rootRef = useRef<HTMLDivElement>(null);
  const [moreOpen, setMoreOpen] = useState(false);
  const [addOpen, setAddOpen] = useState(false);

  const handleSwitch = (app: AppId) => {
    if (app === activeApp) return;
    localStorage.setItem(STORAGE_KEY, app);
    onSwitch(app);
  };

  // Filter apps based on visibility settings (default all visible)
  const appsToShow = APP_IDS.filter((app) => {
    if (!visibleApps) return true;
    return visibleApps[app];
  });
  const appCount = appsToShow.length;
  // 隐藏的应用（「+」的候选）；visibleApps 未加载时视为全部可见
  const hiddenApps = visibleApps
    ? APP_IDS.filter((app) => !visibleApps[app])
    : [];
  // 与设置页同一护栏：只剩一个可见应用时不可再隐藏，否则没有任何 tab 可点
  const canHide = appsToShow.length > 1 && onHideApp !== undefined;

  const [visibleCount, setVisibleCount] = useState(appCount);

  // 宽度必须取父弹性槽而非自身：自身宽度随可见数量变化，
  // 用它做输入会形成收起→变窄→再收起的反馈循环
  useLayoutEffect(() => {
    const root = rootRef.current;
    const slot = root?.parentElement;
    if (!root || !slot) return;

    const compute = () => {
      const sample = root.querySelector("button");
      if (!sample) return;
      const itemWidth = sample.offsetWidth;
      // jsdom 或未完成布局时 offsetWidth 为 0，保持全部可见
      if (itemWidth <= 0) return;
      const rootStyle = window.getComputedStyle(root);
      const gap = parseFloat(rootStyle.columnGap) || 0;
      const padding =
        (parseFloat(rootStyle.paddingLeft) || 0) +
        (parseFloat(rootStyle.paddingRight) || 0);
      const available = slot.clientWidth;
      // 末尾「+」常驻且与 tab 同宽，全部放下时也要占一个槽位
      const addSlots = onShowApp ? 1 : 0;
      const widthAll =
        padding +
        (appCount + addSlots) * itemWidth +
        (appCount - 1 + addSlots) * gap;
      if (widthAll <= available) {
        setVisibleCount(appCount);
        return;
      }
      // 溢出时「更多」与「+」各占一个与 tab 等宽等距的槽位
      // （同 padding + 同尺寸图标），先扣掉再算能放下几个 tab
      const reservedSlots = 1 + addSlots;
      const fit = Math.floor(
        (available - padding - reservedSlots * (itemWidth + gap)) /
          (itemWidth + gap),
      );
      setVisibleCount(Math.max(1, Math.min(appCount - 1, fit)));
    };

    compute();
    const observer = new ResizeObserver(compute);
    observer.observe(slot);
    return () => observer.disconnect();
  }, [appCount, onShowApp]);

  const visibleList = appsToShow.slice(0, Math.max(1, visibleCount));
  // 激活应用被收进溢出区时，顶替最后一个可见位，保证始终可点亮
  if (appsToShow.includes(activeApp) && !visibleList.includes(activeApp)) {
    visibleList[visibleList.length - 1] = activeApp;
  }
  const overflowList = appsToShow.filter((app) => !visibleList.includes(app));

  return (
    <div
      ref={rootRef}
      className="inline-flex bg-muted rounded-xl p-1 gap-1"
      style={{ WebkitAppRegion: "no-drag" } as any}
    >
      {visibleList.map((app) => {
        const isActive = activeApp === app;
        return (
          <div key={app} className="group relative">
            <button
              type="button"
              onClick={() => handleSwitch(app)}
              title={APP_DISPLAY_NAME[app]}
              aria-label={APP_DISPLAY_NAME[app]}
              className={cn(
                "inline-flex items-center px-3 h-8 rounded-md text-sm font-medium transition-all duration-200",
                isActive
                  ? "bg-background text-foreground shadow-sm"
                  : "text-muted-foreground hover:text-foreground hover:bg-background/50",
              )}
            >
              <AppGlyph app={app} isActive={isActive} />
            </button>
            {canHide && (
              <button
                type="button"
                title={t("appSwitcher.hide")}
                aria-label={`${t("appSwitcher.hide")}: ${APP_DISPLAY_NAME[app]}`}
                onClick={(event) => {
                  event.stopPropagation();
                  onHideApp?.(app);
                }}
                className={cn(
                  "absolute -top-1.5 -right-1 z-10 flex h-3.5 w-3.5 items-center justify-center",
                  "rounded-full border border-border bg-background text-muted-foreground shadow-sm",
                  "opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 hover:text-foreground",
                )}
              >
                <X
                  aria-hidden="true"
                  className="h-[9px] w-[9px]"
                  strokeWidth={2.5}
                />
              </button>
            )}
          </div>
        );
      })}
      {overflowList.length > 0 && (
        <Popover open={moreOpen} onOpenChange={setMoreOpen}>
          <PopoverTrigger asChild>
            <button
              type="button"
              title={t("appSwitcher.more")}
              aria-label={t("appSwitcher.more")}
              className={cn(
                "inline-flex items-center px-3 h-8 rounded-md transition-all duration-200",
                moreOpen
                  ? "bg-background text-foreground shadow-sm"
                  : "text-muted-foreground hover:text-foreground hover:bg-background/50",
              )}
            >
              <MoreHorizontal size={20} className="shrink-0" />
            </button>
          </PopoverTrigger>
          <PopoverContent
            side="bottom"
            align="end"
            sideOffset={6}
            className="z-[100] w-56 p-1"
          >
            {overflowList.map((app) => (
              <button
                key={app}
                type="button"
                onClick={() => {
                  setMoreOpen(false);
                  handleSwitch(app);
                }}
                className="group flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-sm font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
              >
                <AppGlyph app={app} isActive={false} />
                <span className="truncate">{APP_DISPLAY_NAME[app]}</span>
              </button>
            ))}
          </PopoverContent>
        </Popover>
      )}
      {onShowApp && (
        <Popover open={addOpen} onOpenChange={setAddOpen}>
          <PopoverTrigger asChild>
            <button
              type="button"
              title={t("appSwitcher.add")}
              aria-label={t("appSwitcher.add")}
              className={cn(
                "inline-flex items-center px-3 h-8 rounded-md transition-all duration-200",
                addOpen
                  ? "bg-background text-foreground shadow-sm"
                  : "text-muted-foreground hover:text-foreground hover:bg-background/50",
              )}
            >
              <Plus size={20} className="shrink-0" />
            </button>
          </PopoverTrigger>
          <PopoverContent
            side="bottom"
            align="end"
            sideOffset={6}
            className="z-[100] w-56 p-1"
          >
            {hiddenApps.length === 0 ? (
              <p className="px-2.5 py-2 text-sm text-muted-foreground">
                {t("appSwitcher.allShown")}
              </p>
            ) : (
              hiddenApps.map((app) => (
                <button
                  key={app}
                  type="button"
                  onClick={() => onShowApp(app)}
                  className="group flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-sm font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                >
                  <AppGlyph app={app} isActive={false} />
                  <span className="truncate">{APP_DISPLAY_NAME[app]}</span>
                </button>
              ))
            )}
          </PopoverContent>
        </Popover>
      )}
    </div>
  );
}
