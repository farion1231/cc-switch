import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-xl text-sm font-medium transition-all duration-200 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50",
  {
    variants: {
      variant: {
        // 主按钮：液态蓝
        default:
          "bg-blue-500/90 text-white shadow-md shadow-blue-500/20 backdrop-blur-sm hover:bg-blue-500 hover:shadow-lg hover:shadow-blue-500/25 dark:bg-blue-500/85 dark:hover:bg-blue-500",
        // 危险按钮
        destructive:
          "bg-red-500/90 text-white shadow-md shadow-red-500/15 backdrop-blur-sm hover:bg-red-500 dark:bg-red-500/85 dark:hover:bg-red-500",
        // 轮廓按钮 — 玻璃描边
        outline:
          "border border-white/40 bg-white/35 text-muted-foreground shadow-sm backdrop-blur-md hover:bg-white/55 hover:text-foreground dark:border-white/10 dark:bg-white/5 dark:hover:bg-white/10 dark:hover:text-gray-100",
        // 次按钮
        secondary:
          "text-gray-500 hover:bg-white/40 dark:text-gray-400 dark:hover:bg-white/10 dark:hover:text-gray-200",
        // 幽灵按钮
        ghost:
          "text-gray-500 hover:text-foreground hover:bg-white/35 dark:text-gray-400 dark:hover:text-gray-100 dark:hover:bg-white/10",
        // MCP 专属按钮
        mcp: "bg-emerald-500/90 text-white shadow-md shadow-emerald-500/15 backdrop-blur-sm hover:bg-emerald-500 dark:bg-emerald-500/85 dark:hover:bg-emerald-500",
        // 链接按钮
        link: "text-blue-500 underline-offset-4 hover:underline dark:text-blue-400",
      },
      size: {
        default: "h-9 px-4 py-2",
        sm: "h-8 rounded-lg px-3 text-xs",
        lg: "h-10 rounded-xl px-8",
        icon: "h-9 w-9 p-1.5",
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
