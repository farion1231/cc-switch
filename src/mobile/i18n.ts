/**
 * Lightweight bilingual (zh / en) dictionary for the Mobile Web UI.
 * Keeps the mobile bundle tiny without pulling all 8 desktop locale JSONs.
 */

export type MobileLang = "zh" | "en";

const LANG_STORAGE_KEY = "cc-switch-mobile-lang";

export function loadMobileLang(): MobileLang {
  try {
    const saved = window.localStorage.getItem(LANG_STORAGE_KEY);
    if (saved === "zh" || saved === "en") return saved;
  } catch {
    // ignore
  }
  const nav =
    typeof navigator !== "undefined" ? navigator.language.toLowerCase() : "en";
  return nav.startsWith("zh") ? "zh" : "en";
}

export function saveMobileLang(lang: MobileLang): void {
  try {
    window.localStorage.setItem(LANG_STORAGE_KEY, lang);
  } catch {
    // ignore
  }
}

const MESSAGES = {
  // Auth
  signInTitle: { en: "Connect to CC Switch", zh: "连接到 CC Switch" },
  signInDesc: {
    en: "Scan the QR code in desktop Settings → Network, or paste your access token below.",
    zh: "扫描桌面端「设置 → 网络」中的二维码，或在下方粘贴访问令牌。",
  },
  tokenLabel: { en: "Access Token", zh: "访问令牌" },
  tokenPlaceholder: {
    en: "Paste 64-character token…",
    zh: "粘贴 64 位访问令牌…",
  },
  signInBtn: { en: "Sign In", zh: "连接" },
  signingIn: { en: "Connecting…", zh: "正在连接…" },
  invalidToken: {
    en: "Invalid access token. Check desktop Settings → Network.",
    zh: "访问令牌无效，请核对桌面端「设置 → 网络」。",
  },
  networkError: {
    en: "Cannot reach CC Switch. Check Wi-Fi / LAN connection.",
    zh: "无法连接到 CC Switch，请检查局域网连接。",
  },

  // Common
  refresh: { en: "Refresh", zh: "刷新" },
  readOnlyBadge: { en: "Read-only", zh: "只读" },
  controlEnabledBadge: { en: "Control on", zh: "可控制" },
  loading: { en: "Loading…", zh: "加载中…" },
  retry: { en: "Retry", zh: "重试" },
  noData: { en: "No data in this period", zh: "当前时间段暂无数据" },
  all: { en: "All", zh: "全部" },
  active: { en: "Active", zh: "当前使用" },
  close: { en: "Close", zh: "关闭" },
  more: { en: "More", zh: "更多" },

  // Overview
  period24h: { en: "24h", zh: "24小时" },
  period7d: { en: "7d", zh: "7天" },
  period30d: { en: "30d", zh: "30天" },
  kpiCost: { en: "Total Cost", zh: "总花费" },
  kpiTokens: { en: "Total Tokens", zh: "总 Tokens" },
  kpiRequests: { en: "Requests", zh: "请求数" },
  kpiCacheHit: { en: "Cache Hit", zh: "缓存命中" },
  kpiSpeed: { en: "Decode Speed", zh: "生成速度" },
  kpiSuccess: { en: "Success Rate", zh: "成功率" },
  dailyTrend: { en: "Daily Cost & Volume", zh: "每日趋势" },
  activeProviders: { en: "Active Providers", zh: "当前供应商" },
  topModels: { en: "Top Models", zh: "热门模型" },

  // Providers
  providersTitle: { en: "Providers", zh: "供应商" },
  readOnlyNotice: {
    en: "Read-only mode. Enable “Allow remote control” in desktop Settings → Network to switch providers from mobile.",
    zh: "当前为只读模式。在桌面端「设置 → 网络」开启「允许远程控制」即可从手机切换供应商。",
  },
  switchBtn: { en: "Switch", zh: "切换" },
  switching: { en: "Switching…", zh: "切换中…" },
  switchedToast: { en: "Switched active provider", zh: "已切换当前供应商" },
  healthy: { en: "Healthy", zh: "正常" },
  degraded: { en: "Degraded", zh: "异常" },
  failoverQueue: { en: "Failover", zh: "故障转移" },
  noProviders: { en: "No providers configured yet", zh: "暂无已配置的供应商" },

  // Logs
  logsTitle: { en: "Request Logs", zh: "请求日志" },
  statusAll: { en: "All", zh: "全部" },
  statusOk: { en: "2xx", zh: "成功" },
  statusErr: { en: "Errors", zh: "错误" },
  prevPage: { en: "Prev", zh: "上一页" },
  nextPage: { en: "Next", zh: "下一页" },
  logDetail: { en: "Request Detail", zh: "请求详情" },
  tokensBreakdown: { en: "Tokens", zh: "Tokens 明细" },
  inputTokens: { en: "Input", zh: "输入" },
  outputTokens: { en: "Output", zh: "输出" },
  cacheRead: { en: "Cache Read", zh: "缓存命中" },
  cacheWrite: { en: "Cache Write", zh: "缓存写入" },
  timingBreakdown: { en: "Timing", zh: "耗时明细" },
  ttft: { en: "TTFT", zh: "首字延迟" },
  totalLatency: { en: "Total Latency", zh: "总耗时" },
  speedTps: { en: "Speed", zh: "速度" },
  cost: { en: "Cost", zh: "费用" },
  errorMessage: { en: "Error", zh: "错误信息" },

  // Proxy
  proxyTitle: { en: "Local Proxy", zh: "本地代理" },
  proxyRunning: { en: "Running", zh: "运行中" },
  proxyStopped: { en: "Stopped", zh: "已停止" },
  listenAddr: { en: "Listen Address", zh: "监听地址" },
  uptime: { en: "Uptime", zh: "运行时长" },
  failovers: { en: "Failovers", zh: "故障转移" },
  takeoverApps: { en: "Routed Apps", zh: "代理接管应用" },
  activeTargets: { en: "Active Targets", zh: "当前代理目标" },
  startProxy: { en: "Start Proxy", zh: "启动代理" },
  stopProxy: { en: "Stop Proxy", zh: "停止代理" },
  noActiveTargets: { en: "No active proxy targets", zh: "暂无活跃代理目标" },

  // Models
  modelsTitle: { en: "Models & Providers", zh: "模型与供应商统计" },
  tabModels: { en: "Models", zh: "按模型" },
  tabProviders: { en: "Providers", zh: "按供应商" },

  // Preferences & Shortcut Bar Customization
  prefsTitle: { en: "Preferences", zh: "偏好设置" },
  appearance: { en: "Appearance", zh: "外观主题" },
  themeSystem: { en: "System", zh: "跟随系统" },
  themeLight: { en: "Light", zh: "浅色" },
  themeDark: { en: "Dark", zh: "深色" },
  language: { en: "Language", zh: "语言" },
  autoRefresh: { en: "Auto Refresh", zh: "自动刷新" },
  refreshOff: { en: "Off", zh: "关闭" },
  shortcutBarSection: { en: "Bottom Shortcut Bar", zh: "底部快捷栏" },
  customizeBarBtn: { en: "Customize Shortcut Bar", zh: "自定义底部快捷栏" },
  customizeBarSub: {
    en: "Choose which tabs stay pinned and drag or tap arrows to reorder.",
    zh: "自由选择固定在底部的页面，并调整顺序。",
  },
  customizeTitle: { en: "Customize Shortcut Bar", zh: "自定义底部快捷栏" },
  previewLabel: { en: "Live Preview", zh: "实时预览" },
  pinnedSection: { en: "Pinned Tabs", zh: "已固定" },
  availableSection: { en: "In “More” Menu", zh: "收纳在「更多」" },
  pinAction: { en: "Pin", zh: "固定" },
  unpinAction: { en: "Remove", zh: "移除" },
  moveLeft: { en: "Move earlier", zh: "前移" },
  moveRight: { en: "Move later", zh: "后移" },
  resetDefault: { en: "Reset to Default", zh: "恢复默认" },
  doneBtn: { en: "Done", zh: "完成" },
  slotsHint: {
    en: "Up to 4 pinned tabs + More button (5 slots total).",
    zh: "最多固定 4 个标签 + 「更多」按钮（共 5 格）。",
  },
  moreSheetTitle: { en: "More Pages", zh: "更多页面" },
  sessionSection: { en: "Session", zh: "当前会话" },
  signOutBtn: { en: "Sign Out", zh: "退出登录" },

  // Unified Multi-Agent Remote Control
  remoteTitle: { en: "Remote Control", zh: "Agent 远控" },
  remoteNewSession: { en: "New Session", zh: "新建会话" },
  remoteSearchPlaceholder: {
    en: "Search sessions or folders…",
    zh: "搜索会话或工作目录…",
  },
  remoteNoSessions: {
    en: "No sessions found for the selected agent.",
    zh: "当前筛选下暂无历史会话。",
  },
  remoteActiveRuns: { en: "Running Now", zh: "正在运行" },
  remoteBack: { en: "Sessions", zh: "返回列表" },
  remoteStop: { en: "Stop", zh: "中断" },
  remoteStopping: { en: "Stopping…", zh: "中断中…" },
  remoteSend: { en: "Send", zh: "发送" },
  remotePromptPlaceholder: {
    en: "Send a message to the agent…",
    zh: "向 Agent 发送指令…",
  },
  remoteChooseAgent: { en: "Agent", zh: "选择 Agent" },
  remoteWorkspace: { en: "Working Directory", zh: "工作目录" },
  remoteWorkspacePlaceholder: {
    en: "/path/to/workspace",
    zh: "输入工作目录路径…",
  },
  remoteRecentWorkspaces: { en: "Recent Workspaces", zh: "最近目录" },
  remoteStartBtn: { en: "Start Run", zh: "开始执行" },
  remoteStarting: { en: "Starting…", zh: "启动中…" },
  remoteModel: { en: "Model", zh: "模型" },
  remoteModelPlaceholder: { en: "Default model", zh: "默认模型" },
  remoteMode: { en: "Mode", zh: "模式" },
  remoteEffort: { en: "Effort", zh: "思考深度" },
  remoteSandbox: { en: "Sandbox", zh: "沙盒" },
  remoteOptions: { en: "Options", zh: "参数" },
  remoteThinking: { en: "Thinking", zh: "思考过程" },
  remoteToolCall: { en: "Tool Call", zh: "调用工具" },
  remoteToolResult: { en: "Tool Result", zh: "工具结果" },
  remoteStderr: { en: "Diagnostics", zh: "诊断输出" },
  remoteStatusRunning: { en: "Running", zh: "运行中" },
  remoteStatusCompleted: { en: "Completed", zh: "已完成" },
  remoteStatusError: { en: "Error", zh: "出错" },
  remoteStatusCancelled: { en: "Cancelled", zh: "已中断" },
  remoteReadOnlyAgent: {
    en: "Remote execution is disabled for this agent. Enable it in desktop Settings → Network → Agent Remote Control.",
    zh: "该 Agent 尚未开启远控权限。请在桌面端「设置 → 网络 → Agent 远控」中勾选启用。",
  },
  remoteNotInstalledAgent: {
    en: "CLI binary is not installed on the host machine.",
    zh: "宿主机未检测到该 Agent 的 CLI 程序。",
  },
  remoteNotResumable: {
    en: "This session is read-only and cannot be resumed from CLI.",
    zh: "该会话为外部托管会话，仅支持只读浏览，无法通过 CLI 继续对话。",
  },
  remoteNoEnabledAgents: {
    en: "No agents enabled for remote control yet. Enable one or more agents in desktop Settings → Network.",
    zh: "暂未启用任何 Agent 远控。请在桌面端「设置 → 网络」勾选允许远控的 Agent。",
  },
  remoteNoMessages: {
    en: "No transcript messages yet.",
    zh: "暂无对话记录。",
  },
  remoteEnabledTag: { en: "Enabled", zh: "已启用" },
  remoteReadOnlyTag: { en: "Read-only", zh: "只读" },
  remoteNotInstalledTag: { en: "Not installed", zh: "未安装" },
  remoteUntitled: { en: "Untitled Session", zh: "未命名会话" },
} as const;

export type MobileMessageKey = keyof typeof MESSAGES;

export function tr(lang: MobileLang, key: MobileMessageKey): string {
  return MESSAGES[key][lang];
}
