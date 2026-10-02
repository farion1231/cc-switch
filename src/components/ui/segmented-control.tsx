import * as React from "react";
import { cn } from "@/lib/utils";
import { SegmentThumb, useSlidingIndicator } from "./sliding-indicator";

/**
 * 分段控件（v7 AUTHORING「分段控件」+ R1）：按钮组 + aria-pressed，不是 tablist。
 * 轨道 --bg-subtle，内衬 3px；选中项 --bg-card + 小阴影。内圆角 = 外圆角 − 内衬。
 * 选中块是一块单独的滑块（SegmentThumb），切换时从上一项滑过去。
 */
export interface SegmentedItem<T extends string> {
  value: T;
  label: React.ReactNode;
  icon?: React.ComponentType<{
    className?: string;
    strokeWidth?: number | string;
  }>;
  /** 跟在文字后面的附加内容（例如模式 tab 的「生效中」圆点） */
  trailing?: React.ReactNode;
  /** 选中项以外的附加样式（例如生效但没在看的模式格用模式 soft 底） */
  className?: string;
  disabled?: boolean;
}

export interface SegmentedControlProps<T extends string> {
  items: SegmentedItem<T>[];
  value: T;
  onValueChange: (value: T) => void;
  "aria-label": string;
  /** 36：模式 tab、页头下的子页面切换；28：面板里的小切换 */
  size?: "md" | "sm";
  className?: string;
}

export function SegmentedControl<T extends string>({
  items,
  value,
  onValueChange,
  size = "md",
  className,
  ...rest
}: SegmentedControlProps<T>) {
  const indicator = useSlidingIndicator<HTMLDivElement>(
    '[aria-pressed="true"]',
    value,
  );
  return (
    <div
      ref={indicator.ref}
      role="group"
      aria-label={rest["aria-label"]}
      className={cn(
        "relative inline-flex items-stretch bg-subtle p-[3px]",
        size === "md" ? "h-9 rounded-[10px]" : "h-7 rounded-[8px]",
        className,
      )}
    >
      <SegmentThumb
        rect={indicator.rect}
        animate={indicator.animate}
        className={size === "md" ? "rounded-[7px]" : "rounded-[5px]"}
      />
      {items.map((item) => {
        const pressed = item.value === value;
        const Icon = item.icon;
        return (
          <button
            key={item.value}
            type="button"
            aria-pressed={pressed}
            disabled={item.disabled}
            onClick={() => onValueChange(item.value)}
            className={cn(
              "relative inline-flex items-center justify-center gap-1.5 whitespace-nowrap font-medium text-fg-2 transition-colors duration-150 hover:text-fg-1 disabled:cursor-not-allowed disabled:opacity-45",
              size === "md"
                ? "rounded-[7px] px-3.5 text-body"
                : "rounded-[5px] px-2.5 text-caption",
              item.className,
              pressed && "text-fg-1",
            )}
          >
            {Icon ? (
              <Icon
                className={size === "md" ? "h-4 w-4" : "h-3.5 w-3.5"}
                strokeWidth={2}
              />
            ) : null}
            {item.label}
            {item.trailing}
          </button>
        );
      })}
    </div>
  );
}
