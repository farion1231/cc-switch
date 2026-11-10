import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { settingsApi } from "@/lib/api";
import { useSettingsQuery } from "@/lib/query";
import { toast } from "@/lib/toast";
import { extractErrorMessage } from "@/utils/errorUtils";
import { Switch } from "@/components/ui/switch";
import { HelpTip } from "@/components/ui/help-tip";

/**
 * 「暂停使用官方登录」的状态和开关动作。设置页不放它：它只在路由 / 聚合模式下生效，用户在
 * Codex 被锁、正看着路由或聚合页时才会找它，所以放在这两个页签的设置侧边栏，开着时页面顶部
 * 还有一条提示可以直接恢复。后端保存时当场重写 Codex 配置，失败会回滚并报错。
 */
export function useCodexPauseOfficialLogin() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: settings } = useSettingsQuery();
  const [saving, setSaving] = useState(false);

  const setPaused = async (paused: boolean) => {
    setSaving(true);
    try {
      const current = await settingsApi.get();
      await settingsApi.save({
        ...current,
        codexProxyWithoutOfficialLogin: paused,
      });
      toast.success(
        t(
          paused
            ? "settings.codexPauseOfficialLogin.pausedToast"
            : "settings.codexPauseOfficialLogin.resumedToast",
        ),
      );
    } catch (error) {
      console.error("[CodexPauseOfficialLogin] Failed to save", error);
      toast.error(
        t("settings.codexPauseOfficialLogin.saveFailed", {
          error: extractErrorMessage(error),
        }),
      );
    } finally {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
      setSaving(false);
    }
  };

  return {
    paused: settings?.codexProxyWithoutOfficialLogin ?? false,
    saving,
    setPaused,
  };
}

/** 路由设置 / 聚合设置侧边栏的第一节：应急开关放最上面，不用往下翻、也不混进下面的表单。 */
export function CodexPauseOfficialLoginSection() {
  const { t } = useTranslation();
  const { paused, saving, setPaused } = useCodexPauseOfficialLogin();
  const title = t("settings.codexPauseOfficialLogin.title");

  return (
    <div className="space-y-1.5 border-b border-border pb-3">
      <div className="flex items-center gap-2">
        <span className="text-caption font-semibold text-fg-2">{title}</span>
        <HelpTip title={title} className="-ms-1">
          {t("settings.codexPauseOfficialLogin.help")}
        </HelpTip>
        <div className="flex-1" />
        <Switch
          checked={paused}
          disabled={saving}
          onCheckedChange={(next) => void setPaused(next)}
          aria-label={title}
        />
      </div>
      <p className="text-caption text-fg-2">
        {t("settings.codexPauseOfficialLogin.description")}
      </p>
    </div>
  );
}
