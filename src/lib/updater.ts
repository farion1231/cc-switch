import { getVersion } from "@tauri-apps/api/app";

/**
 * P0-01：XSEC Agent Shield 的发布身份、签名主体与更新服务器尚未确认。
 * 在发布配置完成前，本构建禁止任何更新检查/安装：前端不发起调用，
 * 后端命令同样返回禁用（双保险，不是只删端点 URL）。
 */
export const UPDATE_CHANNEL_CONFIGURED = false;

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
  if (!UPDATE_CHANNEL_CONFIGURED) {
    throw new Error("更新渠道未配置：此开发构建不提供应用更新");
  }

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
