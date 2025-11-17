import { invoke } from "@tauri-apps/api/core";
import type { EnvConflict, BackupInfo } from "@/types/env";

/**
 * 环境变量管理 API
 */

/**
 * 检查指定应用的环境变量冲突
 * @param appType 应用类型 ("claude" | "codex" | "gemini")
 * @returns 环境变量冲突列表
 */
export async function checkEnvConflicts(
  appType: string,
): Promise<EnvConflict[]> {
  return invoke<EnvConflict[]>("check_env_conflicts", { app: appType });
}

/**
 * 删除指定的环境变量 (会自动备份)
 * @param conflicts 要删除的环境变量冲突列表
 * @returns 备份信息
 */
export async function deleteEnvVars(
  conflicts: EnvConflict[],
): Promise<BackupInfo> {
  return invoke<BackupInfo>("delete_env_vars", { conflicts });
}

/**
 * 从备份文件恢复环境变量
 * @param backupPath 备份文件路径
 */
export async function restoreEnvBackup(backupPath: string): Promise<void> {
  return invoke<void>("restore_env_backup", { backupPath });
}

/**
 * 检查所有应用的环境变量冲突
 * @returns 按应用类型分组的环境变量冲突
 */
export async function checkAllEnvConflicts(): Promise<
  Record<string, EnvConflict[]>
> {
  const apps = ["claude", "codex", "gemini"];
  const results: Record<string, EnvConflict[]> = {};

  await Promise.all(
    apps.map(async (app) => {
      try {
        results[app] = await checkEnvConflicts(app);
      } catch (error) {
        console.error(`检查 ${app} 环境变量失败:`, error);
        results[app] = [];
      }
    }),
  );

  return results;
}
