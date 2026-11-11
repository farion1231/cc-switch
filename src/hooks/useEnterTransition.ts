import { useEffect, useLayoutEffect, useRef } from "react";

/**
 * 新内容从哪边进来。跟着触发它的那个控件走：
 * - 侧栏、设置目录是竖排的：点下面的项，内容从下面浮上来（from-below），点上面的从上面落下（from-above）；
 * - 页签、分段控件是横排的：往右切从行尾滑进（from-end），往左切从行首滑进（from-start）；
 * - 进二级目录（设置）像推进一层，从行尾滑进；退出来从行首滑回。
 * 选中指示条往哪滑，内容就从哪来，视线从点的地方顺着落到新内容上。
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
}

const SHIFT_X = 12;
const SHIFT_Y = 6;
const DURATION_MS = 220;
// 和侧栏开合同一条减速曲线（useSidebarCollapsed）
const EASING = "cubic-bezier(0.2, 0, 0, 1)";
// 不从全透明开始：旧内容是一下子换掉的，从 0 渐入中间会空一帧，看着像闪了一下（#8159 那类）
const FROM_OPACITY = 0.3;

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
  const running = useRef<Animation | null>(null);
  const optionsRef = useRef(options);
  optionsRef.current = options;

  useLayoutEffect(() => {
    const prev = prevKey.current;
    prevKey.current = key;
    const first = !mounted.current;
    mounted.current = true;

    const { skipInitial = false, from = "below" } = optionsRef.current;
    if (first && skipInitial) return;
    const element = ref.current;
    if (!element || typeof element.animate !== "function") return;
    if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) return;

    const direction = typeof from === "function" ? from(prev, key) : from;
    if (direction === "none") return;

    running.current?.cancel();
    const rtl = getComputedStyle(element).direction === "rtl";
    const offset =
      direction === "below"
        ? `translateY(${SHIFT_Y}px)`
        : direction === "above"
          ? `translateY(${-SHIFT_Y}px)`
          : `translateX(${(direction === "end" ? 1 : -1) * (rtl ? -1 : 1) * SHIFT_X}px)`;
    running.current = element.animate(
      [
        { opacity: FROM_OPACITY, transform: offset },
        { opacity: 1, transform: "none" },
      ],
      { duration: DURATION_MS, easing: EASING },
    );
  }, [key]);

  useEffect(() => () => running.current?.cancel(), []);

  return ref;
}

/** 按两项在一排里的先后算方向：横排（页签）给 end / start，竖排（目录）给 below / above。 */
export function directionByOrder<K>(
  order: readonly K[],
  axis: "x" | "y",
): (prev: K | undefined, next: K) => EnterFrom {
  return (prev, next) => {
    if (prev === undefined) return axis === "x" ? "end" : "below";
    const a = order.indexOf(prev);
    const b = order.indexOf(next);
    if (a < 0 || b < 0 || a === b) return axis === "x" ? "end" : "below";
    if (axis === "x") return b > a ? "end" : "start";
    return b > a ? "below" : "above";
  };
}
