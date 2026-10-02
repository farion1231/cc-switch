import { Toaster as SonnerToaster } from "sonner";
import { useTheme } from "@/components/theme-provider";

export function Toaster() {
  const { theme } = useTheme();

  // 将应用主题映射到 Sonner 的主题
  // 如果是 "system"，Sonner 会自己处理
  const sonnerTheme = theme === "system" ? "system" : theme;

  return (
    <SonnerToaster
      position="bottom-center"
      theme={sonnerTheme}
      toastOptions={{
        duration: 2000,
        // unstyled：sonner 自带样式的选择器比 Tailwind 类更具体，会盖掉这里的底色和关闭按钮位置，
        // 所以整条 toast 都自己画（位置、堆叠、滑入滑出仍由 sonner 负责）。
        // 别给 toast 加 position 类：sonner 靠 absolute + transform 堆叠，改成 relative 会乱飞。
        unstyled: true,
        classNames: {
          toast:
            "group flex w-full items-center gap-2.5 rounded-panel border border-border bg-surface py-2.5 pe-3.5 ps-3 text-body text-fg-1 shadow-v7-lg",
          content: "flex min-w-0 flex-1 flex-col gap-0.5",
          title: "text-body font-medium",
          description: "text-caption text-fg-2",
          // 状态靠图标颜色区分：成功绿、失败红、警告琥珀、提示灰
          icon: "flex h-4 w-4 shrink-0 items-center justify-center [&>svg]:h-4 [&>svg]:w-4",
          success: "[&_[data-icon]]:text-success",
          error: "[&_[data-icon]]:text-danger",
          warning: "[&_[data-icon]]:text-warning",
          info: "[&_[data-icon]]:text-fg-2",
          loading: "[&_[data-icon]]:text-fg-2",
          // 关闭按钮在右上角，压在角上（RTL 时到左上角）
          closeButton:
            "absolute -end-2 -top-2 flex h-5 w-5 items-center justify-center rounded-full border border-border bg-surface text-fg-2 shadow-v7-sm transition-colors hover:text-fg-1 [&>svg]:h-3 [&>svg]:w-3",
          actionButton:
            "h-6 shrink-0 rounded-control px-2 text-caption font-medium text-action-text transition-colors hover:bg-subtle",
          cancelButton:
            "h-6 shrink-0 rounded-control px-2 text-caption text-fg-2 transition-colors hover:bg-subtle",
        },
      }}
    />
  );
}
