import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";

export type UpdateChannel = "stable" | "beta";

export interface UpdateInfo {
  currentVersion: string;
  availableVersion: string;
  notes?: string;
  pubDate?: string;
}

export interface CheckOptions {
  timeout?: number;
  channel?: UpdateChannel;
}

export async function getCurrentVersion(): Promise<string> {
  try {
    return await getVersion();
  } catch {
    return "";
  }
}

export async function checkForUpdate(
  opts: CheckOptions = {},
): Promise<
  { status: "up-to-date" } | { status: "available"; info: UpdateInfo }
> {
  // 动态引入，避免在未安装插件时导致打包期问题
  const { check } = await import("@tauri-apps/plugin-updater");

  const currentVersion = await getCurrentVersion();
  const update = await check({ timeout: opts.timeout ?? 30000 } as any);

  if (!update) {
    return { status: "up-to-date" };
  }

  const info: UpdateInfo = {
    currentVersion,
    availableVersion: (update as any).version ?? "",
    notes: (update as any).notes,
    pubDate: (update as any).date,
  };

  return { status: "available", info };
}

/**
 * 让后端在后台预下载当前可用的更新安装包（写入应用缓存目录）。
 * 检查到新版本后调用一次即可；后端自行去重（同版本只下一次）。
 * 失败静默即可——安装命令在预下载不可用时回退为现场下载。
 */
export async function prestageUpdate(): Promise<void> {
  await invoke("prestage_app_update");
}
