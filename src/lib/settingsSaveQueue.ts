import { settingsApi } from "@/lib/api";
import type { Settings } from "@/types";

// 全局共享的串行保存队列：所有走 save_settings 的入口共用同一条 promise 链，
// 任务在轮到它时才读取最新后端设置，再合并本次字段后保存。这样多个入口
// （使用统计首页、设置页首页模式、General 自动保存）并发触发时不会基于
// 各自的旧快照互相覆盖。
let chain: Promise<SettingsSaveResult> = Promise.resolve({ ok: true });

export interface SettingsSaveResult {
  ok: boolean;
  /** 保存前从后端读取的最新设置（副作用字段对比用） */
  fresh?: Settings;
  /** fresh 合并本次 updates 后的完整设置（已落盘） */
  merged?: Settings;
}

/**
 * 入队一次设置保存。队列执行时才读取最新后端设置并合并 updates。
 * 返回保存是否成功以及保存前后上下文；成功与否不影响后续任务，
 * 失败不写任何数据，避免基于旧快照覆盖。
 */
export function enqueueSettingsSave(
  updates: Partial<Settings>,
): Promise<SettingsSaveResult> {
  const run = async (): Promise<SettingsSaveResult> => {
    try {
      const fresh = await settingsApi.get();
      const merged: Settings = { ...fresh, ...updates };
      await settingsApi.save(merged);
      return { ok: true, fresh, merged };
    } catch (error) {
      console.error("[settingsSaveQueue] Failed to save settings", error);
      return { ok: false };
    }
  };
  const next = chain.then(run, run);
  chain = next.then(
    () => ({ ok: true }),
    () => ({ ok: true }),
  );
  return next;
}
