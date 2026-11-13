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
  /** `route`：应用在路由 / 聚合模式时这个环境接入共用的路由（主环境总是跟随） */
  windows: { enabled: boolean; dir?: string | null; route?: boolean };
  wsl: {
    distro: string;
    enabled: boolean;
    dir?: string | null;
    route?: boolean;
  }[];
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
  /** 客户端文件现在是共用路由的代理配置 */
  routed?: boolean;
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

export type InstallStatus = "installed" | "broken" | "notInstalled" | "unknown";

export interface ToolInstall {
  status: InstallStatus;
  version?: string;
  error?: string;
  detectedAt: number;
  /** 发行版没在运行，用的是上次检测结果 */
  cached: boolean;
}

export interface AppTargetsOverview {
  inventory: WslInventory;
  /** 发行版 → 应用 → 安装情况 */
  installs: Record<string, Record<string, ToolInstall>>;
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
  /** 保存后的主环境（自动：勾选了 Windows 就是 Windows） */
  primaryTarget: TargetKey;
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

export interface WslRouteInfo {
  distro: string;
  savedHost?: string;
  detectedHost?: string;
  networkingMode?: string;
  listenAddress: string;
  /** 监听只在回环上而这个发行版要经 Windows 网关访问：连不上 */
  listenUnreachable: boolean;
}

export interface RouteTestResult {
  ok: boolean;
  url: string;
  detail: string;
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

  /** 在某个环境里启用供应商（供应商页的环境开关）。 */
  async activate(
    app: AppTargetAppId,
    target: TargetKey,
    providerId: string,
  ): Promise<TargetReport> {
    return await invoke("activate_app_target_provider", {
      app,
      target,
      providerId,
    });
  },

  /** 某个环境是否接入共用的路由 / 聚合 */
  async setRoute(
    app: AppTargetAppId,
    target: TargetKey,
    join: boolean,
  ): Promise<TargetReport> {
    return await invoke("set_app_target_route", { app, target, join });
  },

  async wslRouteInfo(probe = false): Promise<WslRouteInfo[]> {
    return await invoke("get_wsl_route_info", { probe });
  },

  async setWslClientHost(distro: string, host: string): Promise<void> {
    return await invoke("set_wsl_client_host", { distro, host });
  },

  async testWslRoute(distro: string): Promise<RouteTestResult> {
    return await invoke("test_wsl_route", { distro });
  },

  /** 在某个 WSL 发行版里安装 / 更新客户端（应用页的 WSL 安装条目） */
  async wslToolAction(
    tool: AppTargetAppId,
    distro: string,
    action: "install" | "update",
  ): Promise<void> {
    return await invoke("run_wsl_tool_lifecycle_action", {
      tool,
      distro,
      action,
    });
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

/** 跟随环境的「接入路由」偏好 */
export const routeFlag = (config: AppTargetsConfig, key: TargetKey) => {
  const distro = distroOf(key);
  if (!distro) return Boolean(config.windows.route);
  return Boolean(
    config.wsl.find((t) => t.distro.toLowerCase() === distro.toLowerCase())
      ?.route,
  );
};

/** 某个发行版里装没装这个应用：`undefined` 表示还没检测出来。 */
export function installedIn(
  installs: AppTargetsOverview["installs"] | undefined,
  distro: string,
  app: string,
): boolean | undefined {
  const entry = Object.entries(installs ?? {}).find(
    ([name]) => name.toLowerCase() === distro.toLowerCase(),
  )?.[1]?.[app];
  if (!entry || entry.status === "unknown") return undefined;
  return entry.status !== "notInstalled";
}

export function installOf(
  installs: AppTargetsOverview["installs"] | undefined,
  distro: string,
  app: string,
): ToolInstall | undefined {
  return Object.entries(installs ?? {}).find(
    ([name]) => name.toLowerCase() === distro.toLowerCase(),
  )?.[1]?.[app];
}

/**
 * 应用的 WSL 配置入口里列哪些发行版：已经保存在目标配置里的，加上检测到装了这个客户端的。
 */
export function wslCandidates(
  config: AppTargetsConfig,
  overview: Pick<AppTargetsOverview, "installs" | "inventory"> | undefined,
  app: string,
): string[] {
  const saved = config.wsl.map((t) => t.distro);
  const detected = (overview?.inventory.distros ?? [])
    .map((env) => env.distro)
    .filter(
      (distro) =>
        installedIn(overview?.installs, distro, app) === true &&
        !saved.some((s) => s.toLowerCase() === distro.toLowerCase()),
    );
  return [...saved, ...detected];
}
