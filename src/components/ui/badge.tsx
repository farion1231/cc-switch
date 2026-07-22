import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "@/lib/utils";

const badgeVariants = cva(
  "inline-flex items-center rounded-lg border px-2.5 py-0.5 text-xs font-semibold transition-colors focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2",
  {
    variants: {
      variant: {
        default:
          "border-transparent bg-primary/90 text-primary-foreground shadow-sm shadow-primary/20",
        secondary:
          "border-white/30 dark:border-white/10 bg-white/40 dark:bg-white/10 text-secondary-foreground backdrop-blur-sm",
        destructive:
          "border-transparent bg-destructive/90 text-destructive-foreground shadow-sm",
        outline: "text-foreground border-white/30 dark:border-white/10 glass-pill",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  },
);

export interface BadgeProps
  extends React.HTMLAttributes<HTMLDivElement>,
    VariantProps<typeof badgeVariants> {}

function Badge({ className, variant, ...props }: BadgeProps) {
  return (
    <div className={cn(badgeVariants({ variant }), className)} {...props} />
  );
}

export { Badge, badgeVariants };
