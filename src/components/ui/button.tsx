import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-lg text-sm font-medium transition-all duration-150 ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/30 focus-visible:ring-offset-1 disabled:pointer-events-none disabled:opacity-50 active:scale-[0.97]",
  {
    variants: {
      variant: {
        default:
          "bg-gradient-to-b from-primary to-primary/90 text-primary-foreground shadow-[inset_0_1px_0_rgba(255,255,255,0.25),0_1px_2px_rgba(0,0,0,0.12),0_2px_8px_hsl(var(--primary)/0.25)] hover:shadow-[inset_0_1px_0_rgba(255,255,255,0.25),0_1px_2px_rgba(0,0,0,0.12),0_4px_14px_hsl(var(--primary)/0.4)] hover:brightness-105",
        destructive:
          "bg-gradient-to-b from-destructive to-destructive/90 text-destructive-foreground shadow-[inset_0_1px_0_rgba(255,255,255,0.2),0_1px_2px_rgba(0,0,0,0.12)] hover:brightness-105",
        outline:
          "border border-border bg-card text-foreground shadow-[inset_0_1px_0_rgba(255,255,255,0.4),0_1px_2px_hsl(var(--shadow-tint)/0.05)] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] hover:border-primary/35 hover:bg-accent/60 hover:text-accent-foreground",
        secondary:
          "bg-secondary text-secondary-foreground shadow-[inset_0_1px_0_rgba(255,255,255,0.35)] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] hover:bg-secondary/75",
        ghost:
          "text-muted-foreground hover:bg-accent/70 hover:text-foreground",
        mcp: "bg-gradient-to-b from-emerald-500 to-emerald-600 text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.25),0_1px_2px_rgba(0,0,0,0.12)] hover:brightness-105 dark:from-emerald-600 dark:to-emerald-700",
        link: "text-primary underline-offset-4 hover:underline",
      },
      size: {
        default: "h-9 px-4 py-2",
        sm: "h-8 rounded-md px-3 text-xs",
        lg: "h-10 rounded-lg px-6",
        icon: "h-9 w-9 p-1.5 rounded-lg",
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
