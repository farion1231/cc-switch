import i18n from "@/i18n";
import { toast } from "@/lib/toast";
import { usageApi } from "@/lib/api/usage";
import { queryClient } from "@/lib/query";
import { MODELS_DEV_SYNC_CONFIG_QUERY_KEY } from "@/lib/modelsDevAutoSync";
import { extractErrorMessage } from "@/utils/errorUtils";

/**
 * CC Switch 自己在后台发的请求（检查更新、同步模型定价）失败时，告诉用户是哪个请求、
 * 连的哪个域名、用来做什么。
 *
 * 防火墙弹窗只显示进程和 IP，应用这边又一声不吭，用户就只能猜「它在偷偷连什么」(#9911)。
 * 失败不阻塞启动（离线照常用），但要说一次：每类失败每次启动最多提示一次。
 */
export type BackgroundTask = "updateCheck" | "modelsDevSync";

const MUTE_KEY_PREFIX = "ccswitch:backgroundTaskNotice:muted:";
const shownThisLaunch = new Set<BackgroundTask>();
const MAX_DETAIL_LENGTH = 300;

function isMuted(task: BackgroundTask): boolean {
  try {
    return localStorage.getItem(MUTE_KEY_PREFIX + task) === "1";
  } catch {
    return false;
  }
}

function mute(task: BackgroundTask) {
  try {
    localStorage.setItem(MUTE_KEY_PREFIX + task, "1");
  } catch {
    // 存不下就下次启动再提示，不影响使用
  }
}

/** 连接层失败（reqwest / 更新插件的原文）：原文又长又只有 URL，换成一句话，原文仍在日志里 */
const NETWORK_FAILURE =
  /error sending request|connect|tunnel|dns error|timed out|timeout/i;

function detailOf(error: unknown): string {
  const text = (extractErrorMessage(error) || String(error)).trim();
  if (NETWORK_FAILURE.test(text)) {
    return i18n.t("backgroundTask.networkError");
  }
  return text.length > MAX_DETAIL_LENGTH
    ? `${text.slice(0, MAX_DETAIL_LENGTH)}…`
    : text;
}

async function disableModelsDevAutoSync() {
  try {
    const state = await usageApi.getModelsDevSyncConfig();
    await usageApi.saveModelsDevSyncConfig({
      ...state.config,
      autoSyncEnabled: false,
    });
    await queryClient.invalidateQueries({
      queryKey: MODELS_DEV_SYNC_CONFIG_QUERY_KEY,
    });
    toast.success(i18n.t("backgroundTask.modelsDevSyncDisabled"), {
      duration: 6000,
    });
  } catch (error) {
    toast.error(
      i18n.t("backgroundTask.disableFailed", { error: detailOf(error) }),
    );
  }
}

export function notifyBackgroundTaskFailed(
  task: BackgroundTask,
  error: unknown,
) {
  if (shownThisLaunch.has(task) || isMuted(task)) return;
  shownThisLaunch.add(task);

  const detail = detailOf(error);
  // 启动时可能正盖着别的对话框，给足时间看清楚、点按钮
  const common = { duration: 20000, closeButton: true } as const;

  if (task === "updateCheck") {
    toast.warning(i18n.t("backgroundTask.updateCheckFailed.title"), {
      ...common,
      description: i18n.t("backgroundTask.updateCheckFailed.description", {
        detail,
      }),
      action: {
        label: i18n.t("backgroundTask.mute"),
        onClick: () => mute(task),
      },
    });
    return;
  }

  toast.warning(i18n.t("backgroundTask.modelsDevSyncFailed.title"), {
    ...common,
    description: i18n.t("backgroundTask.modelsDevSyncFailed.description", {
      detail,
    }),
    action: {
      label: i18n.t("backgroundTask.disableModelsDevSync"),
      onClick: () => void disableModelsDevAutoSync(),
    },
  });
}
