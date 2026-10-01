import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
} from "react";
import {
  Download,
  Copy,
  ExternalLink,
  Github,
  Globe,
  Info,
  Loader2,
  RefreshCw,
  Terminal,
  CheckCircle2,
  AlertCircle,
  ArrowUpCircle,
  ChevronDown,
  Stethoscope,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { getVersion } from "@tauri-apps/api/app";
import { settingsApi } from "@/lib/api";
import type {
  ToolInstallation,
  ToolInstallationReport,
} from "@/lib/api/settings";
import { useUpdate } from "@/contexts/UpdateContext";
import { Badge } from "@/components/ui/badge";
import { motion } from "framer-motion";
import appIcon from "@/assets/icons/app-icon.png";
import { APP_ICON_MAP } from "@/config/appConfig";
import type { AppId } from "@/lib/api/types";
import { extractErrorMessage } from "@/utils/errorUtils";
import { isWindows } from "@/lib/platform";
import { isUpdateAvailable } from "@/lib/version";
import { ToolUpgradeConfirmDialog } from "./ToolUpgradeConfirmDialog";
import { ToolInstallRow } from "./ToolInstallRow";
import { ToolErrorMessage } from "./ToolErrorMessage";

interface AboutSectionProps {
  isPortable: boolean;
}

interface ToolVersion {
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

const TOOL_NAMES = [
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
type ToolName = (typeof TOOL_NAMES)[number];
type ToolLifecycleAction = "install" | "update";

interface PendingUpgrade {
  toolNames: ToolName[];
  plans: ToolInstallationReport[];
  fromBatchEntry: boolean;
  wslShellByTool: Record<string, WslShellPreference>;
}

type WslShellPreference = {
  wslShell?: string | null;
  wslShellFlag?: string | null;
};

const WSL_SHELL_OPTIONS = ["sh", "bash", "zsh", "fish", "dash"] as const;
// UI-friendly order: login shell first.
const WSL_SHELL_FLAG_OPTIONS = ["-lic", "-lc", "-c"] as const;

const ENV_BADGE_CONFIG: Record<
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

// 与后端 npm_install_command_for("mcode") 保持一致：npm 12 默认拦截依赖的 install
// 脚本，不放行 better-sqlite3 时 SQLite 不可用。
const MCODE_NPM_INSTALL_COMMAND =
  'npm i -g @minimax-ai/code@latest --ignore-scripts=false --include=optional "--allow-scripts=@minimax-ai/code,better-sqlite3"';

const POSIX_ONE_CLICK_INSTALL_COMMANDS = `# Claude Code
${posixScriptInstallCommand("https://claude.ai/install.sh")} || npm i -g @anthropic-ai/claude-code@latest
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
npm i -g @anthropic-ai/claude-code@latest
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

const ONE_CLICK_INSTALL_COMMANDS = isWindows()
  ? WINDOWS_ONE_CLICK_INSTALL_COMMANDS
  : POSIX_ONE_CLICK_INSTALL_COMMANDS;

const TOOL_DISPLAY_NAMES: Record<ToolName, string> = {
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
function toolDisplayName(tool: string): string {
  return TOOL_DISPLAY_NAMES[tool as ToolName] ?? tool;
}

const TOOL_APP_IDS: Record<ToolName, AppId> = {
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
// 设置页用 Radix Tabs，非激活 Tab 会被卸载——每次切回「关于」都重挂 AboutSection，若都
// 全量重查纯属浪费。用「模块级」缓存（生命周期 = JS 模块 = 应用会话，不随组件卸载销毁）
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
}

// 安装/升级不会随设置页卸载而结束。将任务、确认队列与版本结果一起保留在会话中，
// 让新挂载的页面立即恢复进度，并收到原任务的完成结果。
let toolManagementState: ToolManagementState = {
  toolVersionsCache: null,
  busyTools: new Map(),
  pendingUpgrades: [],
  batchAction: null,
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

// 应用自身版本（getVersion，本地毫秒级、无网络）也缓存一份，纯为重挂时免去 loading 闪烁。
let appVersionCache: string | null = null;

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

export function AboutSection({ isPortable }: AboutSectionProps) {
  // ... (use hooks as before) ...
  const { t } = useTranslation();
  // 惰性初始化自模块缓存：重挂时首帧即渲染上次的值，避免 loading 闪烁；首次挂载缓存
  // 为空则回退到原始初值（null / loading）。
  const [version, setVersion] = useState<string | null>(() => appVersionCache);
  const [isLoadingVersion, setIsLoadingVersion] = useState(
    () => appVersionCache === null,
  );
  const [isDownloading, setIsDownloading] = useState(false);
  const { toolVersionsCache, busyTools, pendingUpgrades, batchAction } =
    useSyncExternalStore(subscribeToolManagement, getToolManagementState);
  const toolVersions = toolVersionsCache?.data ?? EMPTY_TOOL_VERSIONS;
  const pendingUpgrade = pendingUpgrades[0] ?? null;
  // 有缓存（哪怕已超期）就先展示旧值、初始不 loading；超期时由挂载副作用触发后台
  // 重查（stale-while-revalidate）。无缓存（首次）才从 loading 起步。
  const [isLoadingTools, setIsLoadingTools] = useState(
    () => toolVersionsCache === null,
  );
  const [showInstallCommands, setShowInstallCommands] = useState(false);

  const { hasUpdate, updateInfo, checkUpdate, resetDismiss, isChecking } =
    useUpdate();

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
        console.error("[AboutSection] Failed to refresh tools", error);
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

  useEffect(() => {
    let active = true;

    // 本软件自身版本走本地调用（getVersion，无网络，毫秒级），与工具版本探测彼此独立。
    // 之前两者被塞进同一个 Promise.all，导致 setVersion / setIsLoadingVersion 被压在
    // 「全部工具检查完成」之后——图标下方的版本徽标因此要干等 6 个工具全检完才显示。
    // 拆成两条独立链路：应用版本一拿到就立刻显示，工具探测各自渐进刷新，互不阻塞。
    const loadAppVersion = async () => {
      try {
        const appVersion = await getVersion();
        appVersionCache = appVersion;
        if (active) {
          setVersion(appVersion);
        }
      } catch (error) {
        console.error("[AboutSection] Failed to load app version", error);
        if (active) {
          setVersion(null);
        }
      } finally {
        if (active) {
          setIsLoadingVersion(false);
        }
      }
    };

    void loadAppVersion();
    void loadAllToolVersions();
    return () => {
      active = false;
    };
    // Mount-only: loadAllToolVersions is intentionally excluded to avoid
    // re-fetching all tools whenever wslShellByTool changes. Single-tool
    // refreshes are handled by refreshToolVersions in the shell/flag handlers.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ... (handlers like handleOpenReleaseNotes, handleCheckUpdate) ...

  const handleOpenReleaseNotes = useCallback(async () => {
    try {
      const targetVersion = updateInfo?.availableVersion ?? version ?? "";
      const displayVersion = targetVersion.startsWith("v")
        ? targetVersion
        : targetVersion
          ? `v${targetVersion}`
          : "";

      if (!displayVersion) {
        await settingsApi.openExternal(
          "https://github.com/farion1231/cc-switch/releases",
        );
        return;
      }

      await settingsApi.openExternal(
        `https://github.com/farion1231/cc-switch/releases/tag/${displayVersion}`,
      );
    } catch (error) {
      console.error("[AboutSection] Failed to open release notes", error);
      toast.error(t("settings.openReleaseNotesFailed"));
    }
  }, [t, updateInfo?.availableVersion, version]);

  const handleOpenGithub = useCallback(() => {
    void settingsApi.openExternal("https://github.com/farion1231/cc-switch");
  }, []);

  const handleCheckUpdate = useCallback(async () => {
    if (hasUpdate) {
      if (isPortable) {
        try {
          await settingsApi.checkUpdates();
        } catch (error) {
          console.error("[AboutSection] Portable update failed", error);
        }
        return;
      }

      setIsDownloading(true);
      try {
        resetDismiss();
        const installed = await settingsApi.installUpdateAndRestart();
        if (!installed) {
          toast.success(t("settings.upToDate"), { closeButton: true });
        }
      } catch (error) {
        console.error("[AboutSection] Update failed", error);
        toast.error(t("settings.updateFailed"), {
          description: extractErrorMessage(error) || undefined,
          closeButton: true,
        });
        try {
          await settingsApi.checkUpdates();
        } catch (fallbackError) {
          console.error(
            "[AboutSection] Failed to open fallback updater",
            fallbackError,
          );
        }
      } finally {
        setIsDownloading(false);
      }
      return;
    }

    try {
      const available = await checkUpdate();
      if (!available) {
        toast.success(t("settings.upToDate"), { closeButton: true });
      }
    } catch (error) {
      console.error("[AboutSection] Check update failed", error);
      toast.error(t("settings.checkUpdateFailed"), {
        description: extractErrorMessage(error) || undefined,
        closeButton: true,
      });
    }
  }, [checkUpdate, hasUpdate, isPortable, resetDismiss, t]);

  const handleCopyInstallCommands = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(ONE_CLICK_INSTALL_COMMANDS);
      toast.success(t("settings.installCommandsCopied"), { closeButton: true });
    } catch (error) {
      console.error("[AboutSection] Failed to copy install commands", error);
      toast.error(t("settings.installCommandsCopyFailed"));
    }
  }, [t]);

  // 升级后自动补诊单个工具：静默后台执行。有冲突写入结果；无冲突则清掉该工具可能残留
  // 的过期冲突展示（外部卸载/修复后冲突可能已消失，不清会一直显示旧列表）。不弹 toast、
  // 不报错打扰——与用户主动点的全量诊断区别对待。
  const diagnoseToolSilently = useCallback(async (toolName: ToolName) => {
    try {
      const [report] = await settingsApi.probeToolInstallations([toolName]);
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
        `[AboutSection] Auto-diagnose failed for ${toolName}`,
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
      let conflicts = 0;
      for (const report of reports) {
        if (report.is_conflict) {
          next[report.tool as ToolName] = report.installs;
          conflicts += 1;
        }
      }
      setToolDiagnostics(next);
      if (conflicts === 0) {
        toast.info(t("settings.toolDiagnoseNoConflict"), { closeButton: true });
      }
    } catch (error) {
      console.error("[AboutSection] Diagnose all failed", error);
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
              `[AboutSection] Failed to run tool action for ${toolName}`,
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
          console.error("[AboutSection] probeToolInstallations failed", error);
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

  const displayVersion = version ?? t("common.unknown");

  // 全量刷新和诊断仍等待升级结束；各工具的操作只受自身忙碌状态影响。
  const isAnyBusy = busyTools.size > 0;

  return (
    <motion.section
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3 }}
      className="space-y-6"
    >
      <header className="space-y-1">
        <h3 className="text-sm font-medium">{t("common.about")}</h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.aboutHint")}
        </p>
      </header>

      <motion.div
        initial={{ opacity: 0, scale: 0.98 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={{ duration: 0.3, delay: 0.1 }}
        className="rounded-xl border border-border bg-gradient-to-br from-card/80 to-card/40 p-6 space-y-5 shadow-sm"
      >
        <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex items-center gap-8">
            <div className="flex flex-col items-center gap-2">
              <div className="flex items-center gap-2">
                <img src={appIcon} alt="CC Switch" className="h-5 w-5" />
                <h4 className="text-lg font-semibold text-foreground">
                  CC Switch
                </h4>
              </div>
              <div className="flex items-center gap-2">
                <Badge variant="outline" className="gap-1.5 bg-background/80">
                  <span className="text-muted-foreground">
                    {t("common.version")}
                  </span>
                  {isLoadingVersion ? (
                    <Loader2 className="h-3 w-3 animate-spin" />
                  ) : (
                    <span className="font-medium">{`v${displayVersion}`}</span>
                  )}
                </Badge>
                {isPortable && (
                  <Badge variant="secondary" className="gap-1.5">
                    <Info className="h-3 w-3" />
                    {t("settings.portableMode")}
                  </Badge>
                )}
              </div>
            </div>
          </div>

          <p className="min-w-0 flex-1 text-xs leading-relaxed sm:text-right">
            <a
              href="https://github.com/farion1231/cc-switch"
              onClick={(event) => {
                event.preventDefault();
                handleOpenGithub();
              }}
              className="font-medium text-primary hover:underline"
            >
              {t("settings.starPrompt")}
            </a>
            <span aria-hidden="true" className="ml-1.5">
              👉
            </span>
          </p>

          <div className="flex items-center gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={handleOpenGithub}
              className="h-8 gap-1.5 border-primary/30 bg-primary/10 text-xs text-primary hover:bg-primary/20 hover:text-primary"
            >
              <Github className="h-3.5 w-3.5" />
              {t("settings.github")}
              <span
                aria-hidden="true"
                className="inline-block animate-[spin_4s_linear_infinite] motion-reduce:animate-none"
              >
                ⭐
              </span>
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => settingsApi.openExternal("https://ccswitch.io")}
              className="h-8 gap-1.5 text-xs"
            >
              <Globe className="h-3.5 w-3.5" />
              {t("settings.officialWebsite")}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={handleOpenReleaseNotes}
              className="h-8 gap-1.5 text-xs"
            >
              <ExternalLink className="h-3.5 w-3.5" />
              {t("settings.releaseNotes")}
            </Button>
            <Button
              type="button"
              size="sm"
              onClick={handleCheckUpdate}
              disabled={isChecking || isDownloading}
              className="h-8 gap-1.5 text-xs"
            >
              {isDownloading ? (
                <>
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  {t("settings.updating")}
                </>
              ) : hasUpdate ? (
                <>
                  <Download className="h-3.5 w-3.5" />
                  {t("settings.updateTo", {
                    version: updateInfo?.availableVersion ?? "",
                  })}
                </>
              ) : isChecking ? (
                <>
                  <RefreshCw className="h-3.5 w-3.5 animate-spin" />
                  {t("settings.checking")}
                </>
              ) : (
                <>
                  <RefreshCw className="h-3.5 w-3.5" />
                  {t("settings.checkForUpdates")}
                </>
              )}
            </Button>
          </div>
        </div>

        {hasUpdate && updateInfo && (
          <motion.div
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: "auto" }}
            className="rounded-lg bg-primary/10 border border-primary/20 px-4 py-3 text-sm"
          >
            <p className="font-medium text-primary mb-1">
              {t("settings.updateAvailable", {
                version: updateInfo.availableVersion,
              })}
            </p>
            {updateInfo.notes && (
              <p className="text-muted-foreground line-clamp-3 leading-relaxed">
                {updateInfo.notes}
              </p>
            )}
          </motion.div>
        )}
      </motion.div>

      <div className="space-y-3">
        <div className="flex flex-col gap-2 px-1 sm:flex-row sm:items-center sm:justify-between">
          <h3 className="text-sm font-medium">{t("settings.localEnvCheck")}</h3>
          <div className="flex flex-wrap items-center gap-2">
            <Button
              size="sm"
              variant="outline"
              className="h-7 gap-1.5 text-xs"
              onClick={() => handleDiagnoseAll()}
              disabled={isLoadingTools || isAnyBusy || isDiagnosingAll}
            >
              {isDiagnosingAll ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Stethoscope className="h-3.5 w-3.5" />
              )}
              {isDiagnosingAll
                ? t("settings.toolDiagnosing")
                : t("settings.toolDiagnose")}
            </Button>
            <Button
              size="sm"
              variant="outline"
              className="h-7 gap-1.5 text-xs"
              onClick={() => loadAllToolVersions({ force: true })}
              disabled={isLoadingTools || isAnyBusy}
            >
              <RefreshCw
                className={
                  isLoadingTools ? "h-3.5 w-3.5 animate-spin" : "h-3.5 w-3.5"
                }
              />
              {isLoadingTools ? t("common.refreshing") : t("common.refresh")}
            </Button>
            <Button
              size="sm"
              className="h-7 gap-1.5 text-xs"
              onClick={() =>
                handleRunToolAction(updatableToolNames, "update", {
                  fromBatchEntry: true,
                })
              }
              disabled={
                isLoadingTools ||
                Boolean(batchAction) ||
                updatableToolNames.length === 0
              }
              aria-busy={Boolean(batchAction)}
            >
              {batchAction === "update" ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <ArrowUpCircle className="h-3.5 w-3.5" />
              )}
              {t("settings.updateAllTools", {
                count: updatableToolNames.length,
              })}
            </Button>
          </div>
        </div>

        <div className="grid gap-3 px-1 sm:grid-cols-2 xl:grid-cols-3">
          {TOOL_NAMES.map((toolName, index) => {
            const tool = toolVersionByName.get(toolName);
            const appConfig = APP_ICON_MAP[TOOL_APP_IDS[toolName]];
            const displayName = TOOL_DISPLAY_NAMES[toolName];
            // 单卡片 loading 用「结果是否已到」而非「整批是否结束」驱动，实现渐进式刷新：
            //   - loadingTools[t]：本工具探测在途（首次加载或单工具刷新）；
            //   - isLoadingTools && !has(t)：整批进行中且该工具尚未返回——覆盖首帧/刷新时
            //     未完成卡片的 loading 外观。某工具结果一落进 toolVersions，has(t) 即为 true，
            //     该卡片立刻脱离 loading（哪怕全局 isLoadingTools 还为 true），其它卡片不受影响。
            const isToolVersionLoading =
              Boolean(loadingTools[toolName]) ||
              (isLoadingTools && !toolVersionByName.has(toolName));
            const isOutdated = isUpdateAvailable(
              tool?.version,
              tool?.latest_version,
            );
            // 已安装却跑不起来（如 Node 版本不达标）：用它区分卡片文案与按钮，避免把
            // "装了跑不起来"误判成"未安装"而给出无用的安装按钮（重装同一版本解决不了）。
            const installedButBroken = Boolean(tool?.installed_but_broken);
            // loading 和 broken 都没有可执行动作；其余按是否已装/是否过期选择。
            // 已提交的动作持续显示到版本刷新结束，切页或版本变化都不应提前清掉进度。
            const isToolBusy = busyTools.has(toolName);
            const action: ToolLifecycleAction | null =
              busyTools.get(toolName) ??
              (isToolVersionLoading || installedButBroken
                ? null
                : !tool?.version
                  ? "install"
                  : isOutdated
                    ? "update"
                    : null);
            const conflicts = toolDiagnostics[toolName];

            return (
              <motion.div
                key={toolName}
                initial={{ opacity: 0, y: 10 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.3, delay: 0.15 + index * 0.04 }}
                className="flex min-h-[150px] flex-col gap-3 rounded-xl border border-border bg-gradient-to-br from-card/80 to-card/40 p-4 shadow-sm transition-colors hover:border-primary/30"
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="flex min-w-0 items-center gap-2">
                    <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-background/80 text-muted-foreground">
                      {appConfig?.icon ?? <Terminal className="h-4 w-4" />}
                    </span>
                    <div className="min-w-0">
                      <div className="truncate text-sm font-medium">
                        {displayName}
                      </div>
                      {tool?.env_type && ENV_BADGE_CONFIG[tool.env_type] && (
                        <span
                          className={`mt-1 inline-flex w-fit text-[9px] px-1.5 py-0.5 rounded-full border ${ENV_BADGE_CONFIG[tool.env_type].className}`}
                        >
                          {t(ENV_BADGE_CONFIG[tool.env_type].labelKey)}
                          {tool.wsl_distro ? ` · ${tool.wsl_distro}` : ""}
                        </span>
                      )}
                    </div>
                  </div>
                  {isToolVersionLoading ? (
                    <Loader2 className="mt-1 h-4 w-4 animate-spin text-muted-foreground" />
                  ) : tool?.version ? (
                    isOutdated ? (
                      <span className="mt-1 shrink-0 rounded-full border border-yellow-500/20 bg-yellow-500/10 px-1.5 py-0.5 text-[10px] text-yellow-600 dark:text-yellow-400">
                        {t("settings.updateAvailableShort")}
                      </span>
                    ) : (
                      <CheckCircle2 className="mt-1 h-4 w-4 shrink-0 text-green-500" />
                    )
                  ) : (
                    <AlertCircle className="mt-1 h-4 w-4 shrink-0 text-yellow-500" />
                  )}
                </div>

                <div className="space-y-1.5 text-xs">
                  <div className="flex items-center justify-between gap-3">
                    <span className="text-muted-foreground">
                      {t("settings.currentVersion")}
                    </span>
                    <span
                      className="min-w-0 truncate font-mono text-foreground"
                      title={tool?.version || undefined}
                    >
                      {isToolVersionLoading
                        ? t("common.loading")
                        : tool?.version
                          ? tool.version
                          : installedButBroken
                            ? t("settings.installedNotRunnable")
                            : t("common.notInstalled")}
                    </span>
                  </div>
                  <div className="flex items-center justify-between gap-3">
                    <span className="text-muted-foreground">
                      {t("settings.latestVersion")}
                    </span>
                    <span className="min-w-0 truncate font-mono text-foreground">
                      {isToolVersionLoading
                        ? t("common.loading")
                        : tool?.latest_version || t("common.unknown")}
                    </span>
                  </div>
                  {!isToolVersionLoading && !tool?.version && tool?.error && (
                    <ToolErrorMessage message={tool.error} />
                  )}
                </div>

                {tool?.env_type === "wsl" && (
                  <div className="flex flex-wrap gap-2">
                    <Select
                      value={wslShellByTool[toolName]?.wslShell || "auto"}
                      onValueChange={(v) => handleToolShellChange(toolName, v)}
                      disabled={isToolVersionLoading || isToolBusy}
                    >
                      <SelectTrigger className="h-7 w-[82px] text-xs">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="auto">{t("common.auto")}</SelectItem>
                        {WSL_SHELL_OPTIONS.map((shell) => (
                          <SelectItem key={shell} value={shell}>
                            {shell}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    <Select
                      value={wslShellByTool[toolName]?.wslShellFlag || "auto"}
                      onValueChange={(v) =>
                        handleToolShellFlagChange(toolName, v)
                      }
                      disabled={isToolVersionLoading || isToolBusy}
                    >
                      <SelectTrigger className="h-7 w-[82px] text-xs">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="auto">{t("common.auto")}</SelectItem>
                        {WSL_SHELL_FLAG_OPTIONS.map((flag) => (
                          <SelectItem key={flag} value={flag}>
                            {flag}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                )}

                {/* 多处安装冲突诊断结果：仅在懒触发后有数据时渲染。 */}
                {conflicts && conflicts.length > 0 && (
                  <div className="space-y-1.5 rounded-lg border border-yellow-500/20 bg-yellow-500/5 p-2.5">
                    <div className="text-[11px] font-medium text-yellow-600 dark:text-yellow-400">
                      {t("settings.toolConflictTitle")}
                    </div>
                    <p className="text-[10px] leading-snug text-muted-foreground">
                      {t("settings.toolConflictHint")}
                    </p>
                    <ul className="space-y-1.5">
                      {conflicts.map((inst) => (
                        <li key={inst.path}>
                          <ToolInstallRow inst={inst} />
                        </li>
                      ))}
                    </ul>
                  </div>
                )}

                <div className="mt-auto flex items-center justify-end">
                  {isToolVersionLoading && !isToolBusy ? (
                    <span className="text-xs text-muted-foreground">
                      {t("common.loading")}
                    </span>
                  ) : installedButBroken && !isToolBusy ? (
                    // 已安装但跑不起来：重装无济于事，不给按钮，给一句指向环境的提示。
                    <span className="text-xs text-yellow-600 dark:text-yellow-400">
                      {t("settings.toolCheckEnv")}
                    </span>
                  ) : action ? (
                    <Button
                      size="sm"
                      variant={action === "install" ? "outline" : "default"}
                      className="h-7 gap-1.5 text-xs"
                      onClick={() => handleRunToolAction([toolName], action)}
                      disabled={isToolVersionLoading || isToolBusy}
                      aria-busy={isToolBusy}
                    >
                      {/* 预检、等待确认与执行期间都显示该工具的进度。 */}
                      {isToolBusy ? (
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      ) : action === "install" ? (
                        <Download className="h-3.5 w-3.5" />
                      ) : (
                        <ArrowUpCircle className="h-3.5 w-3.5" />
                      )}
                      {/* loading 时文案保持不变、仅图标切换为 spinner，
                          按钮宽度恒定，避免"升级"→"升级中…"导致的抖动。 */}
                      {action === "install"
                        ? t("settings.toolInstall")
                        : t("settings.toolUpdate")}
                    </Button>
                  ) : (
                    <span className="text-xs text-muted-foreground">
                      {t("settings.toolReady")}
                    </span>
                  )}
                </div>
              </motion.div>
            );
          })}
        </div>
      </div>

      <motion.div
        initial={{ opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.3, delay: 0.3 }}
        className="space-y-3"
      >
        <button
          type="button"
          onClick={() => setShowInstallCommands((v) => !v)}
          aria-expanded={showInstallCommands}
          className="flex w-full items-center gap-1.5 px-1 text-sm font-medium text-foreground transition-colors hover:text-primary"
        >
          <ChevronDown
            className={`h-3.5 w-3.5 transition-transform ${
              showInstallCommands ? "" : "-rotate-90"
            }`}
          />
          {t("settings.manualInstallCommands")}
        </button>
        {showInstallCommands && (
          <div className="rounded-xl border border-border bg-gradient-to-br from-card/80 to-card/40 p-4 space-y-3 shadow-sm">
            <div className="flex items-center justify-between gap-2">
              <p className="text-xs text-muted-foreground">
                {t("settings.oneClickInstallHint")}
              </p>
              <Button
                size="sm"
                variant="outline"
                onClick={handleCopyInstallCommands}
                className="h-7 gap-1.5 text-xs"
              >
                <Copy className="h-3.5 w-3.5" />
                {t("common.copy")}
              </Button>
            </div>
            <pre className="text-xs font-mono bg-background/80 px-3 py-2.5 rounded-lg border border-border/60 overflow-x-auto">
              {ONE_CLICK_INSTALL_COMMANDS}
            </pre>
          </div>
        )}
      </motion.div>

      <ToolUpgradeConfirmDialog
        isOpen={pendingUpgrade !== null}
        plans={pendingUpgrade?.plans ?? []}
        displayName={toolDisplayName}
        onConfirm={handleConfirmUpgrade}
        onCancel={handleCancelUpgrade}
      />
    </motion.section>
  );
}
