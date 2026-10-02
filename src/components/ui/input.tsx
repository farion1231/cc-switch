import * as React from "react";
import { cn } from "@/lib/utils";

/**
 * 输入框 / 文本域共用的外观（docs/design-system.html「输入」）：圆角 6、border-strong 描边，
 * 聚焦时描边换主题色 + 3px 光晕，出错时红描边。不能用 Input / Textarea 的原生控件直接拼这个。
 */
export const fieldClass =
  "w-full rounded-control border border-border-strong bg-surface px-2.5 text-body text-fg-1 transition-[border-color,box-shadow] placeholder:text-fg-3 focus:border-ring focus:outline-none focus:ring-[3px] focus:ring-ring/20 dark:focus:ring-ring/15 focus-visible:outline-none aria-[invalid=true]:border-danger disabled:cursor-not-allowed disabled:opacity-50";

export type InputProps = React.InputHTMLAttributes<HTMLInputElement>;

const Input = React.forwardRef<HTMLInputElement, InputProps>(
  ({ className, type, ...props }, ref) => {
    return (
      <input
        type={type}
        className={cn(
          fieldClass,
          "flex h-8 py-1 file:border-0 file:bg-transparent file:text-body file:font-medium file:text-fg-1",
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
