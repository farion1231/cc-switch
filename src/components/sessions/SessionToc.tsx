import { List, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";

interface TocItem {
  index: number;
  preview: string;
  ts?: number;
}

interface SessionTocSidebarProps {
  items: TocItem[];
  onItemClick: (index: number) => void;
}

export function SessionTocSidebar({
  items,
  onItemClick,
}: SessionTocSidebarProps) {
  const { t } = useTranslation();
  if (items.length <= 2) return null;

  return (
    <div className="w-64 border-l shrink-0 hidden xl:block">
      <div className="p-3 border-b">
        <div className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
          <List className="size-3.5" />
          <span>{t("sessionManager.tocTitle")}</span>
        </div>
      </div>
      <ScrollArea className="h-[calc(100%-40px)]">
        <div className="p-2 space-y-0.5">
          {items.map((item, tocIndex) => (
            <button
              key={item.index}
              type="button"
              onClick={() => onItemClick(item.index)}
              className={cn(
                "w-full text-left px-2 py-1.5 rounded text-xs transition-colors",
                "hover:bg-muted/80 text-muted-foreground hover:text-foreground",
                "flex items-start gap-2",
              )}
            >
              <span className="shrink-0 w-4 h-4 rounded-full bg-primary/10 text-primary text-[10px] flex items-center justify-center font-medium">
                {tocIndex + 1}
              </span>
              <span className="line-clamp-2 leading-snug">{item.preview}</span>
            </button>
          ))}
        </div>
      </ScrollArea>
    </div>
  );
}

interface SessionTocDialogProps {
  items: TocItem[];
  onItemClick: (index: number) => void;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function SessionTocDialog({
  items,
  onItemClick,
  open,
  onOpenChange,
}: SessionTocDialogProps) {
  const { t } = useTranslation();
  if (items.length <= 2) return null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogTrigger asChild>
        <Button
          size="icon"
          className="fixed bottom-20 right-4 xl:hidden size-10 rounded-full shadow-lg z-30"
        >
          <List className="size-4" />
        </Button>
      </DialogTrigger>
      <DialogContent
        className="max-w-md max-h-[70vh] flex flex-col p-0 gap-0"
        zIndex="alert"
        onInteractOutside={() => onOpenChange(false)}
        onEscapeKeyDown={() => onOpenChange(false)}
      >
        <DialogHeader className="px-4 py-3 relative border-b">
          <DialogTitle className="flex items-center gap-2 text-base font-semibold">
            <List className="size-4 text-primary" />
            {t("sessionManager.tocTitle")}
          </DialogTitle>
          <DialogClose
            className="absolute right-3 top-1/2 -translate-y-1/2 rounded-full p-1.5 hover:bg-muted transition-colors focus:outline-none focus:ring-2 focus:ring-primary focus:ring-offset-2"
            aria-label={t("common.close")}
          >
            <X className="size-4 text-muted-foreground" />
          </DialogClose>
        </DialogHeader>
        <div className="overflow-y-auto max-h-[calc(70vh-80px)]">
          <div className="p-3 pb-4 space-y-1">
            {items.map((item, tocIndex) => (
              <button
                key={item.index}
                type="button"
                onClick={() => onItemClick(item.index)}
                className={cn(
                  "w-full text-left px-3 py-2.5 rounded-lg text-sm transition-all",
                  "hover:bg-primary/10 text-foreground",
                  "flex items-start gap-3",
                  "focus:outline-none focus:ring-2 focus:ring-primary focus:ring-inset",
                )}
              >
                <span className="shrink-0 w-6 h-6 rounded-full bg-primary text-primary-foreground text-xs flex items-center justify-center font-semibold">
                  {tocIndex + 1}
                </span>
                <span className="line-clamp-2 leading-relaxed pt-0.5">
                  {item.preview}
                </span>
              </button>
            ))}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
