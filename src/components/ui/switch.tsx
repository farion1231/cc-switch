import * as React from "react";
import * as SwitchPrimitives from "@radix-ui/react-switch";
import { cn } from "@/lib/utils";

export interface SwitchProps
  extends React.ComponentPropsWithoutRef<typeof SwitchPrimitives.Root> {
  /** default：30×18；sm：26×16，给表格里成排的开关用 */
  size?: "default" | "sm";
}

const Switch = React.forwardRef<
  React.ElementRef<typeof SwitchPrimitives.Root>,
  SwitchProps
>(({ className, size = "default", ...props }, ref) => (
  <SwitchPrimitives.Root
    ref={ref}
    data-size={size}
    className={cn(
      // v7：轨道 30×18，开 = 操作色、圆点反色；关 = --control-off、白圆点。
      // 用 before 把点击区域扩到 38×28。
      "peer relative inline-flex shrink-0 cursor-pointer items-center rounded-full border-2 border-transparent transition-colors duration-150 before:absolute before:-inset-x-1.5 before:-inset-y-1.5 before:content-[''] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-45 data-[state=checked]:bg-action data-[state=unchecked]:bg-control-off",
      size === "sm" ? "h-4 w-[26px]" : "h-[18px] w-[30px]",
      className,
    )}
    {...props}
  >
    <SwitchPrimitives.Thumb
      className={cn(
        "pointer-events-none block rounded-full shadow-v7-sm ring-0 transition-[transform,background-color] duration-150 data-[state=checked]:bg-action-fg data-[state=unchecked]:translate-x-0 data-[state=unchecked]:bg-white",
        size === "sm"
          ? "h-3 w-3 data-[state=checked]:translate-x-2.5"
          : "h-3.5 w-3.5 data-[state=checked]:translate-x-3",
      )}
    />
  </SwitchPrimitives.Root>
));
Switch.displayName = SwitchPrimitives.Root.displayName;

export { Switch };
