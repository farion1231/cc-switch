import { invoke } from "@tauri-apps/api/core";

/** 支持插件的应用（值和 `AppId` 一致）。 */
export type PluginApp =
  | "claude"
  | "codex"
  | "grokbuild"
  | "mcode"
  | "openclaw"
  | "pi";

export const PLUGIN_APPS: PluginApp[] = [
  "claude",
  "codex",
  "grokbuild",
  "mcode",
  "openclaw",
  "pi",
];

export const isPluginApp = (app: string): app is PluginApp =>
  (PLUGIN_APPS as string[]).includes(app);

/** 各应用的 CLI 支持哪些操作；不支持的入口不显示。 */
export interface PluginAppFeatures {
  /** 有「市场」页：能添加、移除市场 */
  marketplaces: boolean;
  /** 能从来源刷新市场目录 */
  refresh: boolean;
  /** 能更新已装的插件 */
  update: boolean;
  /** CLI 要求显式确认信任才肯安装，安装前先让用户确认 */
  confirmInstall: boolean;
  /** 改动什么时候生效 */
  restartNote: "session" | "gateway";
}

export const PLUGIN_APP_FEATURES: Record<PluginApp, PluginAppFeatures> = {
  claude: {
    marketplaces: true,
    refresh: true,
    update: true,
    confirmInstall: false,
    restartNote: "session",
  },
  codex: {
    marketplaces: true,
    refresh: true,
    update: true,
    confirmInstall: false,
    restartNote: "session",
  },
  // `grok plugin install` 不带 --trust 不装
  grokbuild: {
    marketplaces: true,
    refresh: true,
    update: true,
    confirmInstall: true,
    restartNote: "session",
  },
  // 没有更新命令；正式版只有官方市场
  mcode: {
    marketplaces: false,
    refresh: false,
    update: false,
    confirmInstall: false,
    restartNote: "session",
  },
  // 目录是 ClawHub 托管的，不能增删市场；安装要确认插件声明的权限
  openclaw: {
    marketplaces: false,
    refresh: true,
    update: true,
    confirmInstall: true,
    restartNote: "gateway",
  },
  // 插件是 Pi 包：目录取自 npm（pi-package 关键词），没有市场；包能执行代码，安装前先确认
  pi: {
    marketplaces: false,
    refresh: true,
    update: true,
    confirmInstall: true,
    restartNote: "session",
  },
};

/** 工具没装时后端返回的错误码。 */
export const PLUGIN_CLI_NOT_INSTALLED = "PLUGIN_CLI_NOT_INSTALLED";

/** Claude Code 要用户在终端里亲自确认市场声明的命令时后端返回的错误码（CC Switch 不代为同意）。 */
export const PLUGIN_NEEDS_TERMINAL_CONFIRM = "PLUGIN_NEEDS_TERMINAL_CONFIRM";

/** CLI 要求先登录账号（MiniMax Code 未登录时安装插件）；原样来自 CLI 的 stderr。 */
export const PLUGIN_AUTH_REQUIRED = "PLUGIN_AUTH_REQUIRED";

/** 各 CLI 的登录命令，提示用户去终端登录时用。 */
export const PLUGIN_LOGIN_COMMAND: Partial<Record<PluginApp, string>> = {
  mcode: "mcode login",
};

export interface PluginEntry {
  /** 交给 CLI 的 id：多数应用是 `name@marketplace`，Grok Build 是插件名，OpenClaw 是插件 id 或 ClawHub 包名，Pi 是包来源（`npm:<包名>`） */
  id: string;
  name: string;
  /** 来自哪个市场；没有市场概念的为空串 */
  marketplace: string;
  displayName?: string | null;
  version?: string | null;
  description?: string | null;
  category?: string | null;
  installed: boolean;
  enabled: boolean;
  /** Claude Code 的安装范围（user / project / local / managed） */
  scope?: string | null;
  /** CC Switch 能否启停（Claude Code 只管 user 范围） */
  manageable: boolean;
  /** 能否卸载、更新（OpenClaw 自带的插件只能开关）；旧缓存里没有这个字段时按 manageable 算 */
  removable?: boolean;
  /** 市场是否允许安装 */
  installable: boolean;
  /** 目录条目装好后在已安装列表里的 id（和 id 不同时才有：OpenClaw 的包名、Pi 已按别的写法装过的来源） */
  installedId?: string | null;
  installPath?: string | null;
  updatedAt?: string | null;
}

export interface PluginMarketplace {
  name: string;
  source?: string | null;
  location?: string | null;
  removable: boolean;
}

export interface PluginActionOutcome {
  changed: boolean;
  message?: string | null;
}

export const pluginsApi = {
  /** 已安装的插件（便宜，每次操作后都会重读） */
  listInstalled: (app: PluginApp) =>
    invoke<PluginEntry[]>("list_installed_plugins", { app }),
  /** 所有市场里的插件（含已安装的；大，只在市场变动后重读） */
  listCatalog: (app: PluginApp) =>
    invoke<PluginEntry[]>("list_plugin_catalog", { app }),
  listMarketplaces: (app: PluginApp) =>
    invoke<PluginMarketplace[]>("list_plugin_marketplaces", { app }),
  install: (app: PluginApp, id: string) =>
    invoke<PluginActionOutcome>("install_plugin", { app, id }),
  uninstall: (app: PluginApp, id: string) =>
    invoke<PluginActionOutcome>("uninstall_plugin", { app, id }),
  setEnabled: (app: PluginApp, id: string, enabled: boolean) =>
    invoke<PluginActionOutcome>("set_plugin_enabled", { app, id, enabled }),
  update: (app: PluginApp, id: string) =>
    invoke<PluginActionOutcome>("update_plugin", { app, id }),
  addMarketplace: (app: PluginApp, source: string) =>
    invoke<PluginActionOutcome>("add_plugin_marketplace", { app, source }),
  removeMarketplace: (app: PluginApp, name: string) =>
    invoke<PluginActionOutcome>("remove_plugin_marketplace", { app, name }),
  refreshMarketplaces: (app: PluginApp) =>
    invoke<PluginActionOutcome>("refresh_plugin_marketplaces", { app }),
};
