import { useCallback, useEffect, useMemo, useState } from "react";
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
  "opencode",
  "openclaw",
  "hermes",
] as const;
type ToolName = (typeof TOOL_NAMES)[number];
type ToolLifecycleAction = "install" | "update";

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

const POSIX_ONE_CLICK_INSTALL_COMMANDS = `# Claude Code
${posixScriptInstallCommand("https://claude.ai/install.sh")} || npm i -g @anthropic-ai/claude-code@latest
# Codex
npm i -g @openai/codex@latest
# Gemini CLI
npm i -g @google/gemini-cli@latest
# OpenCode
${posixScriptInstallCommand("https://opencode.ai/install")} || npm i -g opencode-ai@latest
# OpenClaw
npm i -g openclaw@latest
# Hermes
${posixScriptInstallCommand("https://raw.githubusercontent.com/NousResearch/hermes-agent/main/scripts/install.sh")}`;

const WINDOWS_ONE_CLICK_INSTALL_COMMANDS = `# Claude Code
npm i -g @anthropic-ai/claude-code@latest
# Codex
npm i -g @openai/codex@latest
# Gemini CLI
npm i -g @google/gemini-cli@latest
# OpenCode
npm i -g opencode-ai@latest
# OpenClaw
npm i -g openclaw@latest
# Hermes
${HERMES_WINDOWS_INSTALL_COMMAND}`;

const ONE_CLICK_INSTALL_COMMANDS = isWindows()
  ? WINDOWS_ONE_CLICK_INSTALL_COMMANDS
  : POSIX_ONE_CLICK_INSTALL_COMMANDS;

const TOOL_DISPLAY_NAMES: Record<ToolName, string> = {
  claude: "Claude Code",
  codex: "Codex",
  gemini: "Gemini CLI",
  opencode: "OpenCode",
  openclaw: "OpenClaw",
  hermes: "Hermes",
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
  opencode: "opencode",
  openclaw: "openclaw",
  hermes: "hermes",
};

export function AboutSection({ isPortable }: AboutSectionProps) {
  // ... (use hooks as before) ...
  const { t } = useTranslation();
  const [version, setVersion] = useState<string | null>(null);
  const [isLoadingVersion, setIsLoadingVersion] = useState(true);
  const [isDownloading, setIsDownloading] = useState(false);
  const [toolVersions, setToolVersions] = useState<ToolVersion[]>([]);
  const [isLoadingTools, setIsLoadingTools] = useState(true);
  const [toolActions, setToolActions] = useState<
    Partial<Record<ToolName, ToolLifecycleAction>>
  >({});
  const [batchAction, setBatchAction] = useState<ToolLifecycleAction | null>(
    null,
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
  // 升级前探测到「多处安装需确认」时暂存：toolNames=本次要升级的全部工具，
  // plans=其中需要确认的（≥2 处）那些。用户确认后对 toolNames 整体执行升级。
  const [pendingUpgrade, setPendingUpgrade] = useState<{
    toolNames: ToolName[];
    plans: ToolInstallationReport[];
  } | null>(null);
  // 升级 preflight(probe 阶段)的 in-flight 工具集合。
  // probeToolInstallations 是个 1-3 秒级别的跨进程探测(对每个工具跑 --version + canonicalize),
  // 在它返回之前 toolActions / batchAction 都还没被置位 → 按钮不会 disabled → 用户快速双击
  // 会并发开两轮 probe,各自再触发 executeRun(并发的 `npm i -g` / 官方 installer,写冲突)。
  // 把 probe 期间的工具登记在这里、纳入 isAnyBusy 派生,关掉这个并发窗口。
  // 用 Set 而非 boolean:单卡片升级 & 批量升级可能在不同工具上独立 preflight,
  // 精确反映到各自卡片按钮的 disabled。
  const [preflightTools, setPreflightTools] = useState<Set<ToolName>>(
    () => new Set(),
  );

  const toolVersionByName = useMemo(() => {
    return new Map(toolVersions.map((tool) => [tool.name, tool]));
  }, [toolVersions]);

  const updatableToolNames = useMemo(
    () =>
      TOOL_NAMES.filter((toolName) => {
        const tool = toolVersionByName.get(toolName);
        return isUpdateAvailable(tool?.version, tool?.latest_version);
      }),
    [toolVersionByName],
  );

  const refreshToolVersions = useCallback(
    async (
      toolNames: ToolName[],
      wslOverrides?: Record<string, WslShellPreference>,
    ): Promise<ToolVersion[]> => {
      if (toolNames.length === 0) return [];

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

        setToolVersions((prev) => {
          if (prev.length === 0) return updated;
          const byName = new Map(updated.map((t) => [t.name, t]));
          const merged = prev.map((t) => byName.get(t.name) ?? t);
          const existing = new Set(prev.map((t) => t.name));
          for (const u of updated) {
            if (!existing.has(u.name)) merged.push(u);
          }
          return merged;
        });

        // 返回刷新结果，调用方可据此判断版本是否真的探到（避免读 state 撞 stale closure）。
        return updated;
      } catch (error) {
        console.error("[AboutSection] Failed to refresh tools", error);
        return [];
      } finally {
        setLoadingTools((prev) => {
          const next = { ...prev };
          for (const name of toolNames) next[name] = false;
          return next;
        });
      }
    },
    [],
  );

  const loadAllToolVersions = useCallback(async () => {
    setIsLoadingTools(true);
    try {
      // Respect current UI overrides (shell / flag) when doing a full refresh.
      const versions = await settingsApi.getToolVersions(
        [...TOOL_NAMES],
        wslShellByTool,
      );
      setToolVersions(versions);
    } catch (error) {
      console.error("[AboutSection] Failed to load tool versions", error);
    } finally {
      setIsLoadingTools(false);
    }
  }, [wslShellByTool]);

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
    const load = async () => {
      try {
        const [appVersion] = await Promise.all([
          getVersion(),
          loadAllToolVersions(),
        ]);

        if (active) {
          setVersion(appVersion);
        }
      } catch (error) {
        console.error("[AboutSection] Failed to load info", error);
        if (active) {
          setVersion(null);
        }
      } finally {
        if (active) {
          setIsLoadingVersion(false);
        }
      }
    };

    void load();
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
      toast.error(t("settings.checkUpdateFailed"));
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

  // 实际执行安装/升级的串行循环（已通过任何必要的确认后才调用）。
  const executeRun = useCallback(
    async (toolNames: ToolName[], action: ToolLifecycleAction) => {
      const isBatch = toolNames.length > 1;
      if (isBatch) {
        setBatchAction(action);
      }

      // 逐工具串行执行：每个工具独立成败、独立刷新版本，一个失败不会连坐
      // 后续工具（后端把整批拼成单脚本 + set -e，会在首个失败处中止整批）。
      // soft=true 表示"命令成功执行但结果仍需用户介入"（版本没变/装上却跑不起来），
      // 与命令本身报错（soft=false）区别对待：前者不算硬失败，toast 降级为 warning。
      const failures: {
        toolName: ToolName;
        detail: string;
        soft: boolean;
        kind?: "notRunnable" | "versionUnchanged";
      }[] = [];
      let succeeded = 0;

      for (const toolName of toolNames) {
        setToolActions((prev) => ({ ...prev, [toolName]: action }));
        try {
          const previousTool = toolVersionByName.get(toolName);
          const previousVersion = previousTool?.version ?? null;
          const previousLatestVersion = previousTool?.latest_version ?? null;

          await settingsApi.runToolLifecycleAction(
            [toolName],
            action,
            wslShellByTool,
          );
          // 静默执行真正结束后刷新该工具版本，卡片立即反映结果。
          const refreshed = await refreshToolVersions(
            [toolName],
            wslShellByTool,
          );
          const tool = refreshed.find((t) => t.name === toolName);
          if (tool?.version) {
            const latestVersion = tool.latest_version ?? previousLatestVersion;
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
            const detail = tool?.error?.trim() || t("settings.toolNotRunnable");
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
          console.error(
            `[AboutSection] Failed to run tool action for ${toolName}`,
            error,
          );
          const detail = extractErrorMessage(error) || String(error);
          failures.push({ toolName, detail, soft: false });
        } finally {
          setToolActions((prev) => {
            const next = { ...prev };
            delete next[toolName];
            return next;
          });
        }
      }

      if (isBatch) {
        setBatchAction(null);
      }

      const actionLabel =
        action === "install"
          ? t("settings.toolInstall")
          : t("settings.toolUpdate");

      if (failures.length === 0) {
        toast.success(
          t("settings.toolActionDone", {
            count: succeeded,
            action: actionLabel,
          }),
          { closeButton: true },
        );
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
      wslShellByTool,
      toolVersionByName,
      refreshToolVersions,
      diagnoseToolSilently,
    ],
  );

  // 升级/安装的统一入口锁。所有动作在入口处先登记 preflight、出口处解锁;
  // 这层锁覆盖三个否则会漏掉的窗口:
  //   ① update 的 probeToolInstallations 阶段(1-3 秒跨进程,executeRun 之前);
  //   ② executeRun 内部 setToolActions 落到 React commit 前的几个 microtask;
  //   ③ install 直接进 executeRun 的同一段 microtask 窗口。
  // 早退检查避免同一工具在 toolActions / preflight 已登记时被重复入栈——批量场景里
  // 只要有一个工具被锁,整批不开新一轮,因为后端 set -e 串行的语义假设是「一次性
  // 单脚本」,跨两次 IPC 并发会破坏它。
  const handleRunToolAction = useCallback(
    async (toolNames: ToolName[], action: ToolLifecycleAction) => {
      if (toolNames.length === 0) return;
      if (
        toolNames.some(
          (name) => preflightTools.has(name) || toolActions[name] !== undefined,
        )
      ) {
        return;
      }
      // 入栈 preflight,按钮立刻 disabled。new Set(prev) 是不可变更新(直接 mutate
      // 原 Set 会让 React 复用引用、跳过 re-render);finally 块负责异常路径解锁。
      setPreflightTools((prev) => {
        const next = new Set(prev);
        toolNames.forEach((name) => next.add(name));
        return next;
      });
      try {
        if (action === "install") {
          await executeRun(toolNames, action);
          return;
        }
        let reports: ToolInstallationReport[];
        try {
          reports = await settingsApi.probeToolInstallations(toolNames);
        } catch (error) {
          // 探测失败不应阻断升级：退回直接执行（等同旧行为）。
          console.error("[AboutSection] probeToolInstallations failed", error);
          await executeRun(toolNames, action);
          return;
        }
        const needConfirm = reports.filter((r) => r.needs_confirmation);
        if (needConfirm.length === 0) {
          await executeRun(toolNames, action);
          return;
        }
        setPendingUpgrade({ toolNames, plans: needConfirm });
      } finally {
        setPreflightTools((prev) => {
          const next = new Set(prev);
          toolNames.forEach((name) => next.delete(name));
          return next;
        });
      }
    },
    [executeRun, preflightTools, toolActions],
  );

  const handleConfirmUpgrade = useCallback(() => {
    if (pendingUpgrade) {
      void executeRun(pendingUpgrade.toolNames, "update");
    }
    setPendingUpgrade(null);
  }, [pendingUpgrade, executeRun]);

  const handleCancelUpgrade = useCallback(() => setPendingUpgrade(null), []);

  const displayVersion = version ?? t("common.unknown");

  // 任一安装/升级进行中（批量或单工具）即视为忙碌：用于禁用所有操作按钮，
  // 避免并发触发多个 npm/pip 全局写入造成冲突。
  // preflightTools 覆盖升级前的 probe 阶段——那段在 executeRun 之前、toolActions
  // 还没置位,如果不算进 busy 会留出 1-3 秒的并发触发窗口。
  const isAnyBusy =
    Boolean(batchAction) ||
    Object.keys(toolActions).length > 0 ||
    preflightTools.size > 0;

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
          <div className="space-y-2">
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

          <div className="flex items-center gap-2">
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
              onClick={() =>
                settingsApi.openExternal(
                  "https://github.com/farion1231/cc-switch",
                )
              }
              className="h-8 gap-1.5 text-xs"
            >
              <Github className="h-3.5 w-3.5" />
              {t("settings.github")}
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
              onClick={() => loadAllToolVersions()}
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
              onClick={() => handleRunToolAction(updatableToolNames, "update")}
              disabled={
                isLoadingTools || isAnyBusy || updatableToolNames.length === 0
              }
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
            const isToolVersionLoading =
              isLoadingTools || Boolean(loadingTools[toolName]);
            const isOutdated = isUpdateAvailable(
              tool?.version,
              tool?.latest_version,
            );
            // 已安装却跑不起来（如 Node 版本不达标）：用它区分卡片文案与按钮，避免把
            // "装了跑不起来"误判成"未安装"而给出无用的安装按钮（重装同一版本解决不了）。
            const installedButBroken = Boolean(tool?.installed_but_broken);
            // loading 和 broken 都没有可执行动作；其余按是否已装/是否过期选择。
            const action: ToolLifecycleAction | null =
              isToolVersionLoading || installedButBroken
                ? null
                : !tool?.version
                  ? "install"
                  : isOutdated
                    ? "update"
                    : null;
            const runningAction = toolActions[toolName];
            const title = tool?.version || tool?.error || t("common.unknown");
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
                      title={title}
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
                    <div className="truncate text-[11px] text-muted-foreground">
                      {tool.error}
                    </div>
                  )}
                </div>

                {tool?.env_type === "wsl" && (
                  <div className="flex flex-wrap gap-2">
                    <Select
                      value={wslShellByTool[toolName]?.wslShell || "auto"}
                      onValueChange={(v) => handleToolShellChange(toolName, v)}
                      disabled={
                        isLoadingTools || loadingTools[toolName] || isAnyBusy
                      }
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
                      disabled={
                        isLoadingTools || loadingTools[toolName] || isAnyBusy
                      }
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
                  {isToolVersionLoading ? (
                    <span className="text-xs text-muted-foreground">
                      {t("common.loading")}
                    </span>
                  ) : installedButBroken ? (
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
                      disabled={isToolVersionLoading || isAnyBusy}
                    >
                      {runningAction ? (
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
