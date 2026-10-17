import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { listen } from "@tauri-apps/api/event";
import i18n from "i18next";
import {
  AlertTriangle,
  Check,
  ChevronLeft,
  ChevronRight,
  ExternalLink,
  Feather,
  Plus,
  Power,
  Search,
  Settings,
} from "lucide-react";
import { AppGlyph } from "@/components/shell/AppGlyph";
import { ProviderIcon } from "@/components/ProviderIcon";
import { cn } from "@/lib/utils";
import {
  CHANGED_EVENT,
  SHOW_EVENT,
  trayPanelApi,
  type CardRect,
  type PanelApp,
  type PanelPlacement,
  type PanelQuota,
  type PanelRow,
  type PanelTexts,
  type TrayPanelState,
} from "./api";
import { usePanelTheme } from "./usePanelTheme";
import { UsageTab } from "./UsageTab";

type Tab = "config" | "usage";
const TAB_KEY = "cc-switch-tray-tab";
/** 主界面保存语言用的键 */
const LANGUAGE_KEY = "language";
const MAIN_MAX_HEIGHT = 620;
/** 使用固定画布时，窗口在一次弹出里的最大高度 */
const CANVAS_MAX_HEIGHT = 760;
/** 分开摆时窗口四周留的透明边距，给卡片的阴影（系统窗口阴影已关掉） */
const SHADOW_MARGIN = 16;
const SUB_MAX_HEIGHT = 540;
/** 悬停多久展开子面板；已经展开时换一个应用多等一会儿，斜着移向子面板时不误切 */
const HOVER_OPEN_MS = 90;
const HOVER_SWITCH_MS = 180;

const ROW_HOVER = "hover:bg-black/[0.06] dark:hover:bg-white/[0.08]";
const FOCUS_RING =
  "outline-none focus-visible:ring-2 focus-visible:ring-blue-500/60";

function readTab(): Tab {
  try {
    return window.localStorage.getItem(TAB_KEY) === "usage"
      ? "usage"
      : "config";
  } catch {
    return "config";
  }
}

function syncLanguage() {
  try {
    const language = window.localStorage.getItem(LANGUAGE_KEY);
    if (language && language !== i18n.language) {
      void i18n.changeLanguage(language);
    }
  } catch {
    // 读不到沿用当前语言
  }
}

/** 分开摆时窗口的纵向范围（相对贴图标那条边）。 */
function floatingCanvas(p: PanelPlacement) {
  return p.edge === "top"
    ? { top: 0, bottom: Math.min(p.boundsBottom, CANVAS_MAX_HEIGHT) }
    : { top: Math.max(p.boundsTop, -CANVAS_MAX_HEIGHT), bottom: 0 };
}

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), Math.max(min, max));
}

// ─── 额度 ────────────────────────────────────────────────────────────────────

/** 重置时间写短：今天只写时刻，其余写月/日 + 时刻；已经过了的不写。 */
function formatResetTime(iso: string, now: Date): string | null {
  const at = new Date(iso);
  if (Number.isNaN(at.getTime()) || at <= now) return null;
  const time = `${String(at.getHours()).padStart(2, "0")}:${String(at.getMinutes()).padStart(2, "0")}`;
  return at.toDateString() === now.toDateString()
    ? time
    : `${at.getMonth() + 1}/${at.getDate()} ${time}`;
}

function QuotaBlock({
  quota,
  texts,
  compact = false,
}: {
  quota: PanelQuota;
  texts: PanelTexts;
  /** 子面板里：最多两档 */
  compact?: boolean;
}) {
  if (quota.lines.length === 0 && !quota.note && quota.resetCredits == null) {
    return null;
  }
  const now = new Date();
  return (
    <div className={cn("flex flex-col gap-1", compact ? "mt-1" : "mt-1.5")}>
      {quota.lines.slice(0, compact ? 2 : 3).map((line, index) => {
        const when = line.resetsAt ? formatResetTime(line.resetsAt, now) : null;
        return (
          <div key={index} className="flex items-center gap-2">
            <span
              className={cn(
                "shrink-0 text-[11px] tabular-nums",
                line.warn ? "text-amber-500" : "text-muted-foreground",
              )}
            >
              {line.text}
            </span>
            {line.left != null && (
              <span className="h-1 min-w-6 flex-1 overflow-hidden rounded-full bg-black/10 dark:bg-white/10">
                <span
                  className={cn(
                    "block h-full rounded-full",
                    line.left <= 0
                      ? "bg-red-500"
                      : line.warn
                        ? "bg-amber-500"
                        : "bg-blue-500",
                  )}
                  style={{ width: `${Math.max(line.left, 2)}%` }}
                />
              </span>
            )}
            {when && (
              <span className="shrink-0 text-[10.5px] tabular-nums text-muted-foreground">
                {texts.resetsAt.replace("{when}", when)}
              </span>
            )}
          </div>
        );
      })}
      {quota.resetCredits != null && quota.resetCredits > 0 && (
        <span className="text-[11px] text-muted-foreground">
          {texts.resetCredits.replace("{count}", String(quota.resetCredits))}
        </span>
      )}
      {quota.note && (
        <span className="text-[11px] text-amber-500">{quota.note}</span>
      )}
    </div>
  );
}

// ─── 主面板的应用行 ───────────────────────────────────────────────────────────

function AppRow({
  app,
  texts,
  active,
  side,
  onHover,
  onLeave,
  onOpen,
}: {
  app: PanelApp;
  texts: PanelTexts;
  active: boolean;
  side: "left" | "right";
  onHover: () => void;
  onLeave: () => void;
  onOpen: () => void;
}) {
  const Chevron = side === "left" ? ChevronLeft : ChevronRight;
  const subtitle = app.addId
    ? app.addText
    : [app.current, app.mode].filter(Boolean).join(" · ");
  return (
    <button
      type="button"
      data-app-row={app.app}
      data-nav="app"
      onMouseEnter={onHover}
      onMouseLeave={onLeave}
      onClick={onOpen}
      className={cn(
        "flex w-full items-start gap-3 rounded-[10px] px-2.5 py-2 text-left transition-colors",
        FOCUS_RING,
        active ? "bg-blue-500/[0.16]" : ROW_HOVER,
      )}
    >
      <span className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-black/[0.05] dark:bg-white/[0.08]">
        <AppGlyph app={app.app} size={20} badgeClassName="bg-transparent" />
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-1.5">
          <span
            className={cn(
              "truncate text-[13.5px] font-semibold",
              active && "text-blue-500",
            )}
          >
            {app.name}
          </span>
          {app.attention && (
            <span className="shrink-0 rounded bg-amber-500/15 px-1 text-[10px] font-medium text-amber-600 dark:text-amber-400">
              {app.attention}
            </span>
          )}
        </span>
        <span className="block truncate text-xs text-muted-foreground">
          {subtitle}
        </span>
        {app.quota && <QuotaBlock quota={app.quota} texts={texts} />}
      </span>
      {app.addId ? (
        <Plus className="mt-2 h-4 w-4 shrink-0 text-muted-foreground" />
      ) : (
        <Chevron className="mt-2 h-4 w-4 shrink-0 text-muted-foreground" />
      )}
    </button>
  );
}

// ─── 子面板 ───────────────────────────────────────────────────────────────────

function matches(row: PanelRow, query: string) {
  const haystack = [row.text, row.provider?.name, row.provider?.hint]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
  return haystack.includes(query);
}

function SubRow({
  row,
  texts,
  onAction,
}: {
  row: PanelRow;
  texts: PanelTexts;
  onAction: (id: string) => void;
}) {
  if (row.kind === "separator") {
    return (
      <div className="mx-2 my-1 h-px bg-black/[0.08] dark:bg-white/[0.08]" />
    );
  }
  if (row.kind === "label") {
    return (
      <div className="px-3 py-1 text-[11px] text-muted-foreground">
        {row.text}
      </div>
    );
  }
  const provider = row.provider;
  const hint =
    provider?.hint && !row.text.includes(provider.hint) ? provider.hint : null;
  return (
    <button
      type="button"
      data-nav="sub"
      disabled={!row.enabled}
      onClick={() => onAction(row.id)}
      className={cn(
        "flex w-full gap-2.5 rounded-lg px-2.5 py-1.5 text-left transition-colors",
        // 带额度的行高，图标和「使用中」贴着第一行
        provider?.quota ? "items-start" : "items-center",
        FOCUS_RING,
        row.enabled ? ROW_HOVER : "cursor-default opacity-55",
      )}
    >
      <span className="flex h-6 w-6 shrink-0 items-center justify-center">
        {row.checked ? (
          <Check className="h-4 w-4 text-blue-500" strokeWidth={2.5} />
        ) : provider ? (
          <ProviderIcon
            icon={provider.icon ?? undefined}
            name={provider.name}
            color={provider.iconColor ?? undefined}
            size={18}
          />
        ) : null}
      </span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[13px] font-medium">
          {row.text}
        </span>
        {hint && (
          <span className="block truncate text-[11px] text-muted-foreground">
            {hint}
          </span>
        )}
        {provider?.quota && (
          <QuotaBlock quota={provider.quota} texts={texts} compact />
        )}
      </span>
      {row.checked && provider && (
        <span className="shrink-0 text-[11px] font-medium text-blue-500">
          {texts.inUse}
        </span>
      )}
    </button>
  );
}

function SubPanel({
  app,
  texts,
  query,
  onQuery,
  onAction,
  searchRef,
}: {
  app: PanelApp;
  texts: PanelTexts;
  query: string;
  onQuery: (value: string) => void;
  onAction: (id: string) => void;
  searchRef: React.RefObject<HTMLInputElement>;
}) {
  const needle = query.trim().toLowerCase();
  // 额度说明已经写在主面板的应用行下面
  const rows = app.rows.filter((row) => !row.id.endsWith(":quota"));
  const shown = needle
    ? rows.filter((row) => row.kind === "provider" && matches(row, needle))
    : rows;
  const providerCount = rows.filter((row) => row.kind === "provider").length;

  return (
    <>
      <div className="shrink-0 px-4 pb-2 pt-3">
        <div className="truncate text-[14px] font-semibold">{app.name}</div>
        <div className="truncate text-xs text-muted-foreground">
          {app.header ?? texts.switchProvider}
        </div>
      </div>
      {providerCount > 4 && (
        <div className="shrink-0 px-3 pb-2">
          <label className="flex items-center gap-2 rounded-lg bg-black/[0.05] px-2.5 py-1.5 dark:bg-white/[0.08]">
            <Search className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
            <input
              ref={searchRef}
              value={query}
              onChange={(event) => onQuery(event.target.value)}
              placeholder={texts.searchPlaceholder}
              className="min-w-0 flex-1 bg-transparent text-[13px] outline-none placeholder:text-muted-foreground"
            />
          </label>
        </div>
      )}
      <div className="tp-scroll min-h-0 flex-1 px-1.5 pb-1.5">
        {shown.length === 0 ? (
          <div className="px-3 py-4 text-center text-xs text-muted-foreground">
            {texts.noMatch}
          </div>
        ) : (
          shown.map((row, index) => (
            <SubRow
              key={`${row.id}-${index}`}
              row={row}
              texts={texts}
              onAction={onAction}
            />
          ))
        )}
      </div>
    </>
  );
}

function FooterButton({
  icon,
  text,
  title,
  onClick,
  className,
}: {
  icon: React.ReactNode;
  text: string;
  title?: string;
  onClick: () => void;
  className?: string;
}) {
  return (
    <button
      type="button"
      title={title}
      onClick={onClick}
      className={cn(
        "flex items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left text-[13px] transition-colors",
        FOCUS_RING,
        ROW_HOVER,
        className,
      )}
    >
      <span className="flex h-5 w-5 shrink-0 items-center justify-center text-muted-foreground">
        {icon}
      </span>
      <span className="truncate">{text}</span>
    </button>
  );
}

// ─── 面板 ─────────────────────────────────────────────────────────────────────

export function TrayPanel() {
  const [placement, setPlacement] = useState<PanelPlacement | null>(null);
  const [state, setState] = useState<TrayPanelState | null>(null);
  const [tab, setTab] = useState<Tab>(readTab);
  const [openApp, setOpenApp] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [shownCount, setShownCount] = useState(0);
  const theme = usePanelTheme(shownCount);

  const mainRef = useRef<HTMLDivElement>(null);
  const subRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const hoverTimer = useRef<number | undefined>(undefined);
  const layoutFrame = useRef<number | undefined>(undefined);
  const lastLayout = useRef("");
  const placementRef = useRef(placement);
  placementRef.current = placement;

  const load = useCallback(async () => {
    try {
      setState(await trayPanelApi.state());
    } catch (error) {
      console.error("[TrayPanel] 读取托盘面板数据失败", error);
    }
  }, []);

  // 每次弹出：收起子面板、清空搜索、先取数据再排版，显示出来就是新内容
  const begin = useCallback(
    async (next: PanelPlacement) => {
      syncLanguage();
      setOpenApp(null);
      setQuery("");
      await load();
      lastLayout.current = "";
      setShownCount((count) => count + 1);
      setPlacement(next);
    },
    [load],
  );

  useEffect(() => {
    let disposed = false;
    const offs: Array<() => void> = [];
    void (async () => {
      const offShow = await listen<PanelPlacement>(SHOW_EVENT, (event) => {
        void begin(event.payload);
      });
      const offChanged = await listen(CHANGED_EVENT, () => void load());
      if (disposed) {
        offShow();
        offChanged();
        return;
      }
      offs.push(offShow, offChanged);
      // 第一次弹出时窗口是新建的，事件可能早于这里开始监听
      const pending = await trayPanelApi.placement();
      if (pending && !disposed) void begin(pending);
    })();
    return () => {
      disposed = true;
      offs.forEach((off) => off());
    };
  }, [begin, load]);

  useEffect(() => {
    if (!placement) return;
    document.documentElement.dataset.glass = placement.glass;
    document.documentElement.dataset.layout = placement.layout;
  }, [placement]);

  const apps = state?.apps ?? [];
  const openEntry =
    tab === "config"
      ? apps.find((app) => app.app === openApp && !app.addId)
      : undefined;

  /**
   * 量出两块卡片的高度，算好它们在窗口里的位置，回报给后端摆窗口和玻璃。
   * 直接改 DOM 样式（不经 state），量和摆在同一帧里完成。
   */
  const relayout = useCallback(() => {
    const p = placementRef.current;
    const main = mainRef.current;
    if (!p || !main) return;
    const sub = subRef.current;
    const floating = p.layout === "floating";
    const windowsJoined = p.platform === "windows" && !floating;
    const fixedCanvas = floating || windowsJoined;

    main.style.minHeight = "";
    if (sub) sub.style.minHeight = "";
    let mainHeight = main.offsetHeight;
    let subHeight = sub ? sub.offsetHeight : 0;
    if (sub && !floating) {
      // 两栏：一样高，整窗一块背景不留空
      const height = Math.max(mainHeight, subHeight);
      main.style.minHeight = sub.style.minHeight = `${height}px`;
      mainHeight = subHeight = height;
    }

    const mainTop = p.edge === "top" ? 0 : -mainHeight;
    let subX = p.side === "left" ? 0 : p.mainWidth + p.gap;
    let subTop = mainTop;
    let top: number;
    let bottom: number;
    let mainX: number;
    let width: number;
    if (fixedCanvas) {
      // macOS 和 Windows 在这次弹出期间固定窗口范围，不让展开 / 切换子面板
      // 触发 WebView resize。Windows 的原生窗口区域只露出实际的卡片，其余区域可点击穿透。
      const canvas = floatingCanvas(p);
      top = canvas.top;
      bottom = canvas.bottom;
      mainX = p.side === "left" ? p.subWidth + p.gap : 0;
      width = p.mainWidth + p.gap + p.subWidth;
      if (sub && floating) {
        // 子面板顶边对齐展开的那一行
        const row = openApp
          ? main.querySelector<HTMLElement>(`[data-app-row="${openApp}"]`)
          : null;
        const rowOffset = row
          ? row.getBoundingClientRect().top - main.getBoundingClientRect().top
          : 0;
        subTop = clamp(mainTop + rowOffset - 6, top, bottom - subHeight);
      }
    } else {
      // 两栏（整窗一块背景）：窗口跟着卡片走，不留空
      top = mainTop;
      bottom = mainTop + mainHeight;
      mainX = sub && p.side === "left" ? p.subWidth + p.gap : 0;
      width = sub ? p.mainWidth + p.gap + p.subWidth : p.mainWidth;
    }
    if (floating) {
      top -= SHADOW_MARGIN;
      bottom += SHADOW_MARGIN;
      mainX += SHADOW_MARGIN;
      subX += SHADOW_MARGIN;
      width += SHADOW_MARGIN * 2;
    }

    main.style.left = `${mainX}px`;
    main.style.top = `${mainTop - top}px`;
    const cards: CardRect[] = [
      { x: mainX, y: mainTop - top, width: p.mainWidth, height: mainHeight },
    ];
    if (sub) {
      sub.style.left = `${subX}px`;
      sub.style.top = `${subTop - top}px`;
      cards.push({
        x: subX,
        y: subTop - top,
        width: p.subWidth,
        height: subHeight,
      });
    }

    const layout = {
      seq: p.seq,
      width,
      height: Math.ceil(bottom - top),
      offsetX: mainX,
      offsetY: top,
      cards,
      dark: theme.explicit,
    };
    const key = JSON.stringify(layout);
    if (key === lastLayout.current) return;
    const firstLayout = lastLayout.current === "";
    lastLayout.current = key;
    if (layoutFrame.current !== undefined) {
      window.cancelAnimationFrame(layoutFrame.current);
    }
    layoutFrame.current = undefined;
    if (windowsJoined && !firstLayout) {
      // 先让新列表绘制到固定画布，再扩展原生可见区域，避免先露出未绘制的底色。
      layoutFrame.current = window.requestAnimationFrame(() => {
        layoutFrame.current = window.requestAnimationFrame(() => {
          layoutFrame.current = undefined;
          void trayPanelApi.layout(layout);
        });
      });
    } else {
      void trayPanelApi.layout(layout);
    }
  }, [openApp, theme.explicit]);

  useEffect(
    () => () => {
      if (layoutFrame.current !== undefined) {
        window.cancelAnimationFrame(layoutFrame.current);
      }
    },
    [],
  );

  useLayoutEffect(() => {
    relayout();
  });

  useEffect(() => {
    const observer = new ResizeObserver(() => relayout());
    if (mainRef.current) observer.observe(mainRef.current);
    if (subRef.current) observer.observe(subRef.current);
    return () => observer.disconnect();
  }, [relayout, openEntry, placement]);

  const action = useCallback((id: string) => {
    void trayPanelApi.action(id);
  }, []);

  const openSub = useCallback((app: PanelApp | null) => {
    window.clearTimeout(hoverTimer.current);
    setOpenApp(app && !app.addId ? app.app : null);
    setQuery("");
  }, []);

  const hoverApp = (app: PanelApp) => {
    window.clearTimeout(hoverTimer.current);
    if (openApp === app.app) return;
    hoverTimer.current = window.setTimeout(
      () => openSub(app),
      openApp ? HOVER_SWITCH_MS : HOVER_OPEN_MS,
    );
  };

  const changeTab = (next: Tab) => {
    setTab(next);
    setOpenApp(null);
    try {
      window.localStorage.setItem(TAB_KEY, next);
    } catch {
      // 记不住也不影响使用
    }
  };

  // 键盘：Esc 逐级退出；← → 收起 / 展开子面板；↑ ↓ 在行之间移动
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const inInput = target?.tagName === "INPUT";
      if (event.key === "Escape") {
        event.preventDefault();
        if (query) setQuery("");
        else if (openApp) openSub(null);
        else void trayPanelApi.hide();
        return;
      }
      if (inInput && event.key !== "ArrowDown") return;
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        const scope = target?.closest(".tp-card") ?? mainRef.current;
        const items = Array.from(
          scope?.querySelectorAll<HTMLElement>("[data-nav]:not([disabled])") ??
            [],
        );
        if (items.length === 0) return;
        event.preventDefault();
        const index = items.indexOf(target as HTMLElement);
        const step = event.key === "ArrowDown" ? 1 : -1;
        const next =
          index < 0
            ? step > 0
              ? 0
              : items.length - 1
            : (index + step + items.length) % items.length;
        items[next]?.focus();
        return;
      }
      const forward =
        placementRef.current?.side === "left" ? "ArrowLeft" : "ArrowRight";
      const back = forward === "ArrowLeft" ? "ArrowRight" : "ArrowLeft";
      if (event.key === forward && target?.dataset.appRow) {
        const app = apps.find((entry) => entry.app === target.dataset.appRow);
        if (app && !app.addId) {
          event.preventDefault();
          openSub(app);
          window.setTimeout(() => {
            const first =
              searchRef.current ??
              subRef.current?.querySelector<HTMLElement>(
                "[data-nav]:not([disabled])",
              );
            first?.focus();
          }, 0);
        }
      } else if (
        event.key === back &&
        openApp &&
        subRef.current?.contains(target)
      ) {
        event.preventDefault();
        const row = mainRef.current?.querySelector<HTMLElement>(
          `[data-app-row="${openApp}"]`,
        );
        openSub(null);
        row?.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [apps, openApp, openSub, query]);

  const radius = placement?.radius ?? 12;
  const joined = placement?.layout === "joined";
  const cornerStyle = (card: "main" | "sub"): CSSProperties => {
    if (!joined || !openEntry) return { borderRadius: radius };
    const mainOnLeft = placement?.side !== "left";
    const leftCard = mainOnLeft ? card === "main" : card === "sub";
    return {
      borderRadius: leftCard
        ? `${radius}px 0 0 ${radius}px`
        : `0 ${radius}px ${radius}px 0`,
    };
  };

  const mainMaxHeight = useMemo(() => {
    if (!placement) return MAIN_MAX_HEIGHT;
    const room =
      placement.edge === "top" ? placement.boundsBottom : -placement.boundsTop;
    return Math.max(200, Math.min(MAIN_MAX_HEIGHT, room));
  }, [placement]);
  const subMaxHeight = useMemo(() => {
    if (!placement) return SUB_MAX_HEIGHT;
    const room =
      placement.layout === "floating"
        ? floatingCanvas(placement).bottom - floatingCanvas(placement).top
        : placement.boundsBottom - placement.boundsTop;
    return Math.max(200, Math.min(SUB_MAX_HEIGHT, room));
  }, [placement]);

  if (!state || !placement) return null;
  const { texts } = state;

  return (
    <div
      className="relative h-full w-full"
      data-platform={placement.platform}
      onMouseDown={(event) => {
        // 点到两块卡片之外的透明处：先收起子面板，再点一次收起面板
        if (event.target !== event.currentTarget) return;
        if (openApp) openSub(null);
        else void trayPanelApi.hide();
      }}
    >
      <div
        ref={mainRef}
        className="tp-card"
        style={{
          width: placement.mainWidth,
          maxHeight: mainMaxHeight,
          ...cornerStyle("main"),
        }}
      >
        <div className="flex shrink-0 items-center px-4 pb-2 pt-3">
          <span className="text-[15px] font-semibold">CC Switch</span>
          <button
            type="button"
            title={texts.settings}
            aria-label={texts.settings}
            onClick={() => action("nav:settings:general")}
            className={cn(
              "ms-auto flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground",
              FOCUS_RING,
              ROW_HOVER,
            )}
          >
            <Settings className="h-4 w-4" />
          </button>
        </div>

        <div className="shrink-0 px-3 pb-2">
          <div
            role="tablist"
            className="flex rounded-lg bg-black/[0.06] p-0.5 dark:bg-white/[0.08]"
          >
            {(["config", "usage"] as const).map((value) => (
              <button
                key={value}
                type="button"
                role="tab"
                aria-selected={tab === value}
                onClick={() => changeTab(value)}
                className={cn(
                  "flex-1 rounded-md py-1 text-xs font-medium transition-colors",
                  FOCUS_RING,
                  tab === value
                    ? "bg-white text-foreground shadow-sm dark:bg-white/[0.16]"
                    : "text-muted-foreground hover:text-foreground",
                )}
              >
                {value === "config" ? texts.configTab : texts.usageTab}
              </button>
            ))}
          </div>
        </div>

        <div className="tp-scroll min-h-0 flex-1" onScroll={() => relayout()}>
          {tab === "config" ? (
            <div className="flex flex-col gap-0.5 px-1.5 pb-1.5">
              {state.problems.map((problem) => (
                <button
                  key={problem.id}
                  type="button"
                  onClick={() => action(problem.id)}
                  className={cn(
                    "flex items-start gap-2 rounded-lg px-2.5 py-1.5 text-left text-xs text-amber-600 dark:text-amber-400",
                    FOCUS_RING,
                    ROW_HOVER,
                  )}
                >
                  <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                  <span>{problem.text}</span>
                </button>
              ))}
              {state.feedback && (
                <div className="px-2.5 py-1 text-[11.5px] text-muted-foreground">
                  {state.feedback}
                </div>
              )}
              {apps.map((app) => (
                <AppRow
                  key={app.app}
                  app={app}
                  texts={texts}
                  active={openEntry?.app === app.app}
                  side={placement.side}
                  onHover={() => hoverApp(app)}
                  onLeave={() => window.clearTimeout(hoverTimer.current)}
                  onOpen={() =>
                    app.addId
                      ? action(app.addId)
                      : openSub(openApp === app.app ? null : app)
                  }
                />
              ))}
            </div>
          ) : (
            <UsageTab refreshKey={shownCount} visible={tab === "usage"} />
          )}
        </div>

        <div className="shrink-0 border-t border-black/[0.08] px-1.5 py-1.5 dark:border-white/[0.08]">
          <button
            type="button"
            role="switch"
            aria-checked={state.lightweight}
            onClick={() => action("lightweight_mode")}
            className={cn(
              "flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left text-[13px]",
              FOCUS_RING,
              ROW_HOVER,
            )}
          >
            <span className="flex h-5 w-5 shrink-0 items-center justify-center text-muted-foreground">
              <Feather className="h-4 w-4" />
            </span>
            <span className="flex-1">{texts.lightweightMode}</span>
            <span
              className={cn(
                "relative h-[18px] w-8 shrink-0 rounded-full transition-colors",
                state.lightweight
                  ? "bg-blue-500"
                  : "bg-black/15 dark:bg-white/20",
              )}
            >
              <span
                className={cn(
                  "absolute top-[2px] h-[14px] w-[14px] rounded-full bg-white shadow transition-[left]",
                  state.lightweight ? "left-[16px]" : "left-[2px]",
                )}
              />
            </span>
          </button>
          <div className="flex">
            <FooterButton
              className="flex-1"
              icon={<ExternalLink className="h-4 w-4" />}
              text={texts.openMain}
              onClick={() => action("show_main")}
            />
            <FooterButton
              icon={<Power className="h-4 w-4" />}
              text={texts.quit}
              title={state.quitHint ?? undefined}
              onClick={() => action("quit")}
            />
          </div>
        </div>
      </div>

      {openEntry && (
        <div
          ref={subRef}
          className="tp-card tp-sub"
          data-side={placement.side}
          onMouseEnter={() => window.clearTimeout(hoverTimer.current)}
          style={{
            width: placement.subWidth,
            maxHeight: subMaxHeight,
            ...cornerStyle("sub"),
          }}
        >
          <SubPanel
            app={openEntry}
            texts={texts}
            query={query}
            onQuery={setQuery}
            onAction={action}
            searchRef={searchRef}
          />
        </div>
      )}
    </div>
  );
}
