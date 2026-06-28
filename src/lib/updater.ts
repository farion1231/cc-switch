import { getVersion } from "@tauri-apps/api/app";
import { isTauri } from "@/lib/environment";
import { compareVersions } from "@/lib/version";

const GITHUB_RELEASES_API =
  "https://api.github.com/repos/farion1231/cc-switch/releases/latest";

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

interface GitHubRelease {
  tag_name?: string;
  body?: string;
  published_at?: string;
}

async function fetchLatestRelease(): Promise<{
  version: string;
  notes?: string;
  pubDate?: string;
} | null> {
  try {
    const response = await fetch(GITHUB_RELEASES_API);
    if (!response.ok) {
      return null;
    }
    const release = (await response.json()) as GitHubRelease;
    const tagName = release.tag_name ?? "";
    const version = tagName.startsWith("v") ? tagName.slice(1) : tagName;
    return {
      version,
      notes: release.body,
      pubDate: release.published_at,
    };
  } catch {
    return null;
  }
}

export async function getCurrentVersion(): Promise<string> {
  if (!isTauri()) {
    try {
      const response = await fetch("/health");
      if (!response.ok) {
        return "";
      }
      const data = (await response.json()) as { version?: string };
      return data.version ?? "";
    } catch {
      return "";
    }
  }

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
  const currentVersion = await getCurrentVersion();
  if (!currentVersion) {
    return { status: "up-to-date" };
  }

  if (!isTauri()) {
    const latest = await fetchLatestRelease();
    if (!latest) {
      return { status: "up-to-date" };
    }

    if (compareVersions(latest.version, currentVersion) > 0) {
      return {
        status: "available",
        info: {
          currentVersion,
          availableVersion: latest.version,
          notes: latest.notes,
          pubDate: latest.pubDate,
        },
      };
    }

    return { status: "up-to-date" };
  }

  // 动态引入，避免在未安装插件时导致打包期问题
  const { check } = await import("@tauri-apps/plugin-updater");

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
