/**
 * 用量统计里几张表（请求日志、供应商、模型、定价）共用的样式：
 * 列头 12px --text-2，行高 36，分隔线 --border；表格区自己横向滚动，页面不溢出。
 */
export const usageTable = {
  /** 外层：窄窗口时表格在这里横向滚动 */
  scroller: "w-full overflow-x-auto",
  table: "w-full border-collapse text-body tabular-nums",
  headRow: "h-9 border-b border-border text-caption text-fg-2",
  th: "whitespace-nowrap px-1.5 text-start font-medium first:ps-2 last:pe-2",
  thEnd: "whitespace-nowrap px-1.5 text-end font-medium",
  row: "h-9 border-b border-border",
  rowInteractive:
    "h-9 cursor-pointer border-b border-border transition-colors hover:bg-subtle",
  td: "whitespace-nowrap px-1.5",
  tdEnd: "whitespace-nowrap px-1.5 text-end",
  mono: "font-mono text-caption",
  muted: "text-fg-3",
  empty: "py-10 text-center text-body text-fg-3",
  skeleton: "h-[240px] animate-pulse rounded-panel bg-subtle",
} as const;
