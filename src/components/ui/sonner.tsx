import { Toaster as SonnerToaster } from "sonner";
import { useTheme } from "@/components/theme-provider";

export function Toaster() {
  const { theme } = useTheme();

  const sonnerTheme = theme === "system" ? "system" : theme;

  return (
    <SonnerToaster
      position="top-center"
      richColors
      theme={sonnerTheme}
      gap={8}
      toastOptions={{
        duration: 2500,
        classNames: {
          toast:
            "liquid-glass rounded-xl text-foreground",
          title: "text-sm font-semibold",
          description: "text-xs text-muted-foreground",
          closeButton:
            "absolute right-2 top-2 rounded-full p-1 text-muted-foreground transition-colors hover:bg-white/30 dark:hover:bg-white/5 hover:text-foreground",
          actionButton:
            "rounded-lg bg-primary/80 px-3 py-1 text-xs font-medium text-primary-foreground transition-all hover:bg-primary",
        },
      }}
    />
  );
}
