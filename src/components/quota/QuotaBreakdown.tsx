import type { ReactNode } from "react";
import { ChevronDown } from "lucide-react";
import { cn } from "@/lib/utils";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { TONE_TEXT } from "./QuotaLines";
import type {
  QuotaBreakdown,
  QuotaBreakdownItem,
  QuotaLine,
} from "./quotaRules";

/**
 * 一行写不下的额度（存下了好几次重置）：整行（卡片上是那一段）是按钮，点开逐条列出明细
 * （每一次的到期日，同一天到期的并成一条）。授权中心、卡片额度列和展开的额度条共用。
 *
 * `className` 给按钮，用调用方原来那一行的排版；左右各外扩 4px 留出悬停底色，文字不挪位。
 */
export function QuotaBreakdownRow({
  line,
  breakdown,
  className,
  align = "start",
  children,
}: {
  line: QuotaLine;
  breakdown: QuotaBreakdown;
  className?: string;
  /** 弹窗和按钮哪边对齐：卡片额度列靠右，用 end */
  align?: "start" | "end";
  children: ReactNode;
}) {
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label={breakdown.openLabel}
          // 卡片上别让点击冒到卡片
          onClick={(event) => event.stopPropagation()}
          className={cn(
            className,
            "group/breakdown -mx-1 rounded-control px-1 text-start transition-colors hover:bg-subtle focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring data-[state=open]:bg-subtle",
          )}
        >
          {children}
        </button>
      </PopoverTrigger>
      <PopoverContent
        align={align}
        sideOffset={6}
        aria-label={breakdown.title}
        className="w-[236px] rounded-panel border-border bg-surface p-3 shadow-v7-md"
      >
        <div className="mb-2 flex items-baseline justify-between gap-3 text-caption">
          <span className="font-medium text-fg-1">{breakdown.title}</span>
          <span className="whitespace-nowrap tabular-nums text-fg-2">
            {line.value ?? line.text}
          </span>
        </div>
        <BreakdownItems items={breakdown.items} />
        {breakdown.footer && (
          <BreakdownItems
            items={breakdown.footer}
            className="mt-2 border-t border-border pt-2"
          />
        )}
      </PopoverContent>
    </Popover>
  );
}

/** 跟在说明文字后面的 ⌄，点开时翻过来 */
export function QuotaBreakdownChevron() {
  return (
    <ChevronDown
      aria-hidden
      className="h-3 w-3 shrink-0 transition-transform group-hover/breakdown:text-fg-1 group-data-[state=open]/breakdown:rotate-180"
      strokeWidth={1.5}
    />
  );
}

function BreakdownItems({
  items,
  className,
}: {
  items: QuotaBreakdownItem[];
  className?: string;
}) {
  return (
    <ul className={cn("flex flex-col gap-1 text-caption", className)}>
      {items.map((item) => (
        <li key={item.key} className="flex items-center gap-2">
          <span
            className={cn(
              "whitespace-nowrap",
              item.tone === "normal" ? "text-fg-1" : TONE_TEXT[item.tone],
            )}
          >
            {item.label}
          </span>
          <span className="min-w-0 flex-1 truncate text-fg-3">{item.hint}</span>
          <span className="whitespace-nowrap text-end tabular-nums text-fg-2">
            {item.value}
          </span>
        </li>
      ))}
    </ul>
  );
}
