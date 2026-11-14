import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { useIsResyncingSkills, useResyncSkillsToApps } from "@/hooks/useSkills";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import type { SkillAppSyncOutcome } from "@/lib/api/skills";
import type { AppId } from "@/lib/api/types";
import { extractErrorMessage } from "@/utils/errorUtils";

const appName = (app: string) => APP_DISPLAY_NAME[app as AppId] ?? app;

/** 「立即重新同步」：按当前同步方式把已启用的 Skill 重新写到各应用，结果用 toast 报告。
 *  Skills 页 ⋯ 菜单和「存储与同步」对话框共用。 */
export function useSkillsResync() {
  const { t } = useTranslation();
  const resyncMutation = useResyncSkillsToApps();
  // 菜单和对话框各有一个 mutation 实例，进行中状态按 mutationKey 合并
  const isPending = useIsResyncingSkills() || resyncMutation.isPending;

  const reportResync = (outcomes: SkillAppSyncOutcome[]) => {
    const failed = outcomes.filter((outcome) => !outcome.ok);
    if (failed.length === 0) {
      toast.success(t("skills.storageSheet.resyncDone"), { closeButton: true });
      return;
    }
    const details = failed.flatMap((outcome) =>
      outcome.error
        ? [`${appName(outcome.app)}: ${outcome.error}`]
        : outcome.failedSkills.map(
            (skill) =>
              `${appName(outcome.app)} · ${t(
                "skills.storageSheet.resyncSkillItem",
                { directory: skill.directory, error: skill.error },
              )}`,
          ),
    );
    toast.warning(
      t("skills.storageSheet.resyncPartial", {
        count: failed.length,
        apps: failed
          .map((outcome) => appName(outcome.app))
          .join(t("mcpPage.listSeparator")),
      }),
      { description: details.join("\n"), closeButton: true },
    );
  };

  const resync = async () => {
    if (isPending) return;
    try {
      reportResync(await resyncMutation.mutateAsync());
    } catch (error) {
      toast.error(t("common.error"), {
        description: extractErrorMessage(error) || String(error),
      });
    }
  };

  return { resync, isPending };
}
