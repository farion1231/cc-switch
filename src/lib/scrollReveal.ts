import { isTextEditableTarget } from "@/utils/domUtils";

const SCROLL_KEYS = [
  "ArrowUp",
  "ArrowDown",
  "PageUp",
  "PageDown",
  "Home",
  "End",
  " ",
];

export interface ScrollRevealOptions {
  /**
   * 描边画在元素里面：行在 overflow 裁切的卡片里时，外描边会被裁掉。
   * 独立的块（设置分节）用外描边，四周留了 8px。
   */
  inset?: boolean;
}

/**
 * 把容器里的目标滚到最近的可见位置（已可见就不滚），到位后用两次描边闪烁提示，
 * 不改变条目尺寸或位置。只滚这一个容器，不用 scrollIntoView（会带动外层布局）。
 * 用户一接管（滚轮、触摸、按下、滚动键、拖滚动条）就停下，不再把位置拉回来。
 * 返回取消函数：切页、再次定位时调用。
 */
export function scrollToAndFlash(
  container: HTMLElement,
  target: HTMLElement,
  { inset = false }: ScrollRevealOptions = {},
): () => void {
  let highlight: Animation | undefined;
  let frame = 0;
  const inputListeners = new AbortController();
  const cancel = () => {
    cancelAnimationFrame(frame);
    highlight?.cancel();
    inputListeners.abort();
  };
  const handleScrollKey = (event: KeyboardEvent) => {
    if (
      !isTextEditableTarget(event.target) &&
      SCROLL_KEYS.includes(event.key)
    ) {
      cancel();
    }
  };
  const inputOptions = {
    capture: true,
    passive: true,
    signal: inputListeners.signal,
  };
  for (const event of ["wheel", "touchstart", "pointerdown"]) {
    container.addEventListener(event, cancel, inputOptions);
  }
  window.addEventListener("keydown", handleScrollKey, inputOptions);

  const viewportTop =
    container.getBoundingClientRect().top + container.clientTop;
  const viewportBottom = viewportTop + container.clientHeight;
  const targetRect = target.getBoundingClientRect();
  // 为闪烁描边留出 8px，避免底部条目的提示被裁切。
  const topDelta = targetRect.top - viewportTop - 8;
  const bottomDelta = targetRect.bottom - viewportBottom + 8;
  // 只移动到最近的可见位置；已可见（或高度覆盖整个视口）时不滚动。
  const delta =
    topDelta < 0 && bottomDelta < 0
      ? Math.max(topDelta, bottomDelta)
      : topDelta > 0 && bottomDelta > 0
        ? Math.min(topDelta, bottomDelta)
        : 0;
  const startTop = container.scrollTop;
  const endTop = Math.max(
    0,
    Math.min(startTop + delta, container.scrollHeight - container.clientHeight),
  );

  const ring = inset ? "inset 0 0 0 2px" : "0 0 0 2px";
  const flashTarget = () => {
    highlight = target.animate?.(
      [
        { boxShadow: `${ring} transparent` },
        { boxShadow: `${ring} hsl(var(--ring))` },
        { boxShadow: `${ring} transparent` },
      ],
      { duration: 650, iterations: 2, easing: "ease-in-out" },
    );
    if (highlight) {
      highlight.onfinish = () => inputListeners.abort();
    } else {
      inputListeners.abort();
    }
  };
  const prefersReducedMotion =
    window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
  if (startTop === endTop || prefersReducedMotion) {
    container.scrollTop = endTop;
    flashTarget();
    return cancel;
  }

  // 同一帧循环完成缓出滚动，结束后再闪烁；切页时可一并取消。
  const startedAt = performance.now();
  let expectedTop = startTop;
  const scrollToTarget = (now: number) => {
    // 原生滚动条可能不派发 pointerdown；位置已被外部改变时同样让出控制。
    if (container.scrollTop !== expectedTop) {
      cancel();
      return;
    }
    const progress = Math.min((now - startedAt) / 350, 1);
    const eased = 1 - (1 - progress) ** 3;
    container.scrollTop = startTop + (endTop - startTop) * eased;
    expectedTop = container.scrollTop;
    if (progress < 1) {
      frame = requestAnimationFrame(scrollToTarget);
    } else {
      flashTarget();
    }
  };
  frame = requestAnimationFrame(scrollToTarget);
  return cancel;
}

/** 目标最近的、能纵向滚动的祖先；都滚不动时返回 null。 */
export function findScrollContainer(target: HTMLElement): HTMLElement | null {
  let node = target.parentElement;
  while (node && node !== document.body) {
    const { overflowY } = getComputedStyle(node);
    if (
      (overflowY === "auto" || overflowY === "scroll") &&
      node.scrollHeight > node.clientHeight
    ) {
      return node;
    }
    node = node.parentElement;
  }
  return null;
}
