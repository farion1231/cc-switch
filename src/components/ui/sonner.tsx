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
        // v7：反色的中性条，不跟主题色走；成功 / 失败靠图标和文字区分
        classNames: {
          toast:
            "group flex items-center gap-2 rounded-panel border-0 bg-inverse px-3.5 py-2.5 text-body text-inverse-fg shadow-v7-lg",
          title: "text-body font-medium",
          description: "text-caption opacity-80",
          closeButton:
            "!border-0 !bg-inverse !text-inverse-fg hover:!bg-inverse-hover",
          actionButton:
            "!rounded-control !bg-transparent !px-2 !text-caption !font-medium !text-inverse-fg underline underline-offset-2 hover:!bg-inverse-hover",
          cancelButton:
            "!rounded-control !bg-transparent !px-2 !text-caption !text-inverse-fg opacity-80",
        },
      }}
    />
  );
}
