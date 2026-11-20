import { useLayoutEffect, useRef } from "react";

/**
 * 新内容从哪边进来，跟着触发它的那个控件走：
 * - 侧栏、设置目录是竖排的：内容和选中块往同一个方向动。点下面的项，选中块往下滑，
 *   内容也从上面往下落（from-above）；点上面的项，内容从下面往上浮（from-below）。
 *   选中块就在内容旁边，两样东西要是一个往下一个往上，看着就是反的。
 * - 页签、分段控件是横排的：按翻页的习惯，往右切新内容从行尾滑进（from-end），往左切从行首滑进（from-start）。
 * - 进二级目录（设置、整页编辑、会话阅读）像推进一层，从行尾滑进；退出来从行首滑回。
 */
export type EnterFrom = "below" | "above" | "end" | "start" | "none";

interface EnterTransitionOptions<K> {
  /**
   * 用这个 hook 的组件第一次挂载时不播，只在 key 变了以后播。
   * 按的是 hook 所在组件的挂载，不是 ref 指向的元素：元素随 key 重建也照样播。
   */
  skipInitial?: boolean;
  /** 固定方向，或按上一个 key 和这一个 key 算方向 */
  from?: EnterFrom | ((prev: K | undefined, next: K) => EnterFrom);
  /**
   * 整页切换用：页头那条栏（底边线、窗口按钮）不动，只动栏里的标题、操作和栏下面的内容。
   * 像系统的导航栏：栏一直在，换的是里面的字。不开的话整条页头会跟着页面一起位移、变淡。
   */
  preserveHeader?: boolean;
}

const SHIFT_X = 12;
const SHIFT_Y = 6;
export const ENTER_DURATION_MS = 220;
// 和侧栏开合同一条减速曲线（useSidebarCollapsed）
export const ENTER_EASING = "cubic-bezier(0.2, 0, 0, 1)";
// 不从全透明开始：旧内容是一下子换掉的，从 0 渐入中间会空一帧，看着像闪了一下（#8159 那类）
const FROM_OPACITY = 0.3;

export function prefersReducedMotion() {
  return Boolean(
    window.matchMedia?.("(prefers-reduced-motion: reduce)").matches,
  );
}

/**
 * 等新内容第一帧画出来再开始播。
 * 换上来的页面往往很重（MCP 矩阵、Skills 列表），挂载后的第一帧要先排版、栅格化，可能占一两百毫秒；
 * 动画要是建好就开始计时，等第一帧画出来已经播掉大半，看到的只剩最后一下跳，就是「卡」。
 * 先停在第一帧（起点的位移和透明度会照常画出来），隔两帧、确认重的那一帧已经画完再放。
 * 返回取消函数：还没放就被新动画顶掉或卸载时，别再去 play 一个已经取消的动画。
 */
export function playAfterPaint(animation: Animation): () => void {
  animation.pause();
  let frame = requestAnimationFrame(() => {
    frame = requestAnimationFrame(() => animation.play());
  });
  return () => {
    cancelAnimationFrame(frame);
    animation.cancel();
  };
}

/**
 * 开了 preserveHeader 时要动的元素：从页头（data-page-header）往上一路到 root，
 * 每一层除了「通往页头的那一个」以外的兄弟都动；页头自己不动，只动它里面除窗口按钮以外的几组。
 * 页面里没有页头（或页头在弹层里、不在 root 下面）就整块动。
 */
function transitionTargets(root: HTMLElement, preserveHeader: boolean) {
  const header = preserveHeader
    ? root.querySelector<HTMLElement>("[data-page-header]")
    : null;
  if (!header) return [root];
  const targets: Element[] = Array.from(header.children).filter(
    (child) => !child.hasAttribute("data-window-controls"),
  );
  let node: HTMLElement = header;
  while (node !== root && node.parentElement) {
    const parent: HTMLElement = node.parentElement;
    for (const sibling of Array.from(parent.children)) {
      if (sibling !== node) targets.push(...boxesOf(sibling));
    }
    node = parent;
  }
  return targets;
}

/** display: contents 的一层（如 UnsavedChangesScope）自己没有盒子，transform 不起作用：往下拿它的子元素 */
function boxesOf(element: Element): Element[] {
  if (getComputedStyle(element).display !== "contents") return [element];
  return Array.from(element.children).flatMap(boxesOf);
}

/**
 * key 变了就让 ref 指向的容器从 from 那边滑进来。用 Web Animations 跑 transform + opacity，
 * 交给合成器、不占主线程；不加 fill，播完 transform 就撤掉，不会给里面的 fixed 元素留下包含块。
 * 系统开了「减少动态效果」或环境没有 element.animate（jsdom）时什么都不做。
 */
export function useEnterTransition<T extends HTMLElement, K>(
  key: K,
  options: EnterTransitionOptions<K> = {},
) {
  const ref = useRef<T>(null);
  const prevKey = useRef<K | undefined>(undefined);
  const mounted = useRef(false);
  const stop = useRef<(() => void) | null>(null);
  const optionsRef = useRef(options);
  optionsRef.current = options;

  useLayoutEffect(() => {
    const prev = prevKey.current;
    prevKey.current = key;
    const first = !mounted.current;
    mounted.current = true;

    const {
      skipInitial = false,
      from = "below",
      preserveHeader = false,
    } = optionsRef.current;
    if (first && skipInitial) return;
    // 不是第一次跑、key 也没变：开发模式 StrictMode 挂载时把 effect 跑两遍，或 Suspense 藏起又露出来。
    // 这不是「切换」，不能播；否则 skipInitial 的容器一进页面就按 from(key, key) 自己动一下
    // （MCP 页一进来就从左边滑进，就是它那个「编辑页关掉后从左边滑回」被误播了）
    if (!first && Object.is(prev, key)) return;
    const element = ref.current;
    if (!element || typeof element.animate !== "function") return;
    if (prefersReducedMotion()) return;

    const direction = typeof from === "function" ? from(prev, key) : from;
    if (direction === "none") return;

    stop.current?.();
    const rtl = getComputedStyle(element).direction === "rtl";
    const offset =
      direction === "below"
        ? `translateY(${SHIFT_Y}px)`
        : direction === "above"
          ? `translateY(${-SHIFT_Y}px)`
          : `translateX(${(direction === "end" ? 1 : -1) * (rtl ? -1 : 1) * SHIFT_X}px)`;
    const stops = transitionTargets(element, preserveHeader).map((target) =>
      playAfterPaint(
        target.animate(
          [
            { opacity: FROM_OPACITY, transform: offset },
            { opacity: 1, transform: "none" },
          ],
          { duration: ENTER_DURATION_MS, easing: ENTER_EASING },
        ),
      ),
    );
    stop.current = () => stops.forEach((cancel) => cancel());
  }, [key]);
  // 卸载时不取消：元素拿掉动画也就没了。要是在卸载清理里取消，StrictMode 模拟卸载那一下
  // 会把刚开始的进场动画掐掉，而重跑时 key 没变又不会再播

  return ref;
}

/**
 * 按两项在一排里的先后算方向。
 * 横排（页签）：往后切从行尾进（end），往前切从行首进（start）。
 * 竖排（目录）：往下切内容从上面落下（above），往上切从下面浮起（below），和选中块同向。
 */
export function directionByOrder<K>(
  order: readonly K[],
  axis: "x" | "y",
): (prev: K | undefined, next: K) => EnterFrom {
  return (prev, next) => {
    const a = prev === undefined ? -1 : order.indexOf(prev);
    const b = order.indexOf(next);
    const forward = a < 0 || b < 0 || a === b || b > a;
    if (axis === "x") return forward ? "end" : "start";
    return forward ? "above" : "below";
  };
}
