import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-control text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50",
  {
    variants: {
      variant: {
        // 旧变体都映射到 v7 的样式（不再有蓝色 / 绿色按钮）
        default:
          "border border-transparent bg-action text-action-fg hover:bg-action-hover",
        destructive:
          "border border-transparent bg-danger text-white hover:brightness-110",
        outline:
          "border border-border-strong bg-surface text-fg-1 hover:bg-subtle",
        secondary: "text-fg-2 hover:bg-subtle hover:text-fg-1",
        ghost: "text-fg-2 hover:bg-subtle hover:text-fg-1",
        mcp: "border border-transparent bg-action text-action-fg hover:bg-action-hover",
        link: "text-fg-1 underline-offset-4 hover:underline",
        // ── v7（tokens.css 的 btn-*）：一个视图只有一个实心按钮，其余用 neutral ──
        // 中性实心：页头主操作（操作色，以后的主题色只改这一组变量）
        solid:
          "border border-transparent bg-action text-action-fg hover:bg-action-hover",
        // 中性描边：卡片上的「切换」「添加」「设为默认」等
        neutral:
          "border border-border-strong bg-surface text-fg-1 hover:bg-subtle disabled:opacity-100 disabled:border-border disabled:bg-transparent disabled:text-fg-3",
        // 透明：页头里带 ⌄ / › 的次要操作
        quiet: "border border-transparent text-fg-1 hover:bg-subtle",
        // 模式色实心：只用于模式切换的主操作
        direct:
          "border border-transparent bg-direct-solid text-direct-on font-semibold hover:brightness-110",
        route:
          "border border-transparent bg-route-solid text-route-on font-semibold hover:brightness-110",
        stack:
          "border border-transparent bg-stack-solid text-stack-on font-semibold hover:brightness-110",
      },
      size: {
        default: "h-9 px-4 py-2",
        sm: "h-8 rounded-md px-3 text-xs",
        lg: "h-10 rounded-md px-8",
        icon: "h-9 w-9 p-1.5",
        // v7：高 28（卡片、对话框里）/ 高 32（页头、模式切换）/ 28 见方的图标按钮
        compact:
          "h-7 rounded-control px-3 text-body font-medium transition-[background-color,filter,scale] active:scale-[0.96]",
        regular:
          "h-8 rounded-control px-3.5 text-body font-medium transition-[background-color,filter,scale] active:scale-[0.96]",
        "icon-compact":
          "h-7 w-7 rounded-control p-0 text-fg-2 hover:text-fg-1 transition-[background-color,color,scale] active:scale-[0.96]",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean;
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, ...props }, ref) => {
    const Comp = asChild ? Slot : "button";
    return (
      <Comp
        className={cn(buttonVariants({ variant, size, className }))}
        ref={ref}
        {...props}
      />
    );
  },
);
Button.displayName = "Button";

export { Button, buttonVariants };
