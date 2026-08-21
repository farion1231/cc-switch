import type { ComponentProps } from "react";
import { cn } from "@/lib/utils";

type ProviderFormLayoutProps = ComponentProps<"form">;

/** Shared visual shell used by every provider configuration form. */
export function ProviderFormLayout({
  className,
  ...props
}: ProviderFormLayoutProps) {
  return (
    <form
      className={cn(
        "space-y-6 glass rounded-xl p-6 border border-white/10",
        className,
      )}
      {...props}
    />
  );
}
