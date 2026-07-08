import * as React from "react";
import { cn } from "@/lib/utils";

export type InputProps = React.InputHTMLAttributes<HTMLInputElement>;

const Input = React.forwardRef<HTMLInputElement, InputProps>(
  ({ className, type, ...props }, ref) => {
    return (
      <input
        type={type}
        className={cn(
          "flex h-9 w-full rounded-lg border border-input bg-background px-3 py-2 text-sm text-foreground shadow-[inset_0_1px_2px_hsl(var(--shadow-tint)/0.04)] transition-[border-color,box-shadow,background-color] duration-200 file:border-0 file:bg-transparent file:text-sm file:font-medium file:text-foreground placeholder:text-muted-foreground/70 hover:border-input/80 hover:bg-card focus-visible:outline-none focus-visible:bg-card focus-visible:border-primary/55 focus-visible:shadow-[inset_0_1px_2px_hsl(var(--shadow-tint)/0.03),0_0_0_3px_hsl(var(--primary)/0.12)] disabled:cursor-not-allowed disabled:opacity-50",
          className,
        )}
        autoComplete="off"
        autoCorrect="off"
        autoCapitalize="off"
        spellCheck={false}
        ref={ref}
        {...props}
      />
    );
  },
);
Input.displayName = "Input";

export { Input };
