import { useCallback, useEffect, useState } from "react";
import { ChevronLeft, Folder, Home, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { settingsApi, type ServerDirectoryListing } from "@/lib/api/settings";

interface ServerDirectoryPickerDialogProps {
  open: boolean;
  initialPath?: string;
  onOpenChange: (open: boolean) => void;
  onSelect: (path: string) => void;
}

export function ServerDirectoryPickerDialog({
  open,
  initialPath,
  onOpenChange,
  onSelect,
}: ServerDirectoryPickerDialogProps) {
  const { t } = useTranslation();
  const [listing, setListing] = useState<ServerDirectoryListing | null>(null);
  const [inputPath, setInputPath] = useState("");
  const [isLoading, setIsLoading] = useState(false);

  const load = useCallback(
    async (path?: string) => {
      setIsLoading(true);
      try {
        const next = await settingsApi.listServerDirectory(path);
        setListing(next);
        setInputPath(next.path);
      } catch (error) {
        console.error("[ServerDirectoryPickerDialog] list failed", error);
        toast.error(
          t("settings.serverDirectoryPicker.loadFailed", {
            defaultValue: "读取服务端目录失败",
          }),
          {
            description: error instanceof Error ? error.message : String(error),
          },
        );
      } finally {
        setIsLoading(false);
      }
    },
    [t],
  );

  useEffect(() => {
    if (!open) return;
    void load(initialPath);
  }, [initialPath, load, open]);

  const choose = useCallback(() => {
    const path = inputPath.trim() || listing?.path;
    if (!path) return;
    onSelect(path);
    onOpenChange(false);
  }, [inputPath, listing?.path, onOpenChange, onSelect]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl" zIndex="alert">
        <DialogHeader>
          <DialogTitle>
            {t("settings.serverDirectoryPicker.title", {
              defaultValue: "选择服务端目录",
            })}
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-3 px-6">
          <div className="flex items-center gap-2">
            <Input
              value={inputPath}
              onChange={(event) => setInputPath(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  void load(inputPath);
                }
              }}
            />
            <Button
              type="button"
              variant="outline"
              size="icon"
              onClick={() => void load(undefined)}
              title={t("common.home", { defaultValue: "Home" })}
            >
              <Home className="h-4 w-4" />
            </Button>
            <Button
              type="button"
              variant="outline"
              onClick={() => void load(inputPath)}
              disabled={isLoading}
            >
              {isLoading ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
              {t("common.open", { defaultValue: "打开" })}
            </Button>
          </div>

          <div className="h-80 overflow-y-auto rounded-md border border-border/70">
            {listing?.parent ? (
              <button
                type="button"
                className="flex w-full items-center gap-2 border-b border-border/70 px-3 py-2 text-left text-sm hover:bg-muted/60"
                onClick={() => void load(listing.parent ?? undefined)}
              >
                <ChevronLeft className="h-4 w-4" />
                ..
              </button>
            ) : null}
            {listing?.entries.map((entry) => (
              <button
                key={entry.path}
                type="button"
                className="flex w-full items-center gap-2 border-b border-border/40 px-3 py-2 text-left text-sm hover:bg-muted/60"
                onClick={() => void load(entry.path)}
              >
                <Folder className="h-4 w-4 shrink-0 text-primary" />
                <span className="truncate">{entry.name}</span>
              </button>
            ))}
            {!isLoading && listing?.entries.length === 0 ? (
              <div className="px-3 py-6 text-center text-sm text-muted-foreground">
                {t("settings.serverDirectoryPicker.empty", {
                  defaultValue: "没有子目录",
                })}
              </div>
            ) : null}
          </div>
        </div>

        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            onClick={() => onOpenChange(false)}
          >
            {t("common.cancel")}
          </Button>
          <Button type="button" onClick={choose}>
            {t("common.select", { defaultValue: "选择" })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
