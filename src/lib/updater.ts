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
  const { getGlobalProxyUrl } = await import("./api/globalProxy");

  const currentVersion = await getCurrentVersion();

  // 读取全局代理配置，传递给 updater 插件
  // 注意：tauri-plugin-updater 仅支持 http/https 代理，SOCKS 代理需跳过
  let proxyUrl: string | null = null;
  try {
    const raw = await getGlobalProxyUrl();
    if (raw && /^https?:\/\//i.test(raw)) {
      proxyUrl = raw;
    } else if (raw) {
      console.warn(`[Updater] Unsupported proxy scheme, falling back to direct: ${raw}`);
    }
  } catch {
    // 获取代理失败时静默忽略，使用直连
  }

  const update = await check({
    timeout: opts.timeout ?? 30000,
    ...(proxyUrl ? { proxy: proxyUrl } : {}),
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
