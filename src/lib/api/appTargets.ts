import { invoke } from "@tauri-apps/api/core";

/** 第一阶段支持多环境写入的应用；其他应用沿用单目录。 */
export const APP_TARGET_APPS = [
  "claude",
  "codex",
  "gemini",
  "opencode",
] as const;
export type AppTargetAppId = (typeof APP_TARGET_APPS)[number];

export const isAppTargetApp = (app: string): app is AppTargetAppId =>
  (APP_TARGET_APPS as readonly string[]).includes(app);

/** `windows` 或 `wsl:<发行版>` */
export type TargetKey = string;
export const WINDOWS_TARGET = "windows";
export const wslTargetKey = (distro: string): TargetKey => `wsl:${distro}`;
export const distroOf = (key: TargetKey): string | undefined =>
  key.startsWith("wsl:") ? key.slice(4) : undefined;
export const sameTarget = (a: TargetKey, b: TargetKey) =>
  a === b ||
  (distroOf(a) !== undefined &&
    distroOf(a)!.toLowerCase() === distroOf(b)?.toLowerCase());

export interface AppTargetsConfig {
  primary: TargetKey;
  windows: { enabled: boolean; dir?: string | null };
  wsl: { distro: string; enabled: boolean; dir?: string | null }[];
  [extra: string]: unknown;
}

export type ConfigSource = "saved" | "legacy" | "legacyUnresolved";

export type TargetStatus = "ok" | "error" | "deferred" | "skipped" | "external";

export interface TargetRecord {
  status: TargetStatus;
  reason?: string;
  detail?: string;
  providerId?: string;
  fingerprint?: string;
  dir?: string;
  warnings?: string[];
  at: number;
}

export interface TargetReport extends TargetRecord {
  target: TargetKey;
}

export interface WslEnvironment {
  distro: string;
  available: boolean;
  running: boolean;
  home?: string;
  claudeConfigDir?: string;
  codexHome?: string;
  xdgConfigHome?: string;
  cached: boolean;
  detectedAt?: number;
  error?: string;
}

export interface WslInventory {
  hostWindows: boolean;
  wslAvailable: boolean;
  distros: WslEnvironment[];
  error?: string;
}

export interface AppTargetsView {
  app: AppTargetAppId;
  source: ConfigSource;
  config: AppTargetsConfig;
  resolvedDirs: Record<TargetKey, string>;
  records: Record<TargetKey, TargetRecord>;
}

export interface AppTargetsOverview {
  inventory: WslInventory;
  multiEnv: boolean;
  apps: AppTargetsView[];
}

export interface FieldError {
  target?: TargetKey;
  code: string;
}

export interface TargetConflict {
  target: TargetKey;
  dir: string;
  existingProviderId?: string;
  existingProviderName?: string;
  existingHost?: string;
  incomingProviderId?: string;
  incomingProviderName?: string;
}

export interface InspectResult {
  errors: FieldError[];
  resolvedDirs: Record<TargetKey, string>;
  conflicts: TargetConflict[];
  blocked?: string;
}

export type Resolution = "defer" | "overwrite" | "import";

export interface SaveResult extends InspectResult {
  saved: boolean;
  needsResolution: boolean;
  imported: string[];
  primary?: TargetReport;
  targets: TargetReport[];
}

export interface SyncResult {
  primary?: TargetReport;
  targets: TargetReport[];
}

export const appTargetsApi = {
  async overview(probeStopped = false): Promise<AppTargetsOverview> {
    return await invoke("get_app_targets_overview", { probeStopped });
  },

  async inspect(
    app: AppTargetAppId,
    config: AppTargetsConfig,
  ): Promise<InspectResult> {
    return await invoke("inspect_app_targets", { app, config });
  },

  async save(
    app: AppTargetAppId,
    config: AppTargetsConfig,
    resolutions: Record<TargetKey, Resolution>,
  ): Promise<SaveResult> {
    return await invoke("save_app_targets", { app, config, resolutions });
  },

  async sync(app: AppTargetAppId, targets?: TargetKey[]): Promise<SyncResult> {
    return await invoke("sync_app_targets", { app, targets });
  },
};

/** 只启用 Windows、Windows 为主环境：和旧单目录行为一样，主页不显示写入目标入口。 */
export const isWindowsOnly = (config: AppTargetsConfig) =>
  config.primary === WINDOWS_TARGET &&
  config.windows.enabled &&
  config.wsl.every((target) => !target.enabled);

export const enabledTargetKeys = (config: AppTargetsConfig): TargetKey[] => [
  ...(config.windows.enabled ? [WINDOWS_TARGET] : []),
  ...config.wsl.filter((t) => t.enabled).map((t) => wslTargetKey(t.distro)),
];
