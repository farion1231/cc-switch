import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { getVersion } from "@tauri-apps/api/app";
import { OnboardingTour } from "@/components/OnboardingTour";
import { useSettingsQuery } from "@/lib/query";
import { settingsApi } from "@/lib/api";
import { markSeen } from "@/lib/whatsNew";
import { useOnboardingTourRequest } from "@/lib/onboardingTour";

interface FirstRunNoticeDialogProps {
  /** 手动重看前把界面切到供应商页：要圈的侧栏、按钮、卡片都在这一页上 */
  onBeforeOpen?: () => void;
}

/**
 * 使用教程的宿主：settings.json 里 firstRunNoticeConfirmed 为空时自动打开，
 * 跳过或看完都写成 true，以后不再自动打开；关于页可以用 startOnboardingTour() 手动重看。
 */
export function FirstRunNoticeDialog({
  onBeforeOpen,
}: FirstRunNoticeDialogProps = {}) {
  const queryClient = useQueryClient();
  const { data: settings } = useSettingsQuery();
  // 点了就先关，不等保存回来
  const [closed, setClosed] = useState(false);
  const [replaying, setReplaying] = useState(false);

  const firstRunPending =
    !closed && settings != null && settings.firstRunNoticeConfirmed !== true;
  const isOpen = firstRunPending || replaying;

  // 手动重看（从设置 → 关于）时先回到供应商页；首次启动本来就停在供应商页，
  // 而且设置是异步读到的，那时再切页面会把用户刚点开的页面拽回去
  useOnboardingTourRequest(() => {
    onBeforeOpen?.();
    setReplaying(true);
  });

  const acknowledgeFirstRun = async () => {
    setClosed(true);
    if (!settings) return;
    try {
      const { webdavSync: _, ...rest } = settings;
      // 新装用户看的就是新界面、新版本，「界面改版了」弹窗和更新摘要一并记成已看过
      const version = await getVersion().catch(() => undefined);
      await settingsApi.save({
        ...rest,
        firstRunNoticeConfirmed: true,
        newLayoutNoticeConfirmed: true,
        whatsNewSeenVersion: markSeen(settings.whatsNewSeenVersion, version),
      });
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    } catch (error) {
      console.error("Failed to save firstRunNoticeConfirmed:", error);
    }
  };

  const handleClose = () => {
    setReplaying(false);
    if (firstRunPending) void acknowledgeFirstRun();
  };

  return <OnboardingTour open={isOpen} onClose={handleClose} />;
}
