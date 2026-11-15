import type { AppId } from "@/lib/api/types";

/**
 * 每个应用支持哪些功能，全部写在这一张表里。新增应用时 `satisfies` 要求它把每一项都写成
 * true 或 false，漏写一项就是类型错误；下面的应用列表和类型都从这张表派生，不要再手写应用清单。
 *
 * `localProxy`、`routeMode`、`stack`、`additive` 和后端 `AppType::spec` 是同一份事实，
 * `tests/config/appSpecs.json` 由后端测试生成，`tests/config/appSpecs.test.ts` 比对两边。
 */
export interface AppCaps {
  /** 有完整的本地路由和故障转移（后端 `AppSpec::local_proxy`） */
  localProxy: boolean;
  /**
   * 有直连 / 路由模式（后端 `AppSpec::route_mode`）。Claude Desktop 经同一个路由服务转发，
   * 但没有故障转移和路由设置，所以它是 `routeMode` 而不是 `localProxy`。
   */
  routeMode: boolean;
  /** 支持 Stack 模式（后端 `AppSpec::stack`） */
  stack: boolean;
  /**
   * 累加式：供应商并存在一个 live 文件里（后端 `AppSpec::additive`）。Copilot CLI 是混合的：
   * 自带模型卡累加写进 providers.json，官方卡仍是启动式（只有「打开终端」）。
   */
  additive: boolean;
  /** 在 Skills 面板里出现 */
  skills: boolean;
  /** 由 CC Switch 管理 MCP。Claude Desktop、OpenClaw 不管；Pi 1.0 起内置 MCP（`~/.pi/agent/mcp.json`） */
  mcp: boolean;
  /** 支持提示词。Claude Desktop 用 Claude Code 的；OpenClaw 在自己的「工作区」里管理 */
  prompts: boolean;
  /**
   * 能用授权中心里的托管账号。Antigravity 官方卡只算明确绑了号的（没绑的跟着 agy 自己的
   * 登录）；WorkBuddy 官方登录卡一定绑了号。
   */
  managedAccount: boolean;
  /** Pi 一族：Oh My Pi 是 Pi 的分支，供应商表单、启用状态和用量脚本命令与 Pi 共用 */
  piFamily: boolean;
  /**
   * 切换只替换关键字段：供应商编辑器显示「切到这个供应商之后配置文件的样子」，由后端
   * `ProviderService::editor_view` 投影。
   */
  editorView: boolean;
}

/** 键的顺序就是应用的默认顺序（侧栏、应用页）。 */
export const APP_SPECS = {
  claude: {
    localProxy: true,
    routeMode: true,
    stack: true,
    additive: false,
    skills: true,
    mcp: true,
    prompts: true,
    managedAccount: true,
    piFamily: false,
    editorView: true,
  },
  "claude-desktop": {
    localProxy: false,
    routeMode: true,
    stack: true,
    additive: false,
    skills: false,
    mcp: false,
    prompts: false,
    managedAccount: true,
    piFamily: false,
    editorView: false,
  },
  codex: {
    localProxy: true,
    routeMode: true,
    stack: true,
    additive: false,
    skills: true,
    mcp: true,
    prompts: true,
    managedAccount: true,
    piFamily: false,
    editorView: true,
  },
  gemini: {
    localProxy: true,
    routeMode: true,
    stack: false,
    additive: false,
    skills: true,
    mcp: true,
    prompts: true,
    managedAccount: false,
    piFamily: false,
    editorView: true,
  },
  grokbuild: {
    localProxy: true,
    routeMode: true,
    stack: false,
    additive: false,
    skills: true,
    mcp: true,
    prompts: true,
    managedAccount: false,
    piFamily: false,
    editorView: true,
  },
  opencode: {
    localProxy: false,
    routeMode: false,
    stack: false,
    additive: true,
    skills: true,
    mcp: true,
    prompts: true,
    managedAccount: false,
    piFamily: false,
    editorView: false,
  },
  openclaw: {
    localProxy: false,
    routeMode: false,
    stack: false,
    additive: true,
    skills: false,
    mcp: false,
    prompts: false,
    managedAccount: false,
    piFamily: false,
    editorView: false,
  },
  hermes: {
    localProxy: false,
    routeMode: false,
    stack: false,
    additive: true,
    skills: true,
    mcp: true,
    prompts: true,
    managedAccount: false,
    piFamily: false,
    editorView: false,
  },
  pi: {
    localProxy: false,
    routeMode: false,
    stack: false,
    additive: true,
    skills: true,
    mcp: true,
    prompts: true,
    managedAccount: false,
    piFamily: true,
    editorView: false,
  },
  mcode: {
    localProxy: false,
    routeMode: false,
    stack: false,
    additive: true,
    skills: true,
    mcp: true,
    prompts: true,
    managedAccount: false,
    piFamily: false,
    editorView: false,
  },
  kimi: {
    localProxy: false,
    routeMode: false,
    stack: false,
    additive: true,
    skills: true,
    mcp: true,
    prompts: true,
    managedAccount: false,
    piFamily: false,
    editorView: false,
  },
  dsh: {
    localProxy: false,
    routeMode: false,
    stack: false,
    additive: true,
    skills: true,
    mcp: true,
    prompts: true,
    managedAccount: false,
    piFamily: false,
    editorView: false,
  },
  agy: {
    localProxy: false,
    routeMode: false,
    stack: false,
    additive: false,
    skills: true,
    mcp: true,
    prompts: true,
    managedAccount: true,
    piFamily: false,
    editorView: false,
  },
  omp: {
    localProxy: false,
    routeMode: false,
    stack: false,
    additive: true,
    skills: true,
    mcp: true,
    prompts: true,
    managedAccount: false,
    piFamily: true,
    editorView: false,
  },
  copilot: {
    localProxy: false,
    routeMode: false,
    stack: false,
    additive: true,
    skills: true,
    mcp: true,
    prompts: true,
    managedAccount: true,
    piFamily: false,
    editorView: false,
  },
  workbuddy: {
    localProxy: false,
    routeMode: false,
    stack: false,
    additive: true,
    skills: true,
    mcp: true,
    prompts: true,
    managedAccount: true,
    piFamily: false,
    editorView: false,
  },
} as const satisfies Record<AppId, AppCaps>;

/** `APP_SPECS` 里 `cap` 为 true 的应用 ID */
export type AppIdWith<Cap extends keyof AppCaps> = {
  [A in AppId]: (typeof APP_SPECS)[A][Cap] extends true ? A : never;
}[AppId];

/** 所有应用，按默认顺序 */
export const APP_IDS = Object.keys(APP_SPECS) as AppId[];

/** 每项功能为 true 的应用，按默认顺序 */
export const APP_IDS_WITH = Object.fromEntries(
  (Object.keys(APP_SPECS.claude) as (keyof AppCaps)[]).map((cap) => [
    cap,
    APP_IDS.filter((app) => APP_SPECS[app][cap]),
  ]),
) as { [Cap in keyof AppCaps]: AppIdWith<Cap>[] };

/** 有路由模式的应用，本地路由的在前、Claude Desktop 在最后（后端 `route_mode_apps` 同序） */
export const ROUTE_MODE_APP_IDS: AppIdWith<"routeMode">[] = [
  ...APP_IDS_WITH.localProxy,
  ...APP_IDS_WITH.routeMode.filter((app) => !APP_SPECS[app].localProxy),
];

/** `app` 支持 `cap`；不认识的应用 ID 为 false */
export function appHas<Cap extends keyof AppCaps>(
  app: string,
  cap: Cap,
): app is AppIdWith<Cap> {
  return (
    Object.prototype.hasOwnProperty.call(APP_SPECS, app) &&
    APP_SPECS[app as AppId][cap]
  );
}
