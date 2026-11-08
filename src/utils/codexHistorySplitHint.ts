import type { QueryClient } from "@tanstack/react-query";
import type { TFunction } from "i18next";
import { toast } from "@/lib/toast";
import { settingsApi } from "@/lib/api";
import type { CodexHistorySplitHint } from "@/lib/api/providers";
import { extractErrorMessage } from "@/utils/errorUtils";

/**
 * Codex 切换跨了组（官方 openai ↔ 第三方 custom）、统一会话历史又关着时提示一次：
 * 新版 Codex 只能继续当前组的会话，另一组只能查看。可当场开启统一，或不再提示。
 */
export function showCodexHistorySplitHint(
  hint: CodexHistorySplitHint,
  t: TFunction,
  queryClient: QueryClient,
) {
  const previous =
    hint.previousBucket === "openai"
      ? t("notifications.codexHistorySplit.officialBucket", {
          defaultValue: "官方订阅",
        })
      : t("notifications.codexHistorySplit.thirdPartyBucket", {
          defaultValue: "第三方供应商",
        });

  const enableUnify = async () => {
    try {
      // 现读一次设置再保存，别拿可能过期的缓存覆盖别的设置
      const current = await settingsApi.get();
      // 从提示点进来就是为了让旧会话能继续，迁入一并打开
      await settingsApi.save({
        ...current,
        unifyCodexSessionHistory: true,
        unifyCodexMigrateExisting: true,
      });
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
      toast.success(
        t("notifications.codexHistorySplit.enabled", {
          defaultValue:
            "已开启统一会话历史，旧会话正在后台迁入，完成后重启 Codex 即可继续",
        }),
        { duration: 6000 },
      );
    } catch (error) {
      toast.error(
        t("notifications.codexHistorySplit.enableFailed", {
          defaultValue: "开启统一会话历史失败：{{error}}",
          error: extractErrorMessage(error) || t("common.unknown"),
        }),
        { duration: 6000 },
      );
    }
  };

  toast.warning(
    t("notifications.codexHistorySplit.title", {
      defaultValue: "Codex 会话分成了两组",
    }),
    {
      description: t("notifications.codexHistorySplit.description", {
        defaultValue:
          "新版 Codex 只能继续和当前供应商同一组的会话。你之前在{{previous}}下的 {{count}} 个会话现在只能查看。开启「统一 Codex 会话历史」可把两组合并。",
        previous,
        count: hint.lockedThreads,
      }),
      duration: 20000,
      closeButton: true,
      action: {
        label: t("notifications.codexHistorySplit.enable", {
          defaultValue: "开启统一",
        }),
        onClick: () => void enableUnify(),
      },
      cancel: {
        label: t("notifications.codexHistorySplit.dismiss", {
          defaultValue: "不再提示",
        }),
        onClick: () => {
          void settingsApi.dismissCodexHistorySplitHint().catch(() => {
            // 只是少一次「不再提示」，下次跨组还会再提示，不打扰用户
          });
        },
      },
    },
  );
}
