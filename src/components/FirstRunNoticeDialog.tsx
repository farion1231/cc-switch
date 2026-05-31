import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { Sparkles } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { useSettingsQuery } from "@/lib/query";
import { settingsApi } from "@/lib/api";

/** 首次运行欢迎提示：仅当后端启动阶段保留 firstRunNoticeConfirmed 为空时弹出。 */
export function FirstRunNoticeDialog() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: settings } = useSettingsQuery();

  // 后端启动时已经决定好要不要弹：条件不满足的话字段会立即被写成 true，
  // 所以前端这里只需要判空即可——完全对齐 streamCheckConfirmed 等既有 flag 的模式。
  const isOpen = settings != null && settings.firstRunNoticeConfirmed !== true;

  const handleAcknowledge = async () => {
    if (!settings) return;
    try {
      const { webdavSync: _, ...rest } = settings;
      await settingsApi.save({ ...rest, firstRunNoticeConfirmed: true });
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    } catch (error) {
      console.error("Failed to save firstRunNoticeConfirmed:", error);
    }
  };

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(open) => {
        if (!open) void handleAcknowledge();
      }}
    >
      <DialogContent className="max-w-md" zIndex="top">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Sparkles className="h-5 w-5 text-blue-500" />
            {t("firstRunNotice.title")}
          </DialogTitle>
        </DialogHeader>
        <div className="space-y-3 px-6 py-5">
          <DialogDescription className="whitespace-pre-line leading-relaxed">
            {t("firstRunNotice.bodyDefault")}
          </DialogDescription>
          <DialogDescription className="whitespace-pre-line leading-relaxed">
            {t("firstRunNotice.bodyOfficial")}
          </DialogDescription>
        </div>
        <DialogFooter>
          <Button onClick={handleAcknowledge}>
            {t("firstRunNotice.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
