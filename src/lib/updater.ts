import { getVersion } from "@tauri-apps/api/app";

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
  const update = await check({ timeout: opts.timeout ?? 30000 });

  if (!update) {
    return { status: "up-to-date" };
  }

  try {
    return {
      status: "available",
      info: {
        currentVersion: update.currentVersion || currentVersion,
        availableVersion: update.version,
        notes: update.body,
        pubDate: update.date,
      },
    };
  } finally {
    // 检测只读取元数据，不下载；及时释放插件持有的更新资源。
    await update.close().catch(console.error);
  }
}
