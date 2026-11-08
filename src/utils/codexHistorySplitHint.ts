import type { TFunction } from "i18next";
import type { ExternalToast } from "sonner";
import { toast } from "@/lib/toast";
import type { CodexHistorySplitHint } from "@/lib/api/providers";

/** 后端 `codex-unify-migration-finished` 事件：开启统一后的后台迁入跑完了（或失败）。 */
export const CODEX_UNIFY_MIGRATION_FINISHED_EVENT =
  "codex-unify-migration-finished";

export interface CodexUnifyMigrationFinished {
  migratedStateRows: number;
  migratedJsonlFiles: number;
  error?: string | null;
}

/** 迁完才算数：此刻重启 Codex，原来锁住的旧会话才能继续。 */
export function showCodexUnifyMigrationFinished(
  payload: CodexUnifyMigrationFinished,
  t: TFunction,
) {
  if (payload.error) {
    toast.error(
      t("notifications.codexHistorySplit.migrationFailed", {
        defaultValue:
          "迁入旧会话失败：{{error}}。下次启动 CC Switch 会自动重试",
        error: payload.error,
      }),
      { duration: 10000, closeButton: true },
    );
    return;
  }
  // state DB 读不到时只剩 jsonl 的计数
  const count = payload.migratedStateRows || payload.migratedJsonlFiles;
  if (count <= 0) return;
  toast.success(
    t("notifications.codexHistorySplit.migrationFinished", {
      defaultValue: "已迁入 {{count}} 个旧会话，重启 Codex 后即可继续",
      count,
    }),
    { duration: 10000, closeButton: true },
  );
}

/**
 * Codex 切换跨了组（官方 openai ↔ 第三方 custom）、统一会话历史又关着时，在「切换成功」
 * 那条提示下面多一行：新版 Codex 只能继续当前组的会话，另一组只能查看。只是提醒，
 * 不带按钮（要合并两组去设置里开「统一 Codex 会话历史」，开启会先备份会话记录），
 * 多停几秒让人看得完。
 */
export function codexHistorySplitToastExtras(
  hint: CodexHistorySplitHint,
  t: TFunction,
): Pick<ExternalToast, "description" | "duration"> {
  const previous =
    hint.previousBucket === "openai"
      ? t("notifications.codexHistorySplit.officialBucket", {
          defaultValue: "官方订阅",
        })
      : t("notifications.codexHistorySplit.thirdPartyBucket", {
          defaultValue: "第三方供应商",
        });

  return {
    description: t("notifications.codexHistorySplit.hint", {
      defaultValue: "之前在{{previous}}下的 {{count}} 个会话现在只能查看",
      previous,
      count: hint.lockedThreads,
    }),
    duration: 6000,
  };
}
