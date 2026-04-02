import type { CSSProperties } from "react";
import { Toaster as SonnerToaster } from "sonner";
import { useTheme } from "@/components/theme-provider";

export function Toaster() {
  const { theme } = useTheme();

  // 将应用主题映射到 Sonner 的主题
  // 如果是 "system"，Sonner 会自己处理
  const sonnerTheme = theme === "system" ? "system" : theme;
  const toastThemeVars = {
    "--normal-bg": "hsl(var(--background))",
    "--normal-bg-hover": "hsl(var(--muted))",
    "--normal-border": "hsl(var(--border))",
    "--normal-border-hover": "hsl(var(--border) / 0.9)",
    "--normal-text": "hsl(var(--foreground))",
    "--success-bg": "hsl(var(--success) / 0.12)",
    "--success-border": "hsl(var(--success) / 0.24)",
    "--success-text": "hsl(var(--success))",
    "--info-bg": "hsl(var(--info) / 0.12)",
    "--info-border": "hsl(var(--info) / 0.24)",
    "--info-text": "hsl(var(--info))",
    "--warning-bg": "hsl(var(--warning) / 0.12)",
    "--warning-border": "hsl(var(--warning) / 0.24)",
    "--warning-text": "hsl(var(--warning))",
    "--error-bg": "hsl(var(--error) / 0.12)",
    "--error-border": "hsl(var(--error) / 0.24)",
    "--error-text": "hsl(var(--error))",
  } as CSSProperties;

  return (
    <SonnerToaster
      position="top-center"
      richColors
      theme={sonnerTheme}
      style={toastThemeVars}
      toastOptions={{
        duration: 2000,
        classNames: {
          toast:
            "group rounded-md border bg-background text-foreground shadow-lg",
          title: "text-sm font-semibold",
          description: "text-sm text-muted-foreground",
          closeButton:
            "absolute right-2 top-2 rounded-full p-1 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground",
          actionButton:
            "rounded-md bg-primary px-3 py-1 text-xs font-medium text-primary-foreground transition-colors hover:bg-primary/90",
        },
      }}
    />
  );
}
