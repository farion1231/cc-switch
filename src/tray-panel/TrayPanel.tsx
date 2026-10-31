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
import { useQueryClient } from "@tanstack/react-query";
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
import type { AppMode } from "@/types/proxy";
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
import { aimingAt, type Point } from "./menuAim";
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
/** 悬停多久展开子面板；已经展开时换应用不等，只有鼠标正朝子面板移动才暂缓 */
const HOVER_OPEN_MS = 50;
/** 正朝子面板移动时隔这么久再看一次：鼠标停下了或偏离了就切换 */
const AIM_RECHECK_MS = 120;
/** 鼠标停在主面板里不是应用行的项上（`hovered` 的取值） */
const OTHER_ITEM = "";

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
function floatingCanvas(p: PanelPlacement, maxHeight = CANVAS_MAX_HEIGHT) {
  return p.edge === "top"
    ? { top: 0, bottom: Math.min(p.boundsBottom, maxHeight) }
    : { top: Math.max(p.boundsTop, -maxHeight), bottom: 0 };
}

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), Math.max(min, max));
}

// ─── 额度 ────────────────────────────────────────────────────────────────────

/**
 * 还有多久重置，写短：最多两级（天+小时、小时+分、分），零的那级不写；
 * 不足一分钟按一分钟算，已经过了的不写。
 */
export function formatResetIn(
  iso: string,
  now: Date,
  texts: PanelTexts,
): string | null {
  const at = new Date(iso);
  if (Number.isNaN(at.getTime()) || at <= now) return null;
  const totalMinutes = Math.max(
    1,
    Math.floor((at.getTime() - now.getTime()) / 60_000),
  );
  const days = Math.floor(totalMinutes / 1440);
  const hours = Math.floor((totalMinutes % 1440) / 60);
  const minutes = totalMinutes % 60;
  const unit = (template: string, n: number) =>
    template.replace("{n}", String(n));
  const parts =
    days > 0
      ? [
          unit(texts.durationDays, days),
          hours > 0 && unit(texts.durationHours, hours),
        ]
      : hours > 0
        ? [
            unit(texts.durationHours, hours),
            minutes > 0 && unit(texts.durationMinutes, minutes),
          ]
        : [unit(texts.durationMinutes, minutes)];
  return texts.resetsIn.replace(
    "{when}",
    parts.filter(Boolean).join(texts.durationSep),
  );
}

/** 只给数值上绿色（「余额 82.10 CNY」里的金额），和主界面 `TONE_TEXT.normal` 同色。 */
function EmphasizedText({
  text,
  emphasis,
}: {
  text: string;
  emphasis: string | null;
}) {
  const at = emphasis ? text.lastIndexOf(emphasis) : -1;
  if (!emphasis || at < 0) return <>{text}</>;
  return (
    <>
      {text.slice(0, at)}
      <span className="text-green-600 dark:text-green-400">{emphasis}</span>
      {text.slice(at + emphasis.length)}
    </>
  );
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
  const lines = quota.lines.slice(0, compact ? 2 : 3).map((line) => ({
    ...line,
    when: line.resetsAt ? formatResetIn(line.resetsAt, now, texts) : null,
  }));
  // 各档共用一张网格（文字 | 进度条 | 多久重置），进度条的起止才对得齐；
  // 整块都没有重置时间时不留第三列，免得右边空出一截间距。
  const hasWhen = lines.some((line) => line.when);
  return (
    <div
      className={cn(
        "grid items-center gap-x-2 gap-y-1",
        hasWhen
          ? "grid-cols-[auto_minmax(1.5rem,1fr)_auto]"
          : "grid-cols-[auto_minmax(1.5rem,1fr)]",
        compact ? "mt-1" : "mt-1.5",
      )}
    >
      {lines.map((line, index) => {
        return (
          <div key={index} className="contents">
            <span
              className={cn(
                "whitespace-nowrap text-[11px] tabular-nums",
                line.warn ? "text-amber-500" : "text-muted-foreground",
              )}
            >
              <EmphasizedText text={line.text} emphasis={line.emphasis} />
            </span>
            {line.left == null ? (
              <span />
            ) : (
              <span className="h-1 overflow-hidden rounded-full bg-black/10 dark:bg-white/10">
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
            {hasWhen && (
              <span className="justify-self-end whitespace-nowrap text-[10.5px] tabular-nums text-muted-foreground">
                {line.when}
              </span>
            )}
          </div>
        );
      })}
      {quota.resetCredits != null && quota.resetCredits > 0 && (
        <span className="col-span-full text-[11px] text-muted-foreground">
          {texts.resetCredits.replace("{count}", String(quota.resetCredits))}
        </span>
      )}
      {quota.note && (
        <span className="col-span-full text-[11px] text-amber-500">
          {quota.note}
        </span>
      )}
    </div>
  );
}

// ─── 主面板的应用行 ───────────────────────────────────────────────────────────

// 模式名用主界面的模式色（与 ModeTabs 的 MODE_TONE.text 同一组 token），一眼分出直连 / 路由 / 聚合。
const MODE_TEXT: Record<AppMode, string> = {
  direct: "text-direct-text",
  route: "text-route-text",
  stack: "text-stack-text",
};

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
  return (
    <button
      type="button"
      data-app-row={app.app}
      data-nav="app"
      onMouseEnter={onHover}
      onMouseLeave={onLeave}
      onClick={onOpen}
      className={cn(
        // 图标 | 名称 | 箭头；额度另起一行，从名称列一直铺到箭头下方，进度条才不被箭头那一列挤短
        "grid w-full grid-cols-[auto_minmax(0,1fr)_auto] items-start gap-x-3 rounded-[10px] px-2.5 py-2 text-left",
        FOCUS_RING,
        active ? "bg-blue-500/[0.16]" : ROW_HOVER,
      )}
    >
      <span className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-black/[0.05] dark:bg-white/[0.08]">
        <AppGlyph app={app.app} size={20} badgeClassName="bg-transparent" />
      </span>
      <span className="min-w-0">
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
          {app.addId ? (
            app.addText
          ) : (
            <>
              {app.current && `${app.current} · `}
              <span className={cn("font-medium", MODE_TEXT[app.modeKind])}>
                {app.mode}
              </span>
            </>
          )}
        </span>
      </span>
      {app.addId ? (
        <Plus className="mt-2 h-4 w-4 shrink-0 text-muted-foreground" />
      ) : (
        <Chevron className="mt-2 h-4 w-4 shrink-0 text-muted-foreground" />
      )}
      {app.quota && (
        <span className="col-span-2 col-start-2 min-w-0">
          <QuotaBlock quota={app.quota} texts={texts} />
        </span>
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
  const inUse = row.checked && provider != null;
  return (
    <button
      type="button"
      data-nav="sub"
      disabled={!row.enabled}
      onClick={() => onAction(row.id)}
      className={cn(
        // 图标 | 名称 | 使用中；额度另起一行铺到「使用中」下方，免得被那一列挤窄
        "grid w-full gap-x-2.5 rounded-lg px-2.5 py-1.5 text-left",
        inUse
          ? "grid-cols-[auto_minmax(0,1fr)_auto]"
          : "grid-cols-[auto_minmax(0,1fr)]",
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
      <span className="min-w-0">
        <span className="block truncate text-[13px] font-medium">
          {row.text}
        </span>
        {hint && (
          <span className="block truncate text-[11px] text-muted-foreground">
            {hint}
          </span>
        )}
      </span>
      {inUse && (
        <span className="text-[11px] font-medium text-blue-500">
          {texts.inUse}
        </span>
      )}
      {provider?.quota && (
        <span className="min-w-0 [grid-column:2/-1]">
          <QuotaBlock quota={provider.quota} texts={texts} compact />
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
        "flex min-w-0 items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left text-[13px]",
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
  const [nativePlacement, setPlacement] = useState<PanelPlacement | null>(null);
  const [pixelRatio, setPixelRatio] = useState(window.devicePixelRatio || 1);
  // Windows 文本大小 / WebView 缩放可能让 CSS 像素和屏幕逻辑像素不再一一对应。
  const coordinateScale =
    nativePlacement?.platform === "windows"
      ? pixelRatio / nativePlacement.scaleFactor
      : 1;
  const placement = useMemo(
    () =>
      nativePlacement && {
        ...nativePlacement,
        boundsTop: nativePlacement.boundsTop / coordinateScale,
        boundsBottom: nativePlacement.boundsBottom / coordinateScale,
        mainWidth: nativePlacement.mainWidth / coordinateScale,
        subWidth: nativePlacement.subWidth / coordinateScale,
        gap: nativePlacement.gap / coordinateScale,
        radius: nativePlacement.radius / coordinateScale,
      },
    [nativePlacement, coordinateScale],
  );
  const [state, setState] = useState<TrayPanelState | null>(null);
  const [tab, setTab] = useState<Tab>(readTab);
  const [openApp, setOpenApp] = useState<string | null>(null);
  /** 子面板是从另一个应用换过来的（不是刚展开）：内容做一次很短的淡入 */
  const [swapped, setSwapped] = useState(false);
  const [query, setQuery] = useState("");
  const [shownCount, setShownCount] = useState(0);
  const theme = usePanelTheme(shownCount);
  const queryClient = useQueryClient();

  const mainRef = useRef<HTMLDivElement>(null);
  const subRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const hoverTimer = useRef<number | undefined>(undefined);
  /** 最近几次鼠标位置，判断是不是正朝子面板移动 */
  const trail = useRef<Point[]>([]);
  /** 上次因为「正朝子面板移动」而暂缓切换时的鼠标位置 */
  const aimedAt = useRef<Point | null>(null);
  const hovered = useRef<string | null>(null);
  const lastState = useRef("");
  const layoutFrame = useRef<number | undefined>(undefined);
  const lastLayout = useRef("");
  const placementRef = useRef(placement);
  placementRef.current = placement;
  const openAppRef = useRef(openApp);
  openAppRef.current = openApp;

  useEffect(() => {
    const update = () => setPixelRatio(window.devicePixelRatio || 1);
    // 跨屏时 DPR 可能晚于显示事件更新，固定画布也不一定触发 resize。
    // 每次 DPR 变化后重新订阅，才能继续检测下一次跨屏或 WebView 缩放。
    const media = window.matchMedia?.(`(resolution: ${pixelRatio}dppx)`);
    media?.addEventListener("change", update);
    window.addEventListener("resize", update);
    update();
    return () => {
      media?.removeEventListener("change", update);
      window.removeEventListener("resize", update);
    };
  }, [pixelRatio]);

  const load = useCallback(async () => {
    try {
      const next = await trayPanelApi.state();
      // 额度刷新会连着通知好几次，内容没变就不重新渲染
      const key = JSON.stringify(next);
      if (key === lastState.current) return;
      lastState.current = key;
      setState(next);
    } catch (error) {
      console.error("[TrayPanel] 读取托盘面板数据失败", error);
    }
  }, []);

  // 每次弹出：收起子面板、清空搜索、先取数据再排版，显示出来就是新内容。
  // 设置（币种、趋势图样式）也重新读：面板窗口只隐藏不销毁，主窗口改了设置这边收不到。
  const begin = useCallback(
    async (next: PanelPlacement) => {
      syncLanguage();
      setOpenApp(null);
      setQuery("");
      await Promise.all([
        load(),
        queryClient.invalidateQueries({ queryKey: ["settings"] }),
      ]);
      lastLayout.current = "";
      setShownCount((count) => count + 1);
      setPixelRatio(window.devicePixelRatio || 1);
      setPlacement(next);
    },
    [load, queryClient],
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

    const mainHeight = main.getBoundingClientRect().height;
    if (sub) {
      // 两栏以主面板的高度为准，供应商列表在子面板内部滚动。
      // 底部弹出时不能让子面板撑高主面板，否则应用行会移到鼠标下面，反复触发切换。
      sub.style.height = floating ? "" : `${mainHeight}px`;
    }
    const subHeight = sub ? sub.getBoundingClientRect().height : 0;

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
      const canvas = floatingCanvas(p, CANVAS_MAX_HEIGHT / coordinateScale);
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
      width: width * coordinateScale,
      height: Math.ceil((bottom - top) * coordinateScale),
      offsetX: mainX * coordinateScale,
      offsetY: top * coordinateScale,
      cards: cards.map((card) => ({
        x: card.x * coordinateScale,
        y: card.y * coordinateScale,
        width: card.width * coordinateScale,
        height: card.height * coordinateScale,
      })),
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
  }, [openApp, theme.explicit, coordinateScale]);

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
    const next = app && !app.addId ? app.app : null;
    const previous = openAppRef.current;
    setSwapped(previous !== null && next !== null && previous !== next);
    setOpenApp(next);
    setQuery("");
  }, []);

  useEffect(() => {
    const onMove = (event: MouseEvent) => {
      const points = trail.current;
      points.push({ x: event.clientX, y: event.clientY });
      if (points.length > 3) points.shift();
    };
    window.addEventListener("mousemove", onMove);
    return () => window.removeEventListener("mousemove", onMove);
  }, []);

  /**
   * 子面板开着时换到另一个应用（`null`＝移到了主面板里别的项上，收起子面板）：
   * 鼠标正斜着朝子面板移动（途经别的行）就先不换，隔一会儿再看；鼠标停下了、
   * 偏离了，或本来就是竖着移，立即切换。
   */
  const switchTo = (app: PanelApp | null) => {
    if (hovered.current !== (app?.app ?? OTHER_ITEM)) return;
    const points = trail.current;
    const current = points[points.length - 1];
    const sub = subRef.current?.getBoundingClientRect();
    const side = placementRef.current?.side ?? "right";
    const stopped =
      aimedAt.current !== null &&
      current !== undefined &&
      aimedAt.current.x === current.x &&
      aimedAt.current.y === current.y;
    if (
      !stopped &&
      sub &&
      points.length >= 2 &&
      aimingAt(points[0], current, sub, side)
    ) {
      aimedAt.current = current;
      hoverTimer.current = window.setTimeout(
        () => switchTo(app),
        AIM_RECHECK_MS,
      );
      return;
    }
    aimedAt.current = null;
    openSub(app);
  };

  const hoverApp = (app: PanelApp) => {
    window.clearTimeout(hoverTimer.current);
    hovered.current = app.app;
    aimedAt.current = null;
    if (openApp === app.app) return;
    if (openApp) {
      switchTo(app);
      return;
    }
    hoverTimer.current = window.setTimeout(() => openSub(app), HOVER_OPEN_MS);
  };

  // 和原生菜单一样：移到主面板里别的项（标签、设置、底部按钮）上就收起子面板；
  // 移出整个面板不收，手滑出去再回来不用重新展开
  const hoverOtherItem = (event: React.MouseEvent) => {
    if (!openApp || hovered.current === OTHER_ITEM) return;
    if (!(event.target as HTMLElement).closest("[data-other-item]")) return;
    window.clearTimeout(hoverTimer.current);
    hovered.current = OTHER_ITEM;
    aimedAt.current = null;
    switchTo(null);
  };

  const leaveApp = () => {
    window.clearTimeout(hoverTimer.current);
    hovered.current = null;
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
    if (!placement) return MAIN_MAX_HEIGHT / coordinateScale;
    const room =
      placement.edge === "top" ? placement.boundsBottom : -placement.boundsTop;
    return Math.max(0, Math.min(MAIN_MAX_HEIGHT / coordinateScale, room));
  }, [placement, coordinateScale]);
  const subMaxHeight = useMemo(() => {
    if (!placement) return SUB_MAX_HEIGHT / coordinateScale;
    const room =
      placement.layout === "floating"
        ? floatingCanvas(placement).bottom - floatingCanvas(placement).top
        : placement.boundsBottom - placement.boundsTop;
    return Math.max(0, Math.min(SUB_MAX_HEIGHT / coordinateScale, room));
  }, [placement, coordinateScale]);

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
        onMouseOver={hoverOtherItem}
        style={{
          width: placement.mainWidth,
          maxHeight: mainMaxHeight,
          ...cornerStyle("main"),
        }}
      >
        <div
          data-other-item
          className="flex shrink-0 items-center px-4 pb-2 pt-3"
        >
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

        <div data-other-item className="shrink-0 px-3 pb-2">
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
                  data-other-item
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
                  onLeave={leaveApp}
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

        <div
          data-other-item
          className="shrink-0 border-t border-black/[0.08] px-1.5 py-1.5 dark:border-white/[0.08]"
        >
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
          data-swap={swapped || undefined}
          onMouseEnter={() => window.clearTimeout(hoverTimer.current)}
          style={{
            width: placement.subWidth,
            maxHeight: joined ? mainMaxHeight : subMaxHeight,
            ...cornerStyle("sub"),
          }}
        >
          <SubPanel
            key={openEntry.app}
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
