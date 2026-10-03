import * as React from "react";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { isKeyboardModality } from "@/lib/inputModality";

export interface HoverTipProps {
  /** 提示内容：说这个按钮做什么，一句短话。为空时原样渲染子元素。 */
  content?: React.ReactNode;
  /** 一个可聚焦的元素（按钮）；需要能接收 ref。 */
  children: React.ReactElement;
  side?: "top" | "bottom" | "left" | "right";
  align?: "start" | "center" | "end";
  /** 鼠标离开触发元素后关闭，提示本身不接收指针事件。 */
  disableHoverableContent?: boolean;
}

/** 文案末尾的「…」是菜单项「会弹出对话框」的约定，放进提示里像被截断，去掉。 */
function stripEllipsis(content: React.ReactNode): React.ReactNode {
  return typeof content === "string"
    ? content.replace(/\s*(…|\.{3})$/, "")
    : content;
}

/** Radix 的 onFocus 会在 defaultPrevented 时跳过打开（composeEventHandlers） */
function keepClosedUnlessKeyboard(event: React.FocusEvent) {
  if (!isKeyboardModality()) event.preventDefault();
}

/**
 * 全局悬停提示：移上去立即显示深色胶囊。自带 0 延迟的 Provider，放在哪里都能用。
 * 纯图标按钮一律用它报操作内容；按钮自己仍要有 aria-label。
 *
 * 只认键盘带来的聚焦：菜单 / 弹层关闭时 Radix 会把焦点还给触发按钮，
 * 那次回焦不能当成悬停，否则「⋯」的提示会弹出来并一直留着（鼠标根本没在上面）。
 */
export function HoverTip({
  content,
  children,
  side = "bottom",
  align = "center",
  disableHoverableContent,
}: HoverTipProps) {
  if (content === undefined || content === null || content === "") {
    return children;
  }
  return (
    <TooltipProvider delayDuration={0} skipDelayDuration={0}>
      <Tooltip disableHoverableContent={disableHoverableContent}>
        <TooltipTrigger asChild onFocus={keepClosedUnlessKeyboard}>
          {children}
        </TooltipTrigger>
        <TooltipContent
          side={side}
          align={align}
          className={
            disableHoverableContent ? "pointer-events-none" : undefined
          }
        >
          {stripEllipsis(content)}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
