import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import {
  ArrowLeftRight,
  ChartColumn,
  PanelLeft,
  Plus,
  Route,
  Server,
  Sparkles,
  X,
  type LucideIcon,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

/**
 * 界面上给教程指路的锚点：元素上写 `data-tour="<id>"`，教程按 id 找到它、把它圈出来。
 * 同一个 id 有多个元素时只取第一个（供应商卡片就是列表里的第一张）。
 */
export type TourAnchor =
  | "apps"
  | "provider-card"
  | "provider-empty"
  | "add-provider"
  | "mode-tabs"
  | "nav-mcp"
  | "nav-skills"
  | "nav-prompts"
  | "nav-sessions"
  | "nav-auth"
  | "nav-usage"
  | "nav-settings";

interface TourTarget {
  /** 要圈出来的元素组：每个子数组圈成一个独立亮框（避免合并后误圈中间的其他按钮） */
  groups: TourAnchor[][];
  /** 匹配这一组时换用的正文；不给就用步骤的 bodyKeys */
  bodyKeys?: string[];
  /** 匹配这一组时的提示条；不给就用步骤的 calloutKey（传 null 可显式隐藏） */
  calloutKey?: string | null;
}

interface ModeGuideItem {
  mode: "direct" | "route" | "stack";
  titleKey: string;
  descKey: string;
}

interface TourStep {
  id: string;
  icon: LucideIcon;
  titleKey: string;
  /** 正文段落的 i18n key，按顺序各占一段 */
  bodyKeys: string[];
  /** 辅助提示条（放在浅底圆角框里，区分主操作说明和补充提示） */
  calloutKey?: string;
  /** 连接模式步骤的结构化说明项 */
  modeItems?: ModeGuideItem[];
  /**
   * 按顺序找，第一组在界面上的就圈它（比如有供应商时圈卡片，还没有时圈空列表）。
   * 空的或一组都找不到时不圈，卡片居中。
   */
  targets: TourTarget[];
  /** 一组都找不到时的正文（不给就用 bodyKeys） */
  missingBodyKeys?: string[];
  /** 一组都找不到时的提示条（比如当前应用没有模式切换） */
  missingCalloutKey?: string;
}

const MODE_GUIDE_ITEMS: ModeGuideItem[] = [
  {
    mode: "direct",
    titleKey: "tutorial.modes.directTitle",
    descKey: "tutorial.modes.directDesc",
  },
  {
    mode: "route",
    titleKey: "tutorial.modes.routeTitle",
    descKey: "tutorial.modes.routeDesc",
  },
  {
    mode: "stack",
    titleKey: "tutorial.modes.stackTitle",
    descKey: "tutorial.modes.stackDesc",
  },
];

const MODE_DOT_CLASS: Record<ModeGuideItem["mode"], string> = {
  direct: "bg-direct",
  route: "bg-route",
  stack: "bg-stack",
};

export const TOUR_STEPS: TourStep[] = [
  {
    id: "welcome",
    icon: Sparkles,
    titleKey: "firstRunNotice.title",
    bodyKeys: ["firstRunNotice.bodyDefault"],
    calloutKey: "firstRunNotice.bodyBackup",
    targets: [],
  },
  {
    id: "apps",
    icon: PanelLeft,
    titleKey: "tutorial.apps.title",
    bodyKeys: ["tutorial.apps.body"],
    targets: [{ groups: [["apps"]] }],
  },
  {
    id: "providers",
    icon: ArrowLeftRight,
    titleKey: "tutorial.providers.title",
    bodyKeys: ["tutorial.providers.body"],
    calloutKey: "firstRunNotice.bodyOfficial",
    targets: [
      { groups: [["provider-card"]] },
      // 新装、还没有供应商：圈空列表内容区，说明如何添加第一家
      {
        groups: [["provider-empty"]],
        bodyKeys: ["tutorial.providers.empty"],
        calloutKey: null,
      },
    ],
  },
  {
    id: "add",
    icon: Plus,
    titleKey: "tutorial.add.title",
    bodyKeys: ["tutorial.add.body"],
    targets: [{ groups: [["add-provider"]] }],
  },
  {
    id: "modes",
    icon: Route,
    titleKey: "tutorial.modes.title",
    bodyKeys: ["tutorial.modes.body"],
    modeItems: MODE_GUIDE_ITEMS,
    // 只有 Claude Code / Codex / Gemini CLI / Grok Build / Claude Desktop 有这组切换；
    // 其他应用没有时，卡片居中并用提示条说明去哪里找
    targets: [{ groups: [["mode-tabs"]] }],
    missingCalloutKey: "tutorial.modes.unavailable",
  },
  {
    id: "shared",
    icon: Server,
    titleKey: "tutorial.shared.title",
    bodyKeys: ["tutorial.shared.body"],
    targets: [
      {
        groups: [["nav-mcp", "nav-skills", "nav-prompts", "nav-sessions"]],
      },
    ],
  },
  {
    id: "more",
    icon: ChartColumn,
    titleKey: "tutorial.more.title",
    bodyKeys: ["tutorial.more.body"],
    calloutKey: "tutorial.more.replay",
    // 「授权中心 + 用量统计」与右下角「设置」分成两个亮框，避免误圈中间的「应用」按钮
    targets: [{ groups: [["nav-auth", "nav-usage"], ["nav-settings"]] }],
  },
];

interface Rect {
  top: number;
  left: number;
  width: number;
  height: number;
}

interface Viewport {
  width: number;
  height: number;
}

/** 圈的范围比元素本身大一圈 */
const SPOT_PADDING = 6;
/** 卡片和圈之间的距离 */
const CARD_GAP = 14;
/** 卡片离窗口边缘至少留这么多 */
const VIEWPORT_MARGIN = 12;
const CLIPPING_OVERFLOW = new Set(["auto", "scroll", "hidden", "clip"]);

function findAnchor(anchor: TourAnchor): HTMLElement | null {
  return document.querySelector<HTMLElement>(`[data-tour="${anchor}"]`);
}

function readViewport(): Viewport {
  return { width: window.innerWidth, height: window.innerHeight };
}

/** 把元素矩形裁到可滚动 / 裁剪祖先的可见范围内，避免长列表的高亮框溢出到别的区块上 */
function getClippedElementRect(el: HTMLElement): Rect | null {
  const r = el.getBoundingClientRect();
  if (r.width === 0 && r.height === 0) return null;

  let top = r.top;
  let left = r.left;
  let right = r.right;
  let bottom = r.bottom;

  let parent = el.parentElement;
  while (
    parent &&
    parent !== document.body &&
    parent !== document.documentElement
  ) {
    const style = getComputedStyle(parent);
    if (
      CLIPPING_OVERFLOW.has(style.overflowY) ||
      CLIPPING_OVERFLOW.has(style.overflowX)
    ) {
      const pr = parent.getBoundingClientRect();
      if (CLIPPING_OVERFLOW.has(style.overflowY)) {
        top = Math.max(top, pr.top);
        bottom = Math.min(bottom, pr.bottom);
      }
      if (CLIPPING_OVERFLOW.has(style.overflowX)) {
        left = Math.max(left, pr.left);
        right = Math.min(right, pr.right);
      }
    }
    parent = parent.parentElement;
  }

  if (right <= left || bottom <= top) return null;
  return { top, left, width: right - left, height: bottom - top };
}

/** 量出这一组锚点要圈的范围（已裁到窗口内）；一个元素都没找到就是 null */
function measureAnchors(
  anchors: TourAnchor[],
  viewport: Viewport,
): Rect | null {
  let top = Infinity;
  let left = Infinity;
  let right = -Infinity;
  let bottom = -Infinity;

  for (const anchor of anchors) {
    const el = findAnchor(anchor);
    if (!el) continue;
    const r = getClippedElementRect(el);
    if (!r) continue;
    top = Math.min(top, r.top);
    left = Math.min(left, r.left);
    right = Math.max(right, r.left + r.width);
    bottom = Math.max(bottom, r.top + r.height);
  }
  if (top === Infinity) return null;

  top = Math.max(top - SPOT_PADDING, 0);
  left = Math.max(left - SPOT_PADDING, 0);
  right = Math.min(right + SPOT_PADDING, viewport.width);
  bottom = Math.min(bottom + SPOT_PADDING, viewport.height);
  if (right <= left || bottom <= top) return null;
  return { top, left, width: right - left, height: bottom - top };
}

function unionRect(rects: Rect[]): Rect | null {
  if (rects.length === 0) return null;
  let top = Infinity;
  let left = Infinity;
  let right = -Infinity;
  let bottom = -Infinity;
  for (const r of rects) {
    top = Math.min(top, r.top);
    left = Math.min(left, r.left);
    right = Math.max(right, r.left + r.width);
    bottom = Math.max(bottom, r.top + r.height);
  }
  return { top, left, width: right - left, height: bottom - top };
}

interface MeasuredTarget {
  /** 圈的是 targets 里的第几组；-1 表示一组都不在界面上 */
  match: number;
  rects: Rect[];
}

const NO_TARGET: MeasuredTarget = { match: -1, rects: [] };

/** 按顺序找第一组在界面上的目标 */
function measureTargets(
  targets: TourTarget[],
  viewport: Viewport,
): MeasuredTarget {
  for (let i = 0; i < targets.length; i++) {
    const rects = targets[i].groups
      .map((group) => measureAnchors(group, viewport))
      .filter((rect): rect is Rect => rect !== null);
    if (rects.length > 0) return { match: i, rects };
  }
  return NO_TARGET;
}

function sameRects(a: Rect[], b: Rect[]): boolean {
  if (a.length !== b.length) return false;
  return a.every(
    (ra, idx) =>
      Math.abs(ra.top - b[idx].top) < 0.5 &&
      Math.abs(ra.left - b[idx].left) < 0.5 &&
      Math.abs(ra.width - b[idx].width) < 0.5 &&
      Math.abs(ra.height - b[idx].height) < 0.5,
  );
}

const clamp = (value: number, min: number, max: number) =>
  Math.max(min, Math.min(value, Math.max(min, max)));

type CardPlacement =
  | "center"
  | "right"
  | "bottom"
  | "left"
  | "top"
  | "fallback";

interface CardCaret {
  side: "left" | "top" | "right" | "bottom";
  offset: number;
}

interface CardPosition {
  top: number;
  left: number;
  placement: CardPlacement;
  caret: CardCaret | null;
}

function overlapArea(a: Rect, b: Rect): number {
  const x = Math.max(
    0,
    Math.min(a.left + a.width, b.left + b.width) - Math.max(a.left, b.left),
  );
  const y = Math.max(
    0,
    Math.min(a.top + a.height, b.top + b.height) - Math.max(a.top, b.top),
  );
  return x * y;
}

function computeCaret(
  spot: Rect,
  cardPos: { top: number; left: number },
  card: { width: number; height: number },
  placement: CardPlacement,
): CardCaret | null {
  const spotCenterX = spot.left + spot.width / 2;
  const spotCenterY = spot.top + spot.height / 2;
  const edgePadding = 22;

  switch (placement) {
    case "right":
      return {
        side: "left",
        offset: clamp(
          spotCenterY - cardPos.top,
          edgePadding,
          card.height - edgePadding,
        ),
      };
    case "left":
      return {
        side: "right",
        offset: clamp(
          spotCenterY - cardPos.top,
          edgePadding,
          card.height - edgePadding,
        ),
      };
    case "bottom":
      return {
        side: "top",
        offset: clamp(
          spotCenterX - cardPos.left,
          edgePadding,
          card.width - edgePadding,
        ),
      };
    case "top":
      return {
        side: "bottom",
        offset: clamp(
          spotCenterX - cardPos.left,
          edgePadding,
          card.width - edgePadding,
        ),
      };
    default:
      return null;
  }
}

/**
 * 卡片优先放在亮框右 / 下 / 左 / 上不重叠的一侧；若四侧空间都略紧，
 * 选与亮框重叠面积最小的位置，而不是硬压在内容区右下角。
 */
function placeCard(
  spot: Rect | null,
  card: { width: number; height: number },
  viewport: Viewport,
): CardPosition {
  const vw = viewport.width;
  const vh = viewport.height;
  const maxLeft = vw - card.width - VIEWPORT_MARGIN;
  const maxTop = vh - card.height - VIEWPORT_MARGIN;

  if (!spot) {
    return {
      top: clamp((vh - card.height) / 2, VIEWPORT_MARGIN, maxTop),
      left: clamp((vw - card.width) / 2, VIEWPORT_MARGIN, maxLeft),
      placement: "center",
      caret: null,
    };
  }

  const right = spot.left + spot.width;
  const bottom = spot.top + spot.height;
  const spotCenterX = spot.left + spot.width / 2;
  // 亮框偏左时卡片左对齐，偏右时右对齐，视觉连接更顺
  const alignedX = clamp(
    spotCenterX < vw / 2 ? spot.left : right - card.width,
    VIEWPORT_MARGIN,
    maxLeft,
  );

  const candidates: {
    placement: Exclude<CardPlacement, "center" | "fallback">;
    fits: boolean;
    top: number;
    left: number;
  }[] = [
    {
      placement: "right",
      fits: right + CARD_GAP + card.width <= vw - VIEWPORT_MARGIN,
      left: clamp(right + CARD_GAP, VIEWPORT_MARGIN, maxLeft),
      top: clamp(spot.top, VIEWPORT_MARGIN, maxTop),
    },
    {
      placement: "bottom",
      fits: bottom + CARD_GAP + card.height <= vh - VIEWPORT_MARGIN,
      top: clamp(bottom + CARD_GAP, VIEWPORT_MARGIN, maxTop),
      left: alignedX,
    },
    {
      placement: "left",
      fits: spot.left - CARD_GAP - card.width >= VIEWPORT_MARGIN,
      left: clamp(spot.left - CARD_GAP - card.width, VIEWPORT_MARGIN, maxLeft),
      top: clamp(spot.top, VIEWPORT_MARGIN, maxTop),
    },
    {
      placement: "top",
      fits: spot.top - CARD_GAP - card.height >= VIEWPORT_MARGIN,
      top: clamp(spot.top - CARD_GAP - card.height, VIEWPORT_MARGIN, maxTop),
      left: alignedX,
    },
  ];

  const exact = candidates.find((c) => c.fits);
  if (exact) {
    return {
      top: exact.top,
      left: exact.left,
      placement: exact.placement,
      caret: computeCaret(spot, exact, card, exact.placement),
    };
  }

  // 四侧都放不下时按与亮框重叠面积最小择优
  let best = { top: maxTop, left: maxLeft, overlap: Infinity };
  for (const c of [...candidates, { top: maxTop, left: maxLeft }]) {
    const overlap = overlapArea(spot, {
      top: c.top,
      left: c.left,
      width: card.width,
      height: card.height,
    });
    if (overlap < best.overlap) {
      best = { top: c.top, left: c.left, overlap };
    }
  }

  return {
    top: best.top,
    left: best.left,
    placement: "fallback",
    caret: null,
  };
}

function caretStyle(caret: CardCaret): {
  className: string;
  style: CSSProperties;
} {
  const base = "pointer-events-none absolute h-2.5 w-2.5 rotate-45 bg-surface";
  switch (caret.side) {
    case "left":
      return {
        className: cn(
          base,
          "-left-[6px] -translate-y-1/2 border-b border-l border-border",
        ),
        style: { top: caret.offset },
      };
    case "right":
      return {
        className: cn(
          base,
          "-right-[6px] -translate-y-1/2 border-r border-t border-border",
        ),
        style: { top: caret.offset },
      };
    case "top":
      return {
        className: cn(
          base,
          "-top-[6px] -translate-x-1/2 border-l border-t border-border",
        ),
        style: { left: caret.offset },
      };
    case "bottom":
      return {
        className: cn(
          base,
          "-bottom-[6px] -translate-x-1/2 border-b border-r border-border",
        ),
        style: { left: caret.offset },
      };
  }
}

interface OnboardingTourProps {
  open: boolean;
  /** 跳过、关闭、完成、按 Esc 都走这里 */
  onClose: () => void;
}

/**
 * 使用教程：压暗非关注区域，在真实界面元素上开出圆角亮框，旁边放置引导卡片。
 * 要圈的元素不在当前界面上时（比如当前应用没有模式切换），这一步退回居中卡片。
 * 首次启动时由 FirstRunNoticeDialog 打开，设置 → 关于页可随时重新打开。
 */
export function OnboardingTour({ open, onClose }: OnboardingTourProps) {
  const { t } = useTranslation();
  const titleId = useId();
  const maskId = useId();
  const [index, setIndex] = useState(0);
  const [target, setTarget] = useState<MeasuredTarget>(NO_TARGET);
  const [viewport, setViewport] = useState<Viewport>(readViewport);
  const [cardSize, setCardSize] = useState({ width: 360, height: 220 });
  const cardRef = useRef<HTMLDivElement>(null);
  const primaryRef = useRef<HTMLButtonElement>(null);

  // 关掉时回到第一步、清掉亮框：重新打开时第一帧不会带着上次最后一步的圈
  useEffect(() => {
    if (open) return;
    setIndex(0);
    setTarget(NO_TARGET);
  }, [open]);

  const step = TOUR_STEPS[index];
  const matchedTarget = target.match >= 0 ? step.targets[target.match] : null;
  const bodyKeys =
    matchedTarget?.bodyKeys ?? step.missingBodyKeys ?? step.bodyKeys;
  const calloutKey = matchedTarget
    ? matchedTarget.calloutKey === undefined
      ? step.calloutKey
      : matchedTarget.calloutKey
    : (step.missingCalloutKey ?? step.calloutKey);
  const isFirst = index === 0;
  const isLast = index === TOUR_STEPS.length - 1;
  const Icon = step.icon;

  // 换步骤时把要圈的元素滚进视野，焦点放到主按钮上
  useEffect(() => {
    if (!open) return;
    const first = step.targets
      .flatMap((tItem) => tItem.groups.flat())
      .map(findAnchor)
      .find(Boolean);
    first?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
    primaryRef.current?.focus();
  }, [open, step]);

  // 切换步骤或匹配分支后，在绘制前同步量一次卡片尺寸，避免首帧位置跳变
  useLayoutEffect(() => {
    if (!open) return;
    const card = cardRef.current;
    if (!card) return;
    const w = card.offsetWidth;
    const h = card.offsetHeight;
    if (w > 0 && h > 0) {
      setCardSize((prev) =>
        prev.width === w && prev.height === h ? prev : { width: w, height: h },
      );
    }
  }, [open, index, target.match]);

  // 打开期间每帧同步亮框、窗口尺寸与卡片尺寸：侧栏收起展开、窗口缩放、懒加载晚到都能跟上
  useEffect(() => {
    if (!open) return;
    let frame = 0;
    let last: MeasuredTarget | undefined;
    let lastVp = readViewport();
    setViewport(lastVp);
    let lastW = 0;
    let lastH = 0;

    const tick = () => {
      const vp = readViewport();
      if (vp.width !== lastVp.width || vp.height !== lastVp.height) {
        lastVp = vp;
        setViewport(vp);
      }

      const next = measureTargets(step.targets, vp);
      if (
        !last ||
        next.match !== last.match ||
        !sameRects(next.rects, last.rects)
      ) {
        last = next;
        setTarget(next);
      }

      const card = cardRef.current;
      if (card && (card.offsetWidth !== lastW || card.offsetHeight !== lastH)) {
        lastW = card.offsetWidth;
        lastH = card.offsetHeight;
        if (lastW > 0 && lastH > 0) {
          setCardSize({ width: lastW, height: lastH });
        }
      }
      frame = requestAnimationFrame(tick);
    };
    tick();
    return () => cancelAnimationFrame(frame);
  }, [open, step]);

  const goNext = () => {
    if (isLast) onClose();
    else setIndex((i) => i + 1);
  };
  const goBack = () => setIndex((i) => Math.max(0, i - 1));

  const handleKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    } else if (event.key === "ArrowRight") {
      event.preventDefault();
      goNext();
    } else if (event.key === "ArrowLeft" && !isFirst) {
      event.preventDefault();
      goBack();
    } else if (event.key === "Tab") {
      // 焦点只在卡片里的按钮间打转，不会跑到压暗的界面上
      const buttons = Array.from(
        cardRef.current?.querySelectorAll<HTMLElement>("button") ?? [],
      );
      if (buttons.length === 0) return;
      const at = buttons.indexOf(document.activeElement as HTMLElement);
      const next = event.shiftKey
        ? (at <= 0 ? buttons.length : at) - 1
        : (at + 1) % buttons.length;
      event.preventDefault();
      buttons[next].focus();
    }
  };

  if (!open) return null;

  const spots = target.rects;
  const anchorBox = unionRect(spots);
  const cardPos = placeCard(anchorBox, cardSize, viewport);
  const caret = cardPos.caret ? caretStyle(cardPos.caret) : null;

  return createPortal(
    <div
      className="fixed inset-0 z-[110]"
      onKeyDown={handleKeyDown}
      data-testid="onboarding-tour"
    >
      {/* 整片挡住点击：教程期间不让误点到下面的界面 */}
      <div className="absolute inset-0" aria-hidden="true" />

      {/* SVG 遮罩：支持同时挖出多个独立圆角亮框，且居中步骤无需出现缩放点 */}
      <svg
        aria-hidden="true"
        className="pointer-events-none fixed inset-0 h-full w-full"
      >
        <defs>
          <mask id={maskId}>
            <rect x="0" y="0" width="100%" height="100%" fill="white" />
            {spots.map((spot, i) => (
              <rect
                key={i}
                x={spot.left}
                y={spot.top}
                width={spot.width}
                height={spot.height}
                rx={10}
                ry={10}
                fill="black"
              />
            ))}
          </mask>
        </defs>
        <rect
          x="0"
          y="0"
          width="100%"
          height="100%"
          mask={`url(#${maskId})`}
          className="fill-black/45 dark:fill-black/60"
        />
      </svg>

      {/* 每个亮框自己的高亮描边与轻柔外发光 */}
      {spots.map((spot, i) => (
        <div
          key={i}
          aria-hidden="true"
          data-testid="tour-spotlight"
          className="pointer-events-none fixed rounded-[10px] ring-2 ring-action shadow-[0_0_0_4px_rgba(249,115,22,0.16)] transition-all duration-300 ease-out motion-reduce:transition-none"
          style={{
            top: spot.top,
            left: spot.left,
            width: spot.width,
            height: spot.height,
          }}
        />
      ))}

      <div
        ref={cardRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="fixed flex max-h-[calc(100vh-24px)] w-[360px] max-w-[calc(100vw-24px)] flex-col rounded-dialog border border-border bg-surface text-fg-1 shadow-v7-lg transition-[top,left] duration-300 ease-out motion-reduce:transition-none"
        style={{ top: cardPos.top, left: cardPos.left }}
      >
        {caret && (
          <span
            aria-hidden="true"
            className={caret.className}
            style={caret.style}
          />
        )}

        <div className="flex items-center gap-2.5 px-5 pb-2.5 pt-4">
          <span
            aria-hidden="true"
            className="flex h-7 w-7 shrink-0 items-center justify-center rounded-control bg-action-soft text-action-text"
          >
            <Icon className="h-4 w-4" />
          </span>
          <h2
            id={titleId}
            className="m-0 min-w-0 flex-1 text-title font-semibold text-fg-1"
          >
            {t(step.titleKey)}
          </h2>
          <span className="shrink-0 text-caption font-medium tabular-nums text-fg-3">
            {t("tutorial.stepCounter", {
              current: index + 1,
              total: TOUR_STEPS.length,
              defaultValue: `${index + 1} / ${TOUR_STEPS.length}`,
            })}
          </span>
          <Button
            type="button"
            variant="quiet"
            size="icon-compact"
            onClick={onClose}
            aria-label={t("common.close")}
            title={t("common.close")}
            className="-me-1.5 text-fg-3 hover:text-fg-1"
          >
            <X className="h-4 w-4" />
          </Button>
        </div>

        <div className="min-h-0 flex-1 space-y-2.5 overflow-y-auto px-5 pb-4 pt-1">
          {bodyKeys.map((key) => (
            <p
              key={key}
              className="m-0 whitespace-pre-line text-body leading-relaxed text-fg-2"
            >
              {t(key)}
            </p>
          ))}

          {step.modeItems && (
            <div className="divide-y divide-border overflow-hidden rounded-panel border border-border bg-subtle/40">
              {step.modeItems.map((item) => (
                <div
                  key={item.mode}
                  className="flex items-start gap-2.5 px-3 py-2"
                >
                  <span
                    aria-hidden="true"
                    className={cn(
                      "mt-[6px] h-2 w-2 shrink-0 rounded-full",
                      MODE_DOT_CLASS[item.mode],
                    )}
                  />
                  <div className="min-w-0">
                    <div className="text-caption font-semibold text-fg-1">
                      {t(item.titleKey)}
                    </div>
                    <div className="text-caption leading-relaxed text-fg-2">
                      {t(item.descKey)}
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}

          {calloutKey && (
            <div className="rounded-panel border border-border bg-subtle/70 px-3 py-2 text-caption leading-relaxed text-fg-2">
              {t(calloutKey)}
            </div>
          )}
        </div>

        <div className="flex items-center justify-between gap-3 border-t border-border px-5 py-3">
          <div
            className="flex items-center gap-0.5"
            role="group"
            aria-label={t("tutorial.progress", {
              current: index + 1,
              total: TOUR_STEPS.length,
            })}
          >
            {TOUR_STEPS.map((s, i) => (
              <button
                key={s.id}
                type="button"
                onClick={() => setIndex(i)}
                aria-label={t("tutorial.progress", {
                  current: i + 1,
                  total: TOUR_STEPS.length,
                })}
                aria-current={i === index ? "step" : undefined}
                className="flex items-center justify-center px-0.5 py-1.5 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring rounded-sm"
              >
                <span
                  className={cn(
                    "h-1.5 rounded-full transition-all",
                    i === index
                      ? "w-4 bg-action"
                      : "w-1.5 bg-border-strong hover:bg-fg-3",
                  )}
                />
              </button>
            ))}
          </div>
          <div className="flex items-center gap-2">
            {isFirst ? (
              <Button variant="quiet" size="compact" onClick={onClose}>
                {t("tutorial.skip")}
              </Button>
            ) : (
              <Button variant="neutral" size="compact" onClick={goBack}>
                {t("tutorial.back")}
              </Button>
            )}
            <Button
              ref={primaryRef}
              variant="solid"
              size="compact"
              onClick={goNext}
            >
              {isLast ? t("tutorial.done") : t("tutorial.next")}
            </Button>
          </div>
        </div>
      </div>
    </div>,
    document.body,
  );
}
