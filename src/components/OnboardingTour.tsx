import { useEffect, useId, useRef, useState, type CSSProperties } from "react";
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
  /** 要圈出来的元素，多个时圈住它们的外接矩形 */
  anchors: TourAnchor[];
  /** 圈的是这一组时换用的正文；不给就用步骤的 bodyKeys */
  bodyKeys?: string[];
}

interface TourStep {
  id: string;
  icon: LucideIcon;
  titleKey: string;
  /** 正文段落的 i18n key，按顺序各占一段 */
  bodyKeys: string[];
  /**
   * 按顺序找，第一组在界面上的就圈它（比如有供应商时圈卡片，还没有时圈空列表）。
   * 空的或一组都找不到时不圈，卡片居中。
   */
  targets: TourTarget[];
  /** 一组都找不到时的正文（比如当前应用没有模式切换）；不给就用 bodyKeys */
  missingBodyKeys?: string[];
}

// 第一步沿用原来欢迎弹窗的文案：default / 官方供应商是新用户最需要先知道的
export const TOUR_STEPS: TourStep[] = [
  {
    id: "welcome",
    icon: Sparkles,
    titleKey: "firstRunNotice.title",
    bodyKeys: ["firstRunNotice.bodyDefault"],
    targets: [],
  },
  {
    id: "apps",
    icon: PanelLeft,
    titleKey: "tutorial.apps.title",
    bodyKeys: ["tutorial.apps.body"],
    targets: [{ anchors: ["apps"] }],
  },
  {
    id: "providers",
    icon: ArrowLeftRight,
    titleKey: "tutorial.providers.title",
    bodyKeys: ["tutorial.providers.body", "firstRunNotice.bodyOfficial"],
    targets: [
      { anchors: ["provider-card"] },
      // 新装、还没有供应商：圈空列表，说明加了之后会出现在这里
      { anchors: ["provider-empty"], bodyKeys: ["tutorial.providers.empty"] },
    ],
  },
  {
    id: "add",
    icon: Plus,
    titleKey: "tutorial.add.title",
    bodyKeys: ["tutorial.add.body"],
    targets: [{ anchors: ["add-provider"] }],
  },
  {
    id: "modes",
    icon: Route,
    titleKey: "tutorial.modes.title",
    bodyKeys: ["tutorial.modes.body"],
    // 只有 Claude Code / Codex / Gemini CLI / Grok Build / Claude Desktop 有这组切换，
    // 不管有没有供应商都显示；其他应用（OpenCode 等）没有，卡片居中并说明去哪里找
    targets: [{ anchors: ["mode-tabs"] }],
    missingBodyKeys: ["tutorial.modes.body", "tutorial.modes.unavailable"],
  },
  {
    id: "shared",
    icon: Server,
    titleKey: "tutorial.shared.title",
    bodyKeys: ["tutorial.shared.body"],
    targets: [
      { anchors: ["nav-mcp", "nav-skills", "nav-prompts", "nav-sessions"] },
    ],
  },
  {
    id: "more",
    icon: ChartColumn,
    titleKey: "tutorial.more.title",
    bodyKeys: ["tutorial.more.body", "tutorial.more.replay"],
    targets: [{ anchors: ["nav-auth", "nav-usage", "nav-settings"] }],
  },
];

interface Rect {
  top: number;
  left: number;
  width: number;
  height: number;
}

/** 圈的范围比元素本身大一圈 */
const SPOT_PADDING = 6;
/** 卡片和圈之间的距离 */
const CARD_GAP = 14;
/** 卡片离窗口边缘至少留这么多 */
const VIEWPORT_MARGIN = 12;

function findAnchor(anchor: TourAnchor): HTMLElement | null {
  return document.querySelector<HTMLElement>(`[data-tour="${anchor}"]`);
}

/** 量出这一步要圈的范围（已裁到窗口内）；一个元素都没找到就是 null */
function measureAnchors(anchors: TourAnchor[]): Rect | null {
  let top = Infinity;
  let left = Infinity;
  let right = -Infinity;
  let bottom = -Infinity;
  for (const anchor of anchors) {
    const el = findAnchor(anchor);
    if (!el) continue;
    const r = el.getBoundingClientRect();
    if (r.width === 0 && r.height === 0) continue;
    top = Math.min(top, r.top);
    left = Math.min(left, r.left);
    right = Math.max(right, r.right);
    bottom = Math.max(bottom, r.bottom);
  }
  if (top === Infinity) return null;

  const vw = window.innerWidth;
  const vh = window.innerHeight;
  top = Math.max(top - SPOT_PADDING, 0);
  left = Math.max(left - SPOT_PADDING, 0);
  right = Math.min(right + SPOT_PADDING, vw);
  bottom = Math.min(bottom + SPOT_PADDING, vh);
  if (right <= left || bottom <= top) return null;
  return { top, left, width: right - left, height: bottom - top };
}

interface MeasuredTarget {
  /** 圈的是 targets 里的第几组；-1 表示一组都不在界面上 */
  match: number;
  rect: Rect | null;
}

const NO_TARGET: MeasuredTarget = { match: -1, rect: null };

/** 按顺序找第一组在界面上的目标 */
function measureTargets(targets: TourTarget[]): MeasuredTarget {
  for (let i = 0; i < targets.length; i++) {
    const rect = measureAnchors(targets[i].anchors);
    if (rect) return { match: i, rect };
  }
  return NO_TARGET;
}

function sameRect(a: Rect | null, b: Rect | null): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  return (
    Math.abs(a.top - b.top) < 0.5 &&
    Math.abs(a.left - b.left) < 0.5 &&
    Math.abs(a.width - b.width) < 0.5 &&
    Math.abs(a.height - b.height) < 0.5
  );
}

const clamp = (value: number, min: number, max: number) =>
  Math.max(min, Math.min(value, Math.max(min, max)));

/** 卡片放在圈的右、下、左、上，哪边放得下放哪边；都放不下就压在窗口右下角 */
function placeCard(
  spot: Rect | null,
  card: { width: number; height: number },
): { top: number; left: number } {
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const maxLeft = vw - card.width - VIEWPORT_MARGIN;
  const maxTop = vh - card.height - VIEWPORT_MARGIN;

  if (!spot) {
    return {
      top: clamp((vh - card.height) / 2, VIEWPORT_MARGIN, maxTop),
      left: clamp((vw - card.width) / 2, VIEWPORT_MARGIN, maxLeft),
    };
  }

  const right = spot.left + spot.width;
  const bottom = spot.top + spot.height;

  if (right + CARD_GAP + card.width <= vw - VIEWPORT_MARGIN) {
    return {
      left: right + CARD_GAP,
      top: clamp(spot.top, VIEWPORT_MARGIN, maxTop),
    };
  }
  if (bottom + CARD_GAP + card.height <= vh - VIEWPORT_MARGIN) {
    return {
      top: bottom + CARD_GAP,
      left: clamp(right - card.width, VIEWPORT_MARGIN, maxLeft),
    };
  }
  if (spot.left - CARD_GAP - card.width >= VIEWPORT_MARGIN) {
    return {
      left: spot.left - CARD_GAP - card.width,
      top: clamp(spot.top, VIEWPORT_MARGIN, maxTop),
    };
  }
  if (spot.top - CARD_GAP - card.height >= VIEWPORT_MARGIN) {
    return {
      top: spot.top - CARD_GAP - card.height,
      left: clamp(right - card.width, VIEWPORT_MARGIN, maxLeft),
    };
  }
  return { top: maxTop, left: maxLeft };
}

interface OnboardingTourProps {
  open: boolean;
  /** 跳过、完成、按 Esc 都走这里 */
  onClose: () => void;
}

/**
 * 使用教程：把界面压暗，在真实的侧栏、按钮、卡片上开一个亮框，旁边放说明卡片。
 * 要圈的元素不在当前界面上时（比如这个应用没有模式切换），这一步就退回居中卡片。
 * 首次启动时由 FirstRunNoticeDialog 打开，关于页可以重新打开。
 */
export function OnboardingTour({ open, onClose }: OnboardingTourProps) {
  const { t } = useTranslation();
  const titleId = useId();
  const [index, setIndex] = useState(0);
  const [target, setTarget] = useState(NO_TARGET);
  const [cardSize, setCardSize] = useState({ width: 340, height: 220 });
  const cardRef = useRef<HTMLDivElement>(null);
  const primaryRef = useRef<HTMLButtonElement>(null);

  // 关掉时就回到第一步、清掉亮框：重新打开时第一帧不会带着上次最后一步的圈
  useEffect(() => {
    if (open) return;
    setIndex(0);
    setTarget(NO_TARGET);
  }, [open]);

  const step = TOUR_STEPS[index];
  const spot = target.rect;
  const bodyKeys =
    target.match >= 0
      ? (step.targets[target.match]?.bodyKeys ?? step.bodyKeys)
      : (step.missingBodyKeys ?? step.bodyKeys);
  const isFirst = index === 0;
  const isLast = index === TOUR_STEPS.length - 1;
  const Icon = step.icon;

  // 换步骤时把要圈的元素滚进视野，焦点放到主按钮上
  useEffect(() => {
    if (!open) return;
    const first = step.targets
      .flatMap((target) => target.anchors)
      .map(findAnchor)
      .find(Boolean);
    first?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
    primaryRef.current?.focus();
  }, [open, step]);

  // 打开期间每帧量一次：侧栏收起展开、窗口缩放、懒加载的页面晚到，圈都能跟上
  useEffect(() => {
    if (!open) return;
    let frame = 0;
    // 第一帧总是写一次：上一步的圈要换掉，哪怕这一步什么都不圈
    let last: MeasuredTarget | undefined;
    let lastW = 0;
    let lastH = 0;
    const tick = () => {
      const next = measureTargets(step.targets);
      if (
        !last ||
        next.match !== last.match ||
        !sameRect(next.rect, last.rect)
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

  // 没有要圈的元素时亮框缩成窗口正中的一个点，四周的阴影就是一整片遮罩；
  // 换步骤时亮框从上一个位置滑过去
  const spotStyle: CSSProperties = spot
    ? {
        top: spot.top,
        left: spot.left,
        width: spot.width,
        height: spot.height,
      }
    : {
        top: window.innerHeight / 2,
        left: window.innerWidth / 2,
        width: 0,
        height: 0,
      };
  const cardPos = placeCard(spot, cardSize);

  return createPortal(
    <div
      className="fixed inset-0 z-[110]"
      onKeyDown={handleKeyDown}
      data-testid="onboarding-tour"
    >
      {/* 整片挡住点击：教程期间不让误点到下面的界面 */}
      <div className="absolute inset-0" aria-hidden="true" />
      <div
        aria-hidden="true"
        className={cn(
          "pointer-events-none fixed rounded-[10px] transition-all duration-300 ease-out motion-reduce:transition-none",
          spot && "ring-2 ring-action",
        )}
        style={{
          ...spotStyle,
          boxShadow: "0 0 0 200vmax var(--overlay)",
        }}
      />
      <div
        ref={cardRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="fixed flex w-[340px] max-w-[calc(100vw-24px)] flex-col rounded-dialog border border-border bg-surface text-fg-1 shadow-v7-lg transition-[top,left] duration-300 ease-out motion-reduce:transition-none"
        style={cardPos}
      >
        <div className="flex items-center gap-2 px-5 pt-4">
          <Icon className="h-[18px] w-[18px] shrink-0 text-fg-1" />
          <h2 id={titleId} className="m-0 text-title font-semibold">
            {t(step.titleKey)}
          </h2>
        </div>
        <div className="space-y-2.5 px-5 pb-4 pt-2.5">
          {bodyKeys.map((key) => (
            <p
              key={key}
              className="m-0 whitespace-pre-line text-body leading-relaxed text-fg-2"
            >
              {t(key)}
            </p>
          ))}
        </div>
        <div className="flex items-center justify-between gap-3 border-t border-border px-5 py-3">
          <div
            className="flex items-center gap-1.5"
            role="img"
            aria-label={t("tutorial.progress", {
              current: index + 1,
              total: TOUR_STEPS.length,
            })}
          >
            {TOUR_STEPS.map((s, i) => (
              <span
                key={s.id}
                className={cn(
                  "h-1.5 rounded-full transition-all",
                  i === index ? "w-4 bg-fg-1" : "w-1.5 bg-border-strong",
                )}
              />
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
