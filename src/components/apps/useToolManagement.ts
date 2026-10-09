import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
} from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { settingsApi } from "@/lib/api";
import type {
  ToolInstallation,
  ToolInstallationReport,
} from "@/lib/api/settings";
import type { AppId } from "@/lib/api/types";
import { extractErrorMessage } from "@/utils/errorUtils";
import { isWindows } from "@/lib/platform";
import { isUpdateAvailable } from "@/lib/version";

/**
 * 「应用」页的安装 / 升级逻辑（原来在设置 → 关于）。任务、确认队列和版本结果放在模块级的
 * store 里：离开页面也不中断，回来立刻恢复进度。
 */

export interface ToolVersion {
  name: string;
  version: string | null;
  latest_version: string | null;
  error: string | null;
  // 后端已定位到可执行文件但 --version 报错（装了却跑不起来）。直接读此字段，
  // 不要靠匹配 error 文案反推——避免前端与后端字符串硬耦合。
  installed_but_broken: boolean;
  env_type: "windows" | "wsl" | "macos" | "linux" | "unknown";
  wsl_distro: string | null;
}

export const TOOL_NAMES = [
  "claude",
  "codex",
  "gemini",
  "grok",
  "opencode",
  "openclaw",
  "hermes",
  "pi",
  "mcode",
] as const;
export type ToolName = (typeof TOOL_NAMES)[number];
export type ToolLifecycleAction = "install" | "update";

interface PendingUpgrade {
  toolNames: ToolName[];
  plans: ToolInstallationReport[];
  fromBatchEntry: boolean;
  wslShellByTool: Record<string, WslShellPreference>;
}

export type WslShellPreference = {
  wslShell?: string | null;
  wslShellFlag?: string | null;
};

export const WSL_SHELL_OPTIONS = ["sh", "bash", "zsh", "fish", "dash"] as const;
// UI-friendly order: login shell first.
export const WSL_SHELL_FLAG_OPTIONS = ["-lic", "-lc", "-c"] as const;

export const ENV_BADGE_CONFIG: Record<
  string,
  { labelKey: string; className: string }
> = {
  wsl: {
    labelKey: "settings.envBadge.wsl",
    className:
      "bg-orange-500/10 text-orange-600 dark:text-orange-400 border-orange-500/20",
  },
  windows: {
    labelKey: "settings.envBadge.windows",
    className:
      "bg-blue-500/10 text-blue-600 dark:text-blue-400 border-blue-500/20",
  },
  macos: {
    labelKey: "settings.envBadge.macos",
    className:
      "bg-gray-500/10 text-gray-600 dark:text-gray-400 border-gray-500/20",
  },
  linux: {
    labelKey: "settings.envBadge.linux",
    className:
      "bg-green-500/10 text-green-600 dark:text-green-400 border-green-500/20",
  },
};

const posixScriptInstallCommand = (url: string) =>
  `bash -c 'tmp=$(mktemp) && curl -fsSL ${url} -o $tmp && bash $tmp; status=$?; rm -f $tmp; exit $status'`;

const HERMES_WINDOWS_INSTALL_SCRIPT =
  "irm https://raw.githubusercontent.com/NousResearch/hermes-agent/main/scripts/install.ps1 | iex";

const powershellEncodedCommand = (script: string): string => {
  let binary = "";
  for (let i = 0; i < script.length; i += 1) {
    const code = script.charCodeAt(i);
    binary += String.fromCharCode(code & 0xff, code >> 8);
  }
  return btoa(binary);
};

const HERMES_WINDOWS_INSTALL_COMMAND = `powershell -NoProfile -ExecutionPolicy Bypass -EncodedCommand ${powershellEncodedCommand(
  HERMES_WINDOWS_INSTALL_SCRIPT,
)}`;

const MCODE_WINDOWS_INSTALL_COMMAND = `powershell -NoProfile -ExecutionPolicy Bypass -EncodedCommand ${powershellEncodedCommand(
  "irm https://filecdn.minimax.chat/public/install.ps1 | iex",
)}`;

// 与后端 npm_install_command_for("claude") 保持一致：原生依赖和 postinstall 缺一不可，
// 否则 bin/claude.exe 会保留文本占位文件，在 Windows 上报兼容性错误。
const CLAUDE_NPM_INSTALL_COMMAND =
  "npm i -g @anthropic-ai/claude-code@latest --ignore-scripts=false --include=optional --allow-scripts=@anthropic-ai/claude-code";

// 与后端 npm_install_command_for("mcode") 保持一致：npm 12 默认拦截依赖的 install
// 脚本，不放行 better-sqlite3 时 SQLite 不可用。
const MCODE_NPM_INSTALL_COMMAND =
  'npm i -g @minimax-ai/code@latest --ignore-scripts=false --include=optional "--allow-scripts=@minimax-ai/code,better-sqlite3"';

const POSIX_ONE_CLICK_INSTALL_COMMANDS = `# Claude Code
${posixScriptInstallCommand("https://claude.ai/install.sh")} || ${CLAUDE_NPM_INSTALL_COMMAND}
# Codex
npm i -g @openai/codex@latest
# Gemini CLI
npm i -g @google/gemini-cli@latest
# Grok Build
npm i -g @xai-official/grok@latest
# OpenCode
${posixScriptInstallCommand("https://opencode.ai/install")} || npm i -g opencode-ai@latest
# OpenClaw
npm i -g openclaw@latest
# Hermes
${posixScriptInstallCommand("https://raw.githubusercontent.com/NousResearch/hermes-agent/main/scripts/install.sh")}
# Pi
npm i -g @earendil-works/pi-coding-agent@latest
# MiniMax Code
${posixScriptInstallCommand("https://filecdn.minimax.chat/public/install.sh")} || ${MCODE_NPM_INSTALL_COMMAND}`;

const WINDOWS_ONE_CLICK_INSTALL_COMMANDS = `# Claude Code
${CLAUDE_NPM_INSTALL_COMMAND}
# Codex
npm i -g @openai/codex@latest
# Gemini CLI
npm i -g @google/gemini-cli@latest
# Grok Build
npm i -g @xai-official/grok@latest
# OpenCode
npm i -g opencode-ai@latest
# OpenClaw
npm i -g openclaw@latest
# Hermes
${HERMES_WINDOWS_INSTALL_COMMAND}
# Pi
npm i -g @earendil-works/pi-coding-agent@latest
# MiniMax Code
${MCODE_WINDOWS_INSTALL_COMMAND}`;

export const ONE_CLICK_INSTALL_COMMANDS = isWindows()
  ? WINDOWS_ONE_CLICK_INSTALL_COMMANDS
  : POSIX_ONE_CLICK_INSTALL_COMMANDS;

export const TOOL_DISPLAY_NAMES: Record<ToolName, string> = {
  claude: "Claude Code",
  codex: "Codex",
  gemini: "Gemini CLI",
  grok: "Grok Build",
  opencode: "OpenCode",
  openclaw: "OpenClaw",
  hermes: "Hermes",
  pi: "Pi",
  mcode: "MiniMax Code",
};

// 后端返回的 tool 是 string；这里收敛唯一的 ToolName 断言与兜底，供升级确认
// 对话框按工具名展示（避免在 JSX 里内联 cast、且每次渲染都新建闭包）。
export function toolDisplayName(tool: string): string {
  return TOOL_DISPLAY_NAMES[tool as ToolName] ?? tool;
}

export const TOOL_APP_IDS: Record<ToolName, AppId> = {
  claude: "claude",
  codex: "codex",
  gemini: "gemini",
  grok: "grokbuild",
  opencode: "opencode",
  openclaw: "openclaw",
  hermes: "hermes",
  pi: "pi",
  mcode: "mcode",
};

// 工具版本探测代价高：每个工具一次 `--version` 子进程 + 一次 npm/github/pypi 网络请求。
// 「应用」页离开就卸载，每次回来都重挂，若都全量重查纯属浪费。用「模块级」缓存（生命周期 = JS 模块 = 应用会话，不随组件卸载销毁）
// 跨重挂存活：重挂时若缓存仍新鲜（距上次全量加载 < TTL）直接复用、跳过探测；超期或用户
// 手动「刷新」才强制重查。at = 最近一次「全量加载」完成时刻；单工具刷新（切 shell / 升级
// 后）只更新数据、不重置 at，避免一次局部刷新把整体 TTL 续命。
const TOOL_VERSIONS_CACHE_TTL_MS = 10 * 60 * 1000; // 10 分钟
const EMPTY_TOOL_VERSIONS: ToolVersion[] = [];
let toolVersionRequestSequence = 0;
const latestToolVersionRequests = new Map<string, number>();

interface ToolManagementState {
  toolVersionsCache: { data: ToolVersion[]; at: number } | null;
  busyTools: ReadonlyMap<ToolName, ToolLifecycleAction>;
  pendingUpgrades: readonly PendingUpgrade[];
  batchAction: ToolLifecycleAction | null;
  /** 每个工具的安装分布（路径、来源、多处安装），页面打开时探测一次 */
  installReports: Partial<Record<ToolName, ToolInstallationReport>> | null;
}

// 安装/升级不会随设置页卸载而结束。将任务、确认队列与版本结果一起保留在会话中，
// 让新挂载的页面立即恢复进度，并收到原任务的完成结果。
let toolManagementState: ToolManagementState = {
  toolVersionsCache: null,
  busyTools: new Map(),
  pendingUpgrades: [],
  batchAction: null,
  installReports: null,
};
const toolManagementListeners = new Set<() => void>();
const getToolManagementState = () => toolManagementState;
function subscribeToolManagement(listener: () => void) {
  toolManagementListeners.add(listener);
  return () => {
    toolManagementListeners.delete(listener);
  };
}
function updateToolManagementState(update: Partial<ToolManagementState>) {
  toolManagementState = { ...toolManagementState, ...update };
  toolManagementListeners.forEach((listener) => listener());
}

// 把探测结果按 name 合并进已有列表：替换同名项、追加新项；空列表时直接采用新结果。
// 组件 state 与模块缓存共用同一套合并语义（单工具与全量探测都经此函数）。
function mergeToolVersions(
  prev: ToolVersion[],
  updated: ToolVersion[],
): ToolVersion[] {
  if (prev.length === 0) return updated;
  const byName = new Map(updated.map((t) => [t.name, t]));
  const merged = prev.map((t) => byName.get(t.name) ?? t);
  const existing = new Set(prev.map((t) => t.name));
  for (const u of updated) {
    if (!existing.has(u.name)) merged.push(u);
  }
  return merged;
}

/**
 * 设置里打开了「启动时检查应用更新」时，启动后在后台查一次（结果进同一个模块缓存）。
 * 不弹任何提示，只让侧栏「应用」上出现圆点。
 */
export async function checkToolUpdatesInBackground(): Promise<void> {
  const cache = toolManagementState.toolVersionsCache;
  if (cache && Date.now() - cache.at < TOOL_VERSIONS_CACHE_TTL_MS) return;
  const requestId = ++toolVersionRequestSequence;
  TOOL_NAMES.forEach((name) => latestToolVersionRequests.set(name, requestId));
  try {
    const results = await settingsApi.getToolVersions([...TOOL_NAMES]);
    const current = results.filter(
      (tool) => latestToolVersionRequests.get(tool.name) === requestId,
    );
    if (current.length === 0) return;
    const latest = toolManagementState.toolVersionsCache;
    updateToolManagementState({
      toolVersionsCache: {
        data: mergeToolVersions(latest?.data ?? [], current),
        at: Date.now(),
      },
    });
  } catch (error) {
    console.error("[useToolManagement] Background update check failed", error);
  }
}

/** 已检查过的命令行应用里有没有可升级的（不触发检查，只读缓存）。 */
export function useToolUpdatesAvailable(): boolean {
  const { toolVersionsCache } = useSyncExternalStore(
    subscribeToolManagement,
    getToolManagementState,
  );
  return (
    toolVersionsCache?.data.some((tool) =>
      isUpdateAvailable(tool.version, tool.latest_version),
    ) ?? false
  );
}

export function useToolManagement() {
  const { t } = useTranslation();
  const {
    toolVersionsCache,
    busyTools,
    pendingUpgrades,
    batchAction,
    installReports,
  } = useSyncExternalStore(subscribeToolManagement, getToolManagementState);
  const toolVersions = toolVersionsCache?.data ?? EMPTY_TOOL_VERSIONS;
  const pendingUpgrade = pendingUpgrades[0] ?? null;
  // 有缓存（哪怕已超期）就先展示旧值、初始不 loading；超期时由挂载副作用触发后台
  // 重查（stale-while-revalidate）。无缓存（首次）才从 loading 起步。
  const [isLoadingTools, setIsLoadingTools] = useState(
    () => toolVersionsCache === null,
  );

  const [wslShellByTool, setWslShellByTool] = useState<
    Record<string, WslShellPreference>
  >({});
  const [loadingTools, setLoadingTools] = useState<Record<string, boolean>>({});
  // 多处安装冲突诊断结果：按工具存储，有冲突的工具会在其卡片下方展示。
  // 来源两路：顶部「诊断安装冲突」按钮一次性扫全部，或升级后版本未变时自动补诊。
  const [toolDiagnostics, setToolDiagnostics] = useState<
    Partial<Record<ToolName, ToolInstallation[]>>
  >({});
  const [isDiagnosingAll, setIsDiagnosingAll] = useState(false);
  // 每个工具独立锁定，覆盖预检、等待确认、执行与版本刷新。
  // 共享状态同步加锁，避免重复点击或重挂后的页面重复提交同一工具。
  const releaseTools = useCallback((toolNames: ToolName[]) => {
    if (toolNames.length === 0) return;
    const next = new Map(toolManagementState.busyTools);
    toolNames.forEach((name) => next.delete(name));
    updateToolManagementState({ busyTools: next });
  }, []);

  const toolVersionByName = useMemo(() => {
    return new Map(toolVersions.map((tool) => [tool.name, tool]));
  }, [toolVersions]);

  const updatableToolNames = useMemo(
    () =>
      TOOL_NAMES.filter((toolName) => {
        const tool = toolVersionByName.get(toolName);
        return (
          !busyTools.has(toolName) &&
          !loadingTools[toolName] &&
          isUpdateAvailable(tool?.version, tool?.latest_version)
        );
      }),
    [toolVersionByName, busyTools, loadingTools],
  );

  const refreshToolVersions = useCallback(
    async (
      toolNames: ToolName[],
      wslOverrides?: Record<string, WslShellPreference>,
    ): Promise<ToolVersion[]> => {
      if (toolNames.length === 0) return [];

      // 请求顺序跨组件挂载保留，旧页面的迟到响应不能覆盖后续刷新或升级结果。
      const requestId = ++toolVersionRequestSequence;
      toolNames.forEach((name) =>
        latestToolVersionRequests.set(name, requestId),
      );

      // 单工具刷新使用统一后端入口（get_tool_versions）并带工具过滤。
      setLoadingTools((prev) => {
        const next = { ...prev };
        for (const name of toolNames) next[name] = true;
        return next;
      });

      try {
        const updated = await settingsApi.getToolVersions(
          toolNames,
          wslOverrides,
        );
        const current = updated.filter(
          (tool) => latestToolVersionRequests.get(tool.name) === requestId,
        );
        if (current.length === 0) return [];

        // 同步进模块缓存，供切 Tab 重挂时复用。时间戳沿用上次「全量加载」的（单工具
        // 刷新不算全量、不重置 TTL）；缓存为空时以 at=0 起步——0 是「尚未完成全量加载」
        // 的过期哨兵，确保探测中途切走/切回时，残缺缓存被判过期而触发重查，而非把半套
        // 数据当成完整结果复用。真实时间戳只由 loadAllToolVersions 的 finally 盖上。
        const cache = toolManagementState.toolVersionsCache;
        updateToolManagementState({
          toolVersionsCache: {
            data: mergeToolVersions(cache?.data ?? [], current),
            at: cache?.at ?? 0,
          },
        });

        // 返回刷新结果，调用方可据此判断版本是否真的探到（避免读 state 撞 stale closure）。
        return current;
      } catch (error) {
        console.error("[useToolManagement] Failed to refresh tools", error);
        return [];
      } finally {
        setLoadingTools((prev) => {
          const next = { ...prev };
          for (const name of toolNames) {
            if (latestToolVersionRequests.get(name) === requestId) {
              next[name] = false;
            }
          }
          return next;
        });
      }
    },
    [],
  );

  const loadAllToolVersions = useCallback(
    async (options?: { force?: boolean }) => {
      const force = options?.force ?? false;
      const cache = toolManagementState.toolVersionsCache;
      // 命中新鲜缓存：切回「关于」Tab 触发的重挂直接复用上次结果，跳过 6 个 `--version`
      // 子进程 + 6 个 latest 版本网络请求。手动「刷新」传 force 绕过缓存强制重查。
      if (
        !force &&
        cache &&
        Date.now() - cache.at < TOOL_VERSIONS_CACHE_TTL_MS
      ) {
        setIsLoadingTools(false);
        return;
      }
      setIsLoadingTools(true);
      try {
        // 逐工具并发探测：每个工具一完成就合并进 toolVersions（并写模块缓存）、清掉自己
        // 的 loadingTools 标志，对应卡片随即独立刷新——而非等全部探测完才一次性显示（后端
        // 原本对 6 个工具串行 await，总耗时累加；并发后压成「最慢的那一个」）。refreshTool-
        // Versions 已内建按 name 合并 + per-tool loading + try/catch 兜底（单工具失败返回 []
        // 不拖累其余），故 Promise.all 永不 reject。Respect current shell/flag overrides.
        // 切页后即使缓存已过期，也跳过忙碌工具，由原任务在执行结束后刷新版本。
        await Promise.all(
          TOOL_NAMES.filter(
            (toolName) => !toolManagementState.busyTools.has(toolName),
          ).map((toolName) => refreshToolVersions([toolName], wslShellByTool)),
        );
      } finally {
        // 全量探测结束：把缓存时间戳刷新为现在，标记「刚完成一次全量加载」、重置 TTL。
        const latestCache = toolManagementState.toolVersionsCache;
        if (latestCache) {
          updateToolManagementState({
            toolVersionsCache: { ...latestCache, at: Date.now() },
          });
        }
        setIsLoadingTools(false);
      }
    },
    [wslShellByTool, refreshToolVersions],
  );

  const handleToolShellChange = async (toolName: ToolName, value: string) => {
    const wslShell = value === "auto" ? null : value;
    const nextPref: WslShellPreference = {
      ...(wslShellByTool[toolName] ?? {}),
      wslShell,
    };
    setWslShellByTool((prev) => ({ ...prev, [toolName]: nextPref }));
    await refreshToolVersions([toolName], { [toolName]: nextPref });
  };

  const handleToolShellFlagChange = async (
    toolName: ToolName,
    value: string,
  ) => {
    const wslShellFlag = value === "auto" ? null : value;
    const nextPref: WslShellPreference = {
      ...(wslShellByTool[toolName] ?? {}),
      wslShellFlag,
    };
    setWslShellByTool((prev) => ({ ...prev, [toolName]: nextPref }));
    await refreshToolVersions([toolName], { [toolName]: nextPref });
  };

  // 安装分布：路径、来源（npm / 原生 / Homebrew…）、有没有多处安装。只在本机探测，不联网。
  const loadInstallations = useCallback(
    async (options?: { force?: boolean }) => {
      if (!options?.force && toolManagementState.installReports) return;
      try {
        const reports = await settingsApi.listToolInstallations([
          ...TOOL_NAMES,
        ]);
        const next: Partial<Record<ToolName, ToolInstallationReport>> = {};
        for (const report of reports) {
          next[report.tool as ToolName] = report;
        }
        updateToolManagementState({ installReports: next });
      } catch (error) {
        console.error(
          "[useToolManagement] probeToolInstallations failed",
          error,
        );
      }
    },
    [],
  );

  useEffect(() => {
    void loadAllToolVersions();
    void loadInstallations();
    // Mount-only: loadAllToolVersions is intentionally excluded to avoid
    // re-fetching all tools whenever wslShellByTool changes. Single-tool
    // refreshes are handled by refreshToolVersions in the shell/flag handlers.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const checkForUpdates = useCallback(async () => {
    await Promise.all([
      loadAllToolVersions({ force: true }),
      loadInstallations({ force: true }),
    ]);
  }, [loadAllToolVersions, loadInstallations]);

  const handleCopyInstallCommands = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(ONE_CLICK_INSTALL_COMMANDS);
      toast.success(t("settings.installCommandsCopied"), { closeButton: true });
    } catch (error) {
      console.error(
        "[useToolManagement] Failed to copy install commands",
        error,
      );
      toast.error(t("settings.installCommandsCopyFailed"));
    }
  }, [t]);

  // 升级后自动补诊单个工具：静默后台执行。有冲突写入结果；无冲突则清掉该工具可能残留
  // 的过期冲突展示（外部卸载/修复后冲突可能已消失，不清会一直显示旧列表）。不弹 toast、
  // 不报错打扰——与用户主动点的全量诊断区别对待。
  const diagnoseToolSilently = useCallback(async (toolName: ToolName) => {
    try {
      const [report] = await settingsApi.probeToolInstallations([toolName]);
      // 同一份报告也是行上的路径 / 来源 / 「另有 N 处安装」：装完、升级完就刷新，
      // 不用等手动「检查更新」
      if (report) {
        updateToolManagementState({
          installReports: {
            ...(toolManagementState.installReports ?? {}),
            [toolName]: report,
          },
        });
      }
      setToolDiagnostics((prev) => {
        if (report?.is_conflict) {
          return { ...prev, [toolName]: report.installs };
        }
        // 无冲突：清掉残留;无旧结果则返回同引用，避免无谓 re-render。
        if (!(toolName in prev)) return prev;
        const next = { ...prev };
        delete next[toolName];
        return next;
      });
    } catch (error) {
      console.error(
        `[useToolManagement] Auto-diagnose failed for ${toolName}`,
        error,
      );
    }
  }, []);

  // 顶部按钮：一次性诊断全部 6 个工具，有冲突的写入各自卡片，
  // 全部无冲突时给一条 info toast。后端逐工具枚举所有安装并判定分歧。
  const handleDiagnoseAll = useCallback(async () => {
    setIsDiagnosingAll(true);
    try {
      const reports = await settingsApi.probeToolInstallations([...TOOL_NAMES]);
      const next: Partial<Record<ToolName, ToolInstallation[]>> = {};
      const installReports: Partial<Record<ToolName, ToolInstallationReport>> =
        {};
      let conflicts = 0;
      for (const report of reports) {
        installReports[report.tool as ToolName] = report;
        if (report.is_conflict) {
          next[report.tool as ToolName] = report.installs;
          conflicts += 1;
        }
      }
      updateToolManagementState({ installReports });
      setToolDiagnostics(next);
      if (conflicts === 0) {
        toast.info(t("settings.toolDiagnoseNoConflict"), { closeButton: true });
      } else {
        // 冲突列表在对应行里展开（AppsPage 看 toolDiagnostics 自动展开），这里只说一句
        toast.warning(
          t("settings.toolDiagnoseConflicts", { count: conflicts }),
          {
            closeButton: true,
          },
        );
      }
    } catch (error) {
      console.error("[useToolManagement] Diagnose all failed", error);
      toast.error(t("settings.toolDiagnoseFailed"), {
        description: extractErrorMessage(error) || undefined,
        closeButton: true,
      });
    } finally {
      setIsDiagnosingAll(false);
    }
  }, [t]);

  // 已通过必要的确认后，各工具独立提交、刷新和解锁；安装写入由后端串行调度。
  const executeRun = useCallback(
    async (
      toolNames: ToolName[],
      action: ToolLifecycleAction,
      wslOverrides: Record<string, WslShellPreference>,
    ) => {
      const isBatch = toolNames.length > 1;

      // 每个工具独立调用后端，一个失败不会中断其它工具。
      // soft=true 表示"命令成功执行但结果仍需用户介入"（版本没变/装上却跑不起来），
      // 与命令本身报错（soft=false）区别对待：前者不算硬失败，toast 降级为 warning。
      const failures: {
        toolName: ToolName;
        detail: string;
        soft: boolean;
        kind?: "notRunnable" | "versionUnchanged";
      }[] = [];
      let succeeded = 0;

      await Promise.all(
        toolNames.map(async (toolName) => {
          try {
            const previousTool = toolVersionByName.get(toolName);
            const previousVersion = previousTool?.version ?? null;
            const previousLatestVersion = previousTool?.latest_version ?? null;

            await settingsApi.runToolLifecycleAction(
              [toolName],
              action,
              wslOverrides,
            );
            // 静默执行真正结束后刷新该工具版本，卡片立即反映结果。
            const refreshed = await refreshToolVersions(
              [toolName],
              wslOverrides,
            );
            const tool = refreshed.find((t) => t.name === toolName);
            if (tool?.version) {
              const latestVersion =
                tool.latest_version ?? previousLatestVersion;
              const versionUnchangedAfterUpdate =
                action === "update" &&
                Boolean(previousVersion) &&
                tool.version === previousVersion &&
                isUpdateAvailable(tool.version, latestVersion);

              if (versionUnchangedAfterUpdate) {
                // 有些上游 updater 会在未实际改动版本时仍返回 0。这里用刷新后的
                // 当前版本 + latest_version 再确认一次，避免给用户误报升级成功。
                failures.push({
                  toolName,
                  detail: t("settings.toolActionVersionUnchanged", {
                    version: tool.version,
                    latest: latestVersion ?? t("common.unknown"),
                  }),
                  soft: true,
                  kind: "versionUnchanged",
                });
                void diagnoseToolSilently(toolName);
              } else {
                succeeded += 1;
                // 升级成功后无条件补诊：版本没变多半被另一处遮蔽，版本变了另一处也可能仍在，
                // 两种都要刷新冲突展示（diagnoseToolSilently 无冲突时会自动清旧）。
                if (action === "update") {
                  void diagnoseToolSilently(toolName);
                }
              }
            } else {
              // 命令退出码为 0、但刷新后仍探不到版本：多半是"装上了却跑不起来"
              // （如 openclaw 要求更高的 Node 版本）。refreshToolVersions 的 merge 已把
              // version 置空并写入后端 error，这里只需归类为软失败并展示原因。
              const detail =
                tool?.error?.trim() || t("settings.toolNotRunnable");
              failures.push({
                toolName,
                detail,
                soft: true,
                kind: "notRunnable",
              });
              // 装了却跑不起来同样可能源于多处安装，自动诊断帮用户定位。
              void diagnoseToolSilently(toolName);
            }
          } catch (error) {
            const detail = extractErrorMessage(error) || String(error);
            if (detail === "TOOL_ACTION_IN_PROGRESS") {
              toast.info(t("settings.toolActionInProgress"), {
                description: t("settings.toolActionInProgressDetail", {
                  tool: TOOL_DISPLAY_NAMES[toolName],
                }),
                closeButton: true,
              });
              return;
            }
            console.error(
              `[useToolManagement] Failed to run tool action for ${toolName}`,
              error,
            );
            failures.push({ toolName, detail, soft: false });
          } finally {
            releaseTools([toolName]);
          }
        }),
      );

      const actionLabel =
        action === "install"
          ? t("settings.toolInstall")
          : t("settings.toolUpdate");

      if (failures.length === 0) {
        if (succeeded > 0) {
          toast.success(
            t("settings.toolActionDone", {
              count: succeeded,
              action: actionLabel,
            }),
            { closeButton: true },
          );
        }
        return;
      }

      // 批量场景每个失败只摘取错误末行（最相关），单工具场景给出完整详情。
      const lastLine = (text: string) => {
        const lines = text.trim().split("\n").filter(Boolean);
        return lines[lines.length - 1] ?? text;
      };
      const failureDescription = isBatch
        ? failures
            .map(
              (f) => `${TOOL_DISPLAY_NAMES[f.toolName]}: ${lastLine(f.detail)}`,
            )
            .join("\n")
        : failures[0]?.detail;

      const hardFailures = failures.filter((f) => !f.soft);
      const allSoftVersionUnchanged =
        failures.length > 0 &&
        failures.every((f) => f.soft && f.kind === "versionUnchanged");

      if (succeeded === 0 && hardFailures.length === 0) {
        // 命令均成功执行、但结果需要用户介入（版本没变 / 装上却跑不起来）
        // → 降级为 warning 并解释原因。
        toast.warning(
          allSoftVersionUnchanged
            ? t("settings.toolActionVersionUnchangedTitle")
            : t("settings.toolActionInstalledNotRunnable"),
          {
            description: failureDescription || undefined,
            closeButton: true,
          },
        );
      } else if (succeeded === 0) {
        toast.error(t("settings.toolActionFailed"), {
          description: failureDescription || undefined,
          closeButton: true,
        });
      } else {
        // 部分成功：用 warning 汇总成败数量，详情列出失败的工具。
        toast.warning(
          t("settings.toolActionPartial", {
            succeeded,
            failed: failures.length,
            action: actionLabel,
          }),
          { description: failureDescription || undefined, closeButton: true },
        );
      }
    },
    [
      t,
      toolVersionByName,
      refreshToolVersions,
      diagnoseToolSilently,
      releaseTools,
    ],
  );

  // 单独升级与全部升级共用按工具加锁的入口，自动跳过已在处理的工具。
  const handleRunToolAction = useCallback(
    async (
      requestedTools: ToolName[],
      action: ToolLifecycleAction,
      options?: { fromBatchEntry?: boolean },
    ) => {
      const toolNames = requestedTools.filter(
        (name) => !toolManagementState.busyTools.has(name),
      );
      if (toolNames.length === 0) return;
      // 确认可能发生在重新挂载的页面，执行与结果刷新均使用提交时的参数快照。
      const wslOverrides = Object.fromEntries(
        Object.entries(wslShellByTool).map(([name, pref]) => [
          name,
          { ...pref },
        ]),
      );
      updateToolManagementState({
        busyTools: new Map([
          ...toolManagementState.busyTools,
          ...toolNames.map((name) => [name, action] as const),
        ]),
      });
      // 锁移交给 executeRun 或确认队列后，由其负责释放；入口只释放未移交的工具。
      let toolsToRelease = toolNames;
      // 全部升级即使只剩一个工具，也在预检和执行期间显示进度。
      const fromBatchEntry = options?.fromBatchEntry ?? false;
      if (fromBatchEntry) {
        updateToolManagementState({ batchAction: action });
      }
      try {
        if (action === "install") {
          toolsToRelease = [];
          await executeRun(toolNames, action, wslOverrides);
          return;
        }
        let reports: ToolInstallationReport[];
        try {
          reports = await settingsApi.probeToolInstallations(toolNames);
        } catch (error) {
          // 探测失败不应阻断升级：退回直接执行（等同旧行为）。
          console.error(
            "[useToolManagement] probeToolInstallations failed",
            error,
          );
          toolsToRelease = [];
          await executeRun(toolNames, action, wslOverrides);
          return;
        }
        // 认不出安装渠道的原生安装（winget / Scoop / 手动下载的二进制等）不执行升级：
        // 退回 npm 只会另装一份 npm 版（#7650）。跳过并提示用原安装方式升级，其余照常。
        const unmanaged = reports.filter((r) => r.unmanaged);
        if (unmanaged.length > 0) {
          toast.warning(t("settings.toolUpgradeUnmanagedTitle"), {
            description: unmanaged
              .map((r) =>
                t("settings.toolUpgradeUnmanagedDetail", {
                  tool: toolDisplayName(r.tool),
                  path:
                    (r.installs.find((i) => i.is_path_default) ?? r.installs[0])
                      ?.path ?? "",
                }),
              )
              .join("\n"),
            closeButton: true,
          });
        }
        const runnableTools = toolNames.filter(
          (name) => !unmanaged.some((r) => r.tool === name),
        );
        if (runnableTools.length === 0) return;
        toolsToRelease = toolNames.filter(
          (name) => !runnableTools.includes(name),
        );
        const needConfirm = reports.filter(
          (r) => r.needs_confirmation && !r.unmanaged,
        );
        if (needConfirm.length === 0) {
          await executeRun(runnableTools, action, wslOverrides);
          return;
        }
        // 并发探测的确认按到达顺序排队，切页后也能继续确认或取消。
        updateToolManagementState({
          pendingUpgrades: [
            ...toolManagementState.pendingUpgrades,
            {
              toolNames: runnableTools,
              plans: needConfirm,
              fromBatchEntry,
              wslShellByTool: wslOverrides,
            },
          ],
        });
      } finally {
        if (fromBatchEntry) {
          updateToolManagementState({ batchAction: null });
        }
        releaseTools(toolsToRelease);
      }
    },
    [executeRun, releaseTools, t, wslShellByTool],
  );

  const handleConfirmUpgrade = useCallback(() => {
    if (
      !pendingUpgrade ||
      toolManagementState.pendingUpgrades[0] !== pendingUpgrade
    )
      return;
    updateToolManagementState({
      pendingUpgrades: toolManagementState.pendingUpgrades.slice(1),
    });
    const {
      toolNames,
      fromBatchEntry,
      wslShellByTool: wslOverrides,
    } = pendingUpgrade;
    if (fromBatchEntry) {
      updateToolManagementState({ batchAction: "update" });
    }
    void executeRun(toolNames, "update", wslOverrides).finally(() => {
      if (fromBatchEntry) {
        updateToolManagementState({ batchAction: null });
      }
    });
  }, [pendingUpgrade, executeRun]);

  const handleCancelUpgrade = useCallback(() => {
    if (
      !pendingUpgrade ||
      toolManagementState.pendingUpgrades[0] !== pendingUpgrade
    )
      return;
    updateToolManagementState({
      pendingUpgrades: toolManagementState.pendingUpgrades.slice(1),
    });
    releaseTools(pendingUpgrade.toolNames);
  }, [pendingUpgrade, releaseTools]);

  // 全量刷新和诊断仍等待升级结束；各工具的操作只受自身忙碌状态影响。
  const isAnyBusy = busyTools.size > 0;
  // 最近一次全量检查完成的时刻；0 表示还没完整查过一次
  const lastCheckedAt =
    toolVersionsCache && toolVersionsCache.at > 0 ? toolVersionsCache.at : null;

  return {
    toolVersionByName,
    installReports,
    isLoadingTools,
    loadingTools,
    busyTools,
    batchAction,
    pendingUpgrade,
    updatableToolNames,
    toolDiagnostics,
    isDiagnosingAll,
    isAnyBusy,
    lastCheckedAt,
    wslShellByTool,
    checkForUpdates,
    handleToolShellChange,
    handleToolShellFlagChange,
    handleDiagnoseAll,
    handleRunToolAction,
    handleConfirmUpgrade,
    handleCancelUpgrade,
    handleCopyInstallCommands,
  };
}
