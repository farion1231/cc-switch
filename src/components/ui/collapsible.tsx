import * as React from "react";
import * as CollapsiblePrimitive from "@radix-ui/react-collapsible";

const Collapsible = CollapsiblePrimitive.Root;

const CollapsibleTrigger = CollapsiblePrimitive.CollapsibleTrigger;

/**
 * 展开 / 收起时高度跟着动，下面的内容是被慢慢推开、慢慢收回的，不是一下跳过去。
 * - className（间距、内边距）放在里层：Radix 量的是外层高度，内边距算在里面，动画首尾才不跳；
 * - 裁切（overflow: hidden）只写在关键帧里，动画结束就撤掉：常开时不裁掉贴边输入框的焦点环；
 * - 首次挂载就是展开的不播（Radix 自己会跳过）。
 */
const CollapsibleContent = React.forwardRef<
  React.ElementRef<typeof CollapsiblePrimitive.CollapsibleContent>,
  React.ComponentPropsWithoutRef<typeof CollapsiblePrimitive.CollapsibleContent>
>(({ className, children, ...props }, ref) => (
  <CollapsiblePrimitive.CollapsibleContent
    ref={ref}
    className="data-[state=closed]:animate-collapsible-up data-[state=open]:animate-collapsible-down motion-reduce:animate-none"
    {...props}
  >
    <div className={className}>{children}</div>
  </CollapsiblePrimitive.CollapsibleContent>
));
CollapsibleContent.displayName =
  CollapsiblePrimitive.CollapsibleContent.displayName;

export { Collapsible, CollapsibleTrigger, CollapsibleContent };
