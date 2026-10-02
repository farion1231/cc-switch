import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Notice } from "@/components/ui/notice";
import { APP_IDS } from "@/config/appConfig";
import { providersApi } from "@/lib/api";
import { useSettingsQuery } from "@/lib/query";

export const NEW_LAYOUT_NOTICE_KEY = "cc-switch-new-layout-notice-dismissed";

function readDismissed(): boolean {
  try {
    return window.localStorage.getItem(NEW_LAYOUT_NOTICE_KEY) === "1";
  } catch {
    return false;
  }
}

function writeDismissed() {
  try {
    window.localStorage.setItem(NEW_LAYOUT_NOTICE_KEY, "1");
  } catch {
    // 存不下就只在这次会话里隐藏，下次启动可能再出现一次，不影响使用
  }
}

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
 * 「界面改版了」一次性提示（方案 B6 coach.newLayout、B7 阶段 1）：主内容区顶部的
 * 中性通知条，关掉后永不再出现。只给老用户看：
 * - 数据库里已经有供应商（任一应用）；
 * - 并且之前已经确认过首次启动的欢迎弹窗。全新安装时后端启动会先导入当前配置、
 *   再预置官方供应商，单看「有没有供应商」分不出新老用户；新装用户这次启动会看到
 *   欢迎弹窗（firstRunNoticeConfirmed 还不是 true），这时直接记成「已看过」，
 *   以后也不再给他看「改版」。
 */
export function NewLayoutNotice() {
  const { t } = useTranslation();
  const [dismissed, setDismissed] = useState(readDismissed);
  const { data: settings } = useSettingsQuery();
  const [returningUser, setReturningUser] = useState<boolean | null>(null);

  // 只在第一次拿到设置时判断一次：欢迎弹窗确认之后字段会变 true，不能因此再弹
  useEffect(() => {
    if (dismissed || returningUser !== null || !settings) return;
    const returning = settings.firstRunNoticeConfirmed === true;
    setReturningUser(returning);
    if (!returning) {
      writeDismissed();
      setDismissed(true);
    }
  }, [dismissed, returningUser, settings]);

  const { data: hasProviders } = useQuery({
    queryKey: ["new-layout-notice", "has-providers"],
    queryFn: hasAnyProvider,
    enabled: !dismissed && returningUser === true,
    staleTime: Infinity,
  });

  if (dismissed || returningUser !== true || hasProviders !== true) {
    return null;
  }

  return (
    <div className="shrink-0 px-6 pt-3">
      <Notice
        tone="neutral"
        title={t("coach.newLayout")}
        onDismiss={() => {
          writeDismissed();
          setDismissed(true);
        }}
        dismissLabel={t("common.close")}
      />
    </div>
  );
}
