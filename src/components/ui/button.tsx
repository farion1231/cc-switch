import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-xl text-sm font-medium transition-all duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/30 focus-visible:ring-offset-1 focus-visible:ring-offset-background disabled:pointer-events-none disabled:opacity-40 press-squish",
  {
    variants: {
      variant: {
        default: [
          "bg-primary/90 text-primary-foreground backdrop-blur-sm",
          "hover:bg-primary hover:shadow-md",
          "shadow-sm",
        ].join(" "),
        destructive: [
          "bg-destructive/90 text-destructive-foreground backdrop-blur-sm",
          "hover:bg-destructive hover:shadow-md",
        ].join(" "),
        outline: [
          "liquid-glass-subtle text-foreground",
          "hover:bg-white/40 dark:hover:bg-white/10",
        ].join(" "),
        secondary: [
          "text-muted-foreground",
          "hover:bg-white/30 hover:text-foreground dark:hover:bg-white/5",
        ].join(" "),
        ghost: [
          "text-muted-foreground",
          "hover:text-foreground hover:bg-white/30 dark:hover:bg-white/5",
        ].join(" "),
        mcp: [
          "bg-emerald-500/90 text-white backdrop-blur-sm",
          "hover:bg-emerald-500 hover:shadow-md",
          "shadow-sm",
        ].join(" "),
        link: [
          "text-primary underline-offset-4",
          "hover:underline",
        ].join(" "),
      },
      size: {
        default: "h-9 px-4 py-2",
        sm: "h-8 rounded-lg px-3 text-xs",
        lg: "h-10 rounded-lg px-8",
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
