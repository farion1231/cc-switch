import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-lg text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50",
  {
    variants: {
      variant: {
        // 主按钮：跟随主题主色
        default: "bg-primary text-primary-foreground hover:bg-primary/90",
        // 危险按钮：红底白字（对应旧版 danger）
        destructive:
          "bg-destructive text-destructive-foreground hover:bg-destructive/90",
        // 轮廓按钮
        outline:
          "border border-border-default bg-background text-foreground hover:bg-accent hover:text-accent-foreground hover:border-border-hover",
        // 次按钮：跟随 secondary token
        secondary:
          "bg-secondary text-secondary-foreground hover:bg-secondary/80",
        // 幽灵按钮（对应旧版 ghost）
        ghost: "text-muted-foreground hover:text-foreground hover:bg-accent",
        // MCP 专属按钮：跟随 accent token，避免写死绿色
        mcp: "bg-accent text-accent-foreground hover:bg-accent/80",
        // 链接按钮
        link: "text-primary underline-offset-4 hover:underline",
      },
      size: {
        default: "h-9 px-4 py-2",
        sm: "h-8 rounded-md px-3 text-xs",
        lg: "h-10 rounded-md px-8",
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
