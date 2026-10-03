import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
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
import { APP_IDS } from "@/config/appConfig";
import { providersApi, settingsApi } from "@/lib/api";
import { useSettingsQuery } from "@/lib/query";

const RELEASES_URL = "https://github.com/farion1231/cc-switch/releases";

/** 任一应用的数据库里已经有供应商（找到一家就停） */
async function hasAnyProvider(): Promise<boolean> {
  for (const app of APP_IDS) {
    try {
      const providers = await providersApi.getAll(app);
      if (Object.keys(providers).length > 0) return true;
    } catch {
      // 某个应用读失败不影响其他应用
    }
  }
  return false;
}

/**
 * 「界面改版了」一次性弹窗：只给升级上来的老用户看，确认后写 newLayoutNoticeConfirmed。
 * - 必须已经确认过首次启动的欢迎弹窗。全新安装时后端会先导入当前配置、再预置官方
 *   供应商，单看「有没有供应商」分不出新老用户；新用户点欢迎弹窗的「我知道了」时
 *   会把这个字段一起写成 true，以后也不会再看到这里。
 * - 数据库里已经有供应商（任一应用）。
 * 弹窗盖在页面上，不占布局，晚一点出来也不会推动内容区。
 */
export function NewLayoutDialog() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: settings } = useSettingsQuery();
  // 点了就先关，不等保存回来
  const [closed, setClosed] = useState(false);

  const eligible =
    settings != null &&
    settings.firstRunNoticeConfirmed === true &&
    settings.newLayoutNoticeConfirmed !== true;

  const { data: hasProviders } = useQuery({
    queryKey: ["new-layout-notice", "has-providers"],
    queryFn: hasAnyProvider,
    enabled: eligible && !closed,
    staleTime: Infinity,
  });

  const isOpen = eligible && !closed && hasProviders === true;

  const handleAcknowledge = async () => {
    setClosed(true);
    if (!settings) return;
    try {
      const { webdavSync: _, ...rest } = settings;
      await settingsApi.save({ ...rest, newLayoutNoticeConfirmed: true });
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    } catch (error) {
      console.error("Failed to save newLayoutNoticeConfirmed:", error);
    }
  };

  const handleViewReleaseNotes = async () => {
    try {
      const version = await getVersion();
      await settingsApi.openExternal(
        version ? `${RELEASES_URL}/tag/v${version}` : RELEASES_URL,
      );
    } catch (error) {
      console.error("Failed to open release notes:", error);
    }
    await handleAcknowledge();
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
            <Sparkles className="h-5 w-5 text-fg-1" />
            {t("newLayoutNotice.title")}
          </DialogTitle>
        </DialogHeader>
        <div className="px-6 py-5">
          <DialogDescription className="whitespace-pre-line leading-relaxed">
            {t("newLayoutNotice.body")}
          </DialogDescription>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={handleViewReleaseNotes}>
            {t("newLayoutNotice.viewReleaseNotes")}
          </Button>
          <Button onClick={handleAcknowledge}>
            {t("newLayoutNotice.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
