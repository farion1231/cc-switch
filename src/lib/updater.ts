import { getVersion } from "@tauri-apps/api/app";
import { getGlobalProxyUrl } from "@/lib/api/globalProxy";

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

  // 让更新检查/下载也走全局出站代理：JS 插件的 check({ proxy }) 会把代理传给
  // 插件的 plugin:updater|check 命令，并随 Update 资源带到下载与安装阶段。
  let proxyUrl: string | null = null;
  try {
    proxyUrl = await getGlobalProxyUrl();
  } catch (err) {
    console.warn("获取全局代理失败，更新检查将直连:", err);
  }

  const update = await check({
    timeout: opts.timeout ?? 30000,
    proxy: proxyUrl ?? undefined,
  } as any);

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
