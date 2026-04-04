import { getVersion } from "@tauri-apps/api/app";

// eslint-disable-next-line @typescript-eslint/consistent-type-imports
import type { Update } from "@tauri-apps/plugin-updater";

export type UpdateChannel = "stable" | "beta";

export type UpdaterPhase =
  | "idle"
  | "checking"
  | "available"
  | "downloading"
  | "installing"
  | "restarting"
  | "upToDate"
  | "error";

export interface UpdateInfo {
  currentVersion: string;
  availableVersion: string;
  notes?: string;
  pubDate?: string;
}

export interface UpdateProgressEvent {
  event: "Started" | "Progress" | "Finished";
  total?: number;
  downloaded?: number;
}

export interface UpdateHandle {
  version: string;
  notes?: string;
  date?: string;
  downloadAndInstall: (
    onProgress?: (e: UpdateProgressEvent) => void,
  ) => Promise<void>;
  download?: () => Promise<void>;
  install?: () => Promise<void>;
}

export interface CheckOptions {
  timeout?: number;
  channel?: UpdateChannel;
}

function mapUpdateHandle(raw: Update): UpdateHandle {
  return {
    version: (raw as any).version ?? "",
    notes: (raw as any).notes,
    date: (raw as any).date,
    async downloadAndInstall(onProgress?: (e: UpdateProgressEvent) => void) {
      await (raw as any).downloadAndInstall((evt: any) => {
        if (!onProgress) return;
        const mapped: UpdateProgressEvent = {
          event: evt?.event,
        };
        if (evt?.event === "Started") {
          mapped.total = evt?.data?.contentLength ?? 0;
          mapped.downloaded = 0;
        } else if (evt?.event === "Progress") {
          mapped.downloaded = evt?.data?.chunkLength ?? 0;
        }
        onProgress(mapped);
      });
    },
    download: (raw as any).download
      ? async () => {
          await (raw as any).download();
        }
      : undefined,
    install: (raw as any).install
      ? async () => {
          await (raw as any).install();
        }
      : undefined,
  };
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
  | { status: "up-to-date" }
  | { status: "available"; info: UpdateInfo; update: UpdateHandle }
> {
  const { check } = await import("@tauri-apps/plugin-updater");

  const currentVersion = await getCurrentVersion();
  const update = await check({ timeout: opts.timeout ?? 30000 } as any);

  if (!update) {
    return { status: "up-to-date" };
  }

  const mapped = mapUpdateHandle(update);
  const info: UpdateInfo = {
    currentVersion,
    availableVersion: mapped.version,
    notes: mapped.notes,
    pubDate: mapped.date,
  };

  return { status: "available", info, update: mapped };
}

export async function relaunchApp(): Promise<void> {
  const { relaunch } = await import("@tauri-apps/plugin-process");
  await relaunch();
}
