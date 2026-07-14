import { settingsApi } from "@/lib/api";

/**
 * 统一的“后置同步”工具：将当前使用的供应商写回对应应用的 live 配置。
 * 不抛出异常，由调用方根据返回值决定提示策略。
 */
export async function syncCurrentProvidersLiveSafe(): Promise<{
  ok: boolean;
  error?: Error;
}> {
  try {
    await settingsApi.syncCurrentProvidersLive();
    return { ok: true };
  } catch (err) {
    const error = err instanceof Error ? err : new Error(String(err ?? ""));
    return { ok: false, error };
  }
}
