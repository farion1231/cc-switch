import { getVersion } from "@tauri-apps/api/app";

export type UpdateChannel = "stable" | "beta";

export interface UpdateInfo {
  currentVersion: string;
  availableVersion: string;
  notes?: string;
  pubDate?: string;
}

/** 自动更新在后台下好、等用户确认安装的版本（后端 `app_update::ReadyUpdate`）。 */
export interface ReadyUpdate {
  version: string;
  notes?: string | null;
}

/** 后端 `app_update::AutoUpdateStatus`。 */
export interface AutoUpdateStatus {
  /** 这台机器能不能自动安装（便携版、Linux 非 AppImage 不行） */
  supported: boolean;
  downloading: boolean;
  ready: ReadyUpdate | null;
  /** 有应用在用路由服务：重启会切断进行中的请求 */
  routingInUse: boolean;
}

/** 待安装版本变了的事件，载荷是 {@link ReadyUpdate}；null 表示下好的版本被撤回 */
export const UPDATE_READY_EVENT = "app-update-ready";

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
