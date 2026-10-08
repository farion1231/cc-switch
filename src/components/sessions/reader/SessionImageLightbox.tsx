import { memo, useState } from "react";
import { Copy, Maximize2, Minimize2, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { HoverTip } from "@/components/ui/hover-tip";
import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { useReaderT } from "./i18n";

const toolButton =
  "inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-control text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";

export interface SessionImageLightboxProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** 已加载好的 Blob URL */
  url: string;
  alt: string;
  mediaType?: string;
}

/** 写剪贴板：浏览器普遍只收 image/png，其他格式先经 canvas 转一次 */
const copyImage = async (url: string) => {
  const blob = await fetch(url).then((response) => response.blob());
  let png = blob;
  if (blob.type !== "image/png") {
    const bitmap = await createImageBitmap(blob);
    const canvas = document.createElement("canvas");
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    canvas.getContext("2d")?.drawImage(bitmap, 0, 0);
    png = await new Promise<Blob>((resolve, reject) =>
      canvas.toBlob(
        (result) => (result ? resolve(result) : reject(new Error("toBlob"))),
        "image/png",
      ),
    );
  }
  await navigator.clipboard.write([new ClipboardItem({ "image/png": png })]);
};

/**
 * 图片灯箱：基于 Dialog（焦点陷阱、Esc 关闭），原图默认适应窗口，可切到 100% 原始大小并滚动查看。
 */
export const SessionImageLightbox = memo(function SessionImageLightbox({
  open,
  onOpenChange,
  url,
  alt,
  mediaType,
}: SessionImageLightboxProps) {
  const { t } = useTranslation();
  const rt = useReaderT();
  const [actualSize, setActualSize] = useState(false);

  const handleCopy = async () => {
    try {
      await copyImage(url);
      toast.success(rt("image.copied"));
    } catch {
      toast.error(t("common.error", { defaultValue: "复制失败" }));
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        zIndex="top"
        className="w-auto max-w-[min(92vw,1280px)] gap-0 overflow-hidden p-0"
        onInteractOutside={() => onOpenChange(false)}
      >
        <div className="flex h-10 shrink-0 items-center gap-1 border-b border-border pe-1.5 ps-4">
          <DialogTitle className="min-w-0 flex-1 truncate text-body font-medium">
            {alt}
          </DialogTitle>
          <DialogDescription className="sr-only">
            {mediaType ?? ""}
          </DialogDescription>
          <HoverTip
            content={actualSize ? rt("image.fit") : rt("image.actualSize")}
          >
            <button
              type="button"
              aria-pressed={actualSize}
              aria-label={actualSize ? rt("image.fit") : rt("image.actualSize")}
              onClick={() => setActualSize((value) => !value)}
              className={toolButton}
            >
              {actualSize ? (
                <Minimize2 aria-hidden className="h-3.5 w-3.5" />
              ) : (
                <Maximize2 aria-hidden className="h-3.5 w-3.5" />
              )}
            </button>
          </HoverTip>
          <HoverTip content={rt("image.copy")}>
            <button
              type="button"
              aria-label={rt("image.copy")}
              onClick={() => void handleCopy()}
              className={toolButton}
            >
              <Copy aria-hidden className="h-3.5 w-3.5" />
            </button>
          </HoverTip>
          <DialogClose asChild>
            <button
              type="button"
              aria-label={rt("image.close")}
              className={toolButton}
            >
              <X aria-hidden className="h-4 w-4" />
            </button>
          </DialogClose>
        </div>
        <div
          className={cn(
            "flex max-h-[80vh] min-h-[160px] min-w-[240px] bg-subtle",
            actualSize
              ? "items-start justify-start overflow-auto"
              : "items-center justify-center overflow-hidden",
          )}
        >
          <img
            src={url}
            alt={alt}
            className={cn(
              "block",
              actualSize
                ? "max-w-none"
                : "max-h-[80vh] max-w-full object-contain",
            )}
          />
        </div>
      </DialogContent>
    </Dialog>
  );
});
