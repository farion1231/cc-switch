import { useEffect, useLayoutEffect, useState } from "react";

/**
 * 用量统计里几张表（请求日志、供应商、模型、定价）共用的样式：
 * 列头 12px --text-2，行高 36，分隔线 --border；表格区自己横向滚动，页面不溢出。
 * 表格放在带边框的面板里，首尾列各留 16px，和面板页签、指标条的内边距对齐；列与列之间 24px。
 * 层次：页签栏（白）→ 表头（浅底）→ 表体（白，行高 40）→ 翻页栏（浅底），表头和翻页栏夹住表体。
 * 最后一行不画底线，翻页栏自己带顶线；没有翻页栏时由面板边框收口，不会出现双线。
 * 所有列（含数字列）一律左对齐，列头和内容同一规则。
 * 分隔线画在单元格上（border-separate）：合并边框模式下，固定列的底色会盖住行线。
 */
export const usageTable = {
  /** 外层：窄窗口时表格在这里横向滚动 */
  scroller: "w-full overflow-x-auto",
  table: "w-full border-separate border-spacing-0 text-body tabular-nums",
  headRow:
    "h-9 bg-subtle text-caption text-fg-2 [&>th]:border-b [&>th]:border-border",
  th: "whitespace-nowrap px-3 text-start font-medium first:ps-4 last:pe-4",
  // 不可点的行不加 group：固定列的悬停变色（group-hover）只跟着可点的行走
  row: "h-10 [&:not(:last-child)>td]:border-b [&>td]:border-border",
  rowInteractive:
    "group h-10 cursor-pointer transition-colors hover:bg-subtle [&:not(:last-child)>td]:border-b [&>td]:border-border",
  td: "whitespace-nowrap px-3 first:ps-4 last:pe-4",
  /** 列组之间多留的间距（请求日志按身份 / 成本 / Tokens / 性能分组）：组内列距 24px，换组处 40px */
  groupGap: "ps-7",
  /** 请求日志列多，收紧一档：组内列距 16px，换组处 32px（叠在 th / td / groupGap 之后，首尾 16px 不变） */
  denseCell: "px-2 first:ps-4 last:pe-4",
  denseGroupGap: "ps-6",
  mono: "font-mono text-caption",
  muted: "text-fg-3",
  empty: "py-10 text-center text-body text-fg-3",
  skeleton: "h-[240px] animate-pulse rounded-panel bg-subtle",
} as const;

/**
 * 横向滚动时固定在左侧的列。固定格子要有自己的底色（不然滚过去的内容会透出来），
 * 表体格子悬停时跟着整行变色；内容真的滚到它下面时，右侧才画分隔线和淡阴影。
 */
export const stickyCol = {
  start: "sticky z-[1]",
  head: "bg-subtle",
  body: "bg-surface transition-colors group-hover:bg-subtle",
  /** 贴左那组的最后一列：右侧有内容藏在它下面 */
  startEdge:
    "shadow-[inset_-1px_0_0_hsl(var(--border)),8px_0_8px_-8px_rgb(0_0_0/0.14)]",
} as const;

/**
 * 滚动容器是否已经往右滚过、左边有内容藏到固定列底下（决定固定列要不要画分隔线和阴影）。
 * 返回回调 ref：加载中先画骨架、表格后挂上时也能接上监听。
 */
export function useScrolledFromStart<T extends HTMLElement>() {
  const [element, setElement] = useState<T | null>(null);
  const [scrolled, setScrolled] = useState(false);
  useEffect(() => {
    if (!element) return;
    const measure = () => setScrolled(Math.abs(element.scrollLeft) > 0);
    measure();
    element.addEventListener("scroll", measure, { passive: true });
    return () => element.removeEventListener("scroll", measure);
  }, [element]);
  return [setElement, scrolled] as const;
}

/** 元素宽度（ResizeObserver）；用回调 ref，元素晚挂上也能量到；测量不到（jsdom）时返回 0 */
export function useElementWidth<T extends HTMLElement>() {
  const [element, setElement] = useState<T | null>(null);
  const [width, setWidth] = useState(0);
  // 在绘制前量：页面按宽度切紧凑布局，晚一帧会先闪一下宽布局
  useLayoutEffect(() => {
    if (!element) return;
    const measure = () => setWidth(element.offsetWidth);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [element]);
  return [setElement, width] as const;
}
