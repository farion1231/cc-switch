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
    <div className="hidden w-64 shrink-0 overflow-hidden rounded-2xl border border-border bg-card xl:block">
      <div className="border-b border-white/40 p-3 dark:border-white/8">
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
                "w-full rounded-[0.9rem] px-2 py-1.5 text-left text-xs transition-colors",
                "text-muted-foreground hover:bg-white/40 hover:text-foreground dark:hover:bg-white/[0.05]",
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
          className="fixed bottom-20 right-4 z-30 size-10 rounded-full border border-cyan-200/70 bg-[linear-gradient(145deg,rgba(255,255,255,0.88),rgba(109,228,255,0.74))] text-slate-900 shadow-[0_20px_36px_-22px_rgba(14,165,233,0.7),inset_0_1px_0_rgba(255,255,255,0.92)] xl:hidden dark:border-cyan-300/15 dark:bg-[linear-gradient(145deg,rgba(34,211,238,0.28),rgba(14,116,144,0.28))] dark:text-cyan-50"
        >
          <List className="size-4" />
        </Button>
      </DialogTrigger>
      <DialogContent
        className="max-h-[70vh] max-w-md flex flex-col gap-0 p-0"
        zIndex="alert"
        onInteractOutside={() => onOpenChange(false)}
        onEscapeKeyDown={() => onOpenChange(false)}
      >
        <DialogHeader className="relative border-b border-white/40 px-4 py-3 dark:border-white/8">
          <DialogTitle className="flex items-center gap-2 text-base font-semibold">
            <List className="size-4 text-primary" />
            {t("sessionManager.tocTitle")}
          </DialogTitle>
          <DialogClose
            className="absolute right-3 top-1/2 -translate-y-1/2 rounded-full p-1.5 transition-colors hover:bg-white/50 focus:outline-none focus:ring-2 focus:ring-primary focus:ring-offset-2 dark:hover:bg-white/[0.08]"
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
                  "w-full rounded-[1rem] px-3 py-2.5 text-left text-sm text-foreground transition-all",
                  "hover:bg-white/50 dark:hover:bg-white/[0.06]",
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
