import { invoke } from "@tauri-apps/api/core";
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

interface AppUpdateInfo {
  version: string;
  notes: string | null;
  pubDate: string | null;
}

export async function checkForUpdate(
  opts: CheckOptions = {},
): Promise<
  { status: "up-to-date" } | { status: "available"; info: UpdateInfo }
> {
  // 走后端命令而不是插件的 JS check()：只有 Rust 侧的 UpdaterBuilder 能关闭系统代理跟随，
  // 这样「检查更新」和其他出站请求遵守同一套全局出站代理策略。
  const currentVersion = await getCurrentVersion();
  const update = await invoke<AppUpdateInfo | null>("check_app_update", {
    timeoutMs: opts.timeout ?? 30000,
  });

  if (!update) {
    return { status: "up-to-date" };
  }

  return {
    status: "available",
    info: {
      currentVersion,
      availableVersion: update.version,
      notes: update.notes ?? undefined,
      pubDate: update.pubDate ?? undefined,
    },
  };
}
