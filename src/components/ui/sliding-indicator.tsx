import * as React from "react";
import { cn } from "@/lib/utils";

interface IndicatorRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/**
 * 分段控件 / 页签的滑块：量出选中项（selector 命中的子元素）相对容器的位置，
 * 让一块绝对定位的指示条从上一项滑过去。容器要 position: relative。
 * 第一次量完之前不加过渡，免得打开页面时从左上角飞进来；窗口、文字变化时用 ResizeObserver 重量。
 */
export function useSlidingIndicator<T extends HTMLElement>(
  selector: string,
  value: unknown,
) {
  const ref = React.useRef<T>(null);
  const [rect, setRect] = React.useState<IndicatorRect | null>(null);
  const [animate, setAnimate] = React.useState(false);

  React.useLayoutEffect(() => {
    const container = ref.current;
    if (!container) return;
    const measure = () => {
      const target = container.querySelector<HTMLElement>(selector);
      const next = target
        ? {
            x: target.offsetLeft,
            y: target.offsetTop,
            width: target.offsetWidth,
            height: target.offsetHeight,
          }
        : null;
      // 侧栏开合时容器每帧都在变宽，选中项位置多半没动：没变就不触发重渲染
      setRect((prev) =>
        prev &&
        next &&
        prev.x === next.x &&
        prev.y === next.y &&
        prev.width === next.width &&
        prev.height === next.height
          ? prev
          : next,
      );
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(container);
    for (const child of Array.from(container.children)) observer.observe(child);
    return () => observer.disconnect();
  }, [selector, value]);

  React.useEffect(() => {
    if (!rect || animate) return;
    const frame = requestAnimationFrame(() => setAnimate(true));
    return () => cancelAnimationFrame(frame);
  }, [rect, animate]);

  return { ref, rect, animate };
}

const MOTION =
  "transition-[transform,width,height] duration-200 ease-out motion-reduce:transition-none";

/** 滑块挪的是 left / top，不用 transform，理由见 SegmentThumb。 */
const THUMB_MOTION =
  "transition-[left,top,width,height] duration-200 ease-out motion-reduce:transition-none";

/**
 * 分段控件的白底凸起块，垫在选中项下面。按钮自己要 relative + 不画底色。
 * 不用 transform 滑：WebKit 给 transform 过渡开合成层，压在滑块上的按钮也被提成层，
 * 层按整像素对齐，图标在滑动开始和结束时会抖半个像素。
 */
export function SegmentThumb({
  rect,
  animate,
  className,
}: {
  rect: IndicatorRect | null;
  animate: boolean;
  /** 圆角等，和选中项一致 */
  className?: string;
}) {
  if (!rect) return null;
  return (
    <span
      aria-hidden="true"
      className={cn(
        "pointer-events-none absolute bg-surface shadow-v7-sm",
        animate && THUMB_MOTION,
        className,
      )}
      style={{
        left: rect.x,
        top: rect.y,
        width: rect.width,
        height: rect.height,
      }}
    />
  );
}

/** 页签的下划线：贴着容器底边，只滑横向位置和宽度。 */
export function TabUnderline({
  rect,
  animate,
  className,
}: {
  rect: IndicatorRect | null;
  animate: boolean;
  className?: string;
}) {
  if (!rect) return null;
  return (
    <span
      aria-hidden="true"
      className={cn(
        "pointer-events-none absolute bottom-0 left-0 h-0.5 rounded-full bg-fg-1",
        animate && MOTION,
        className,
      )}
      style={{ transform: `translateX(${rect.x}px)`, width: rect.width }}
    />
  );
}
