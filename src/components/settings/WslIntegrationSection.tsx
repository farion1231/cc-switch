import { useCallback, useEffect, useState } from "react";
import {
  CheckCircle2,
  Loader2,
  RefreshCw,
  RotateCcw,
  Settings2,
  Terminal,
  XCircle,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { wslApi, type WslToolStatus } from "@/lib/api";
import { isWindows } from "@/lib/platform";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

const TOOL_NAMES = [
  "claude",
  "codex",
  "gemini",
  "opencode",
  "openclaw",
  "hermes",
] as const;

interface WslIntegrationSectionProps {
  /** 当前各工具的目录覆盖值 */
  directoryOverrides: Record<string, string | undefined>;
  /** 重置目录覆盖后的回调（用于刷新父组件状态） */
  onOverridesChanged: () => void;
}

export function WslIntegrationSection({
  directoryOverrides,
  onOverridesChanged,
}: WslIntegrationSectionProps) {
  const { t } = useTranslation();

  // 仅 Windows 显示
  if (!isWindows()) {
    return null;
  }

  const [distros, setDistros] = useState<string[]>([]);
  const [selectedDistro, setSelectedDistro] = useState<string>("");
  const [toolStatuses, setToolStatuses] = useState<WslToolStatus[]>([]);
  const [isDetectingDistros, setIsDetectingDistros] = useState(false);
  const [isDetectingTools, setIsDetectingTools] = useState(false);
  const [configuringTools, setConfiguringTools] = useState<Set<string>>(
    new Set(),
  );
  const [isApplyingAll, setIsApplyingAll] = useState(false);
  const [isResettingAll, setIsResettingAll] = useState(false);

  // 检测 WSL 发行版
  const detectDistros = useCallback(async () => {
    setIsDetectingDistros(true);
    try {
      const result = await wslApi.detectDistros();
      setDistros(result);
      if (result.length > 0 && !selectedDistro) {
        setSelectedDistro(result[0]);
      }
    } catch (error) {
      console.error("[WSL] Failed to detect distros:", error);
      toast.error(t("settings.advanced.wsl.detectError"));
    } finally {
      setIsDetectingDistros(false);
    }
  }, [selectedDistro, t]);

  // 检测 WSL 工具
  const detectTools = useCallback(
    async (distro: string) => {
      if (!distro) return;
      setIsDetectingTools(true);
      try {
        const result = await wslApi.detectTools(distro);
        setToolStatuses(result);
      } catch (error) {
        console.error("[WSL] Failed to detect tools:", error);
        toast.error(t("settings.advanced.wsl.detectToolsError"));
      } finally {
        setIsDetectingTools(false);
      }
    },
    [t],
  );

  // 初始检测
  useEffect(() => {
    detectDistros();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // 选择发行版后检测工具
  useEffect(() => {
    if (selectedDistro) {
      detectTools(selectedDistro);
    }
  }, [selectedDistro, detectTools]);

  // 单个工具一键配置
  const handleConfigure = useCallback(
    async (toolName: string) => {
      if (!selectedDistro) return;
      setConfiguringTools((prev) => new Set(prev).add(toolName));
      try {
        await wslApi.applyDirectoryOverrides(selectedDistro, [toolName]);
        toast.success(
          t("settings.advanced.wsl.applySuccess", { tool: toolName }),
        );
        // 刷新工具状态
        await detectTools(selectedDistro);
        onOverridesChanged();
      } catch (error) {
        console.error(`[WSL] Failed to configure ${toolName}:`, error);
        toast.error(
          t("settings.advanced.wsl.applyError", { tool: toolName }),
        );
      } finally {
        setConfiguringTools((prev) => {
          const next = new Set(prev);
          next.delete(toolName);
          return next;
        });
      }
    },
    [selectedDistro, detectTools, onOverridesChanged, t],
  );

  // 全部配置
  const handleApplyAll = useCallback(async () => {
    if (!selectedDistro) return;
    const toolsToConfigure = toolStatuses
      .filter((ts) => ts.installed && !ts.is_currently_overridden)
      .map((ts) => ts.name);

    if (toolsToConfigure.length === 0) {
      toast.info(t("settings.advanced.wsl.nothingToConfigure"));
      return;
    }

    setIsApplyingAll(true);
    try {
      const configured = await wslApi.applyDirectoryOverrides(
        selectedDistro,
        toolsToConfigure,
      );
      toast.success(
        t("settings.advanced.wsl.applyAllSuccess", {
          count: configured.length,
        }),
      );
      await detectTools(selectedDistro);
      onOverridesChanged();
    } catch (error) {
      console.error("[WSL] Failed to apply all:", error);
      toast.error(t("settings.advanced.wsl.applyAllError"));
    } finally {
      setIsApplyingAll(false);
    }
  }, [selectedDistro, toolStatuses, detectTools, onOverridesChanged, t]);

  // 全部重置
  const handleResetAll = useCallback(async () => {
    const overriddenTools = toolStatuses
      .filter((ts) => ts.is_currently_overridden)
      .map((ts) => ts.name);

    if (overriddenTools.length === 0) {
      toast.info(t("settings.advanced.wsl.nothingToReset"));
      return;
    }

    setIsResettingAll(true);
    try {
      await wslApi.resetDirectoryOverrides(overriddenTools);
      toast.success(t("settings.advanced.wsl.resetAllSuccess"));
      if (selectedDistro) {
        await detectTools(selectedDistro);
      }
      onOverridesChanged();
    } catch (error) {
      console.error("[WSL] Failed to reset all:", error);
      toast.error(t("settings.advanced.wsl.resetAllError"));
    } finally {
      setIsResettingAll(false);
    }
  }, [toolStatuses, selectedDistro, detectTools, onOverridesChanged, t]);

  // 单个工具重置
  const handleReset = useCallback(
    async (toolName: string) => {
      setConfiguringTools((prev) => new Set(prev).add(toolName));
      try {
        await wslApi.resetDirectoryOverrides([toolName]);
        toast.success(
          t("settings.advanced.wsl.resetSuccess", { tool: toolName }),
        );
        if (selectedDistro) {
          await detectTools(selectedDistro);
        }
        onOverridesChanged();
      } catch (error) {
        console.error(`[WSL] Failed to reset ${toolName}:`, error);
        toast.error(
          t("settings.advanced.wsl.resetError", { tool: toolName }),
        );
      } finally {
        setConfiguringTools((prev) => {
          const next = new Set(prev);
          next.delete(toolName);
          return next;
        });
      }
    },
    [selectedDistro, detectTools, onOverridesChanged, t],
  );

  const hasDistros = distros.length > 0;
  const hasOverriddenTools = toolStatuses.some((ts) => ts.is_currently_overridden);
  const hasConfigurableTools = toolStatuses.some(
    (ts) => ts.installed && !ts.is_currently_overridden,
  );

  return (
    <section className="space-y-4">
      {/* 发行版选择 */}
      <div className="flex items-center gap-3">
        <Terminal className="h-4 w-4 text-muted-foreground" />
        <span className="text-sm text-muted-foreground">
          {t("settings.advanced.wsl.detectedDistros")}
        </span>
        {isDetectingDistros ? (
          <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
        ) : hasDistros ? (
          <Select value={selectedDistro} onValueChange={setSelectedDistro}>
            <SelectTrigger className="w-[200px] h-8 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {distros.map((distro) => (
                <SelectItem key={distro} value={distro}>
                  {distro}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        ) : (
          <span className="text-sm text-muted-foreground">
            {t("settings.advanced.wsl.noDistros")}
          </span>
        )}
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7"
          onClick={detectDistros}
          disabled={isDetectingDistros}
          title={t("settings.advanced.wsl.refresh")}
        >
          <RefreshCw
            className={`h-3.5 w-3.5 ${isDetectingDistros ? "animate-spin" : ""}`}
          />
        </Button>
      </div>

      {/* 工具状态表 */}
      {hasDistros && selectedDistro && (
        <div className="space-y-2">
          {/* 表头 */}
          <div className="grid grid-cols-[1fr_100px_100px_100px] gap-2 text-xs text-muted-foreground font-medium px-1">
            <span>{t("settings.advanced.wsl.columnTool")}</span>
            <span className="text-center">
              {t("settings.advanced.wsl.columnWindows")}
            </span>
            <span className="text-center">
              {t("settings.advanced.wsl.columnWsl")}
            </span>
            <span className="text-center">
              {t("settings.advanced.wsl.columnAction")}
            </span>
          </div>

          {/* 工具行 */}
          {isDetectingTools ? (
            <div className="flex items-center justify-center py-6">
              <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
              <span className="ml-2 text-sm text-muted-foreground">
                {t("settings.advanced.wsl.detectingTools")}
              </span>
            </div>
          ) : (
            TOOL_NAMES.map((toolName) => {
              const status = toolStatuses.find((ts) => ts.name === toolName);
              const isConfiguring = configuringTools.has(toolName);

              return (
                <WslToolRow
                  key={toolName}
                  toolName={toolName}
                  status={status}
                  isConfiguring={isConfiguring}
                  onConfigure={() => handleConfigure(toolName)}
                  onReset={() => handleReset(toolName)}
                />
              );
            })
          )}

          {/* 操作按钮 */}
          {!isDetectingTools && (
            <div className="flex items-center gap-2 pt-2">
              <Button
                variant="outline"
                size="sm"
                className="text-xs"
                onClick={handleApplyAll}
                disabled={isApplyingAll || !hasConfigurableTools}
              >
                {isApplyingAll ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin mr-1.5" />
                ) : (
                  <Settings2 className="h-3.5 w-3.5 mr-1.5" />
                )}
                {t("settings.advanced.wsl.applyAll")}
              </Button>
              <Button
                variant="outline"
                size="sm"
                className="text-xs"
                onClick={handleResetAll}
                disabled={isResettingAll || !hasOverriddenTools}
              >
                {isResettingAll ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin mr-1.5" />
                ) : (
                  <RotateCcw className="h-3.5 w-3.5 mr-1.5" />
                )}
                {t("settings.advanced.wsl.resetAll")}
              </Button>
            </div>
          )}
        </div>
      )}
    </section>
  );
}

// ===== 工具行组件 =====

interface WslToolRowProps {
  toolName: string;
  status: WslToolStatus | undefined;
  isConfiguring: boolean;
  onConfigure: () => void;
  onReset: () => void;
}

function WslToolRow({
  toolName,
  status,
  isConfiguring,
  onConfigure,
  onReset,
}: WslToolRowProps) {
  const { t } = useTranslation();

  const isOverridden = status?.is_currently_overridden ?? false;
  const isInstalled = status?.installed ?? false;
  const configExists = status?.config_exists ?? false;

  return (
    <div className="grid grid-cols-[1fr_100px_100px_100px] gap-2 items-center text-xs px-1 py-1.5 rounded-md hover:bg-muted/50">
      {/* 工具名 */}
      <span className="font-medium text-foreground">{toolName}</span>

      {/* Windows 状态 */}
      <span className="text-center">
        {isOverridden ? (
          <span className="text-orange-500 dark:text-orange-400">
            {t("settings.advanced.wsl.statusOverridden")}
          </span>
        ) : (
          <span className="text-muted-foreground">
            {t("settings.advanced.wsl.statusDefault")}
          </span>
        )}
      </span>

      {/* WSL 状态 */}
      <span className="text-center">
        {isInstalled ? (
          configExists ? (
            <span className="text-green-500 dark:text-green-400 flex items-center justify-center gap-1">
              <CheckCircle2 className="h-3 w-3" />
              {t("settings.advanced.wsl.statusInstalled")}
            </span>
          ) : (
            <span className="text-yellow-500 dark:text-yellow-400">
              {t("settings.advanced.wsl.statusNoConfig")}
            </span>
          )
        ) : (
          <span className="text-muted-foreground flex items-center justify-center gap-1">
            <XCircle className="h-3 w-3" />
            {t("settings.advanced.wsl.statusNotInstalled")}
          </span>
        )}
      </span>

      {/* 操作 */}
      <span className="text-center">
        {isConfiguring ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin mx-auto text-muted-foreground" />
        ) : isOverridden ? (
          <Button
            variant="ghost"
            size="sm"
            className="h-6 px-2 text-xs text-muted-foreground hover:text-foreground"
            onClick={onReset}
          >
            {t("settings.advanced.wsl.reset")}
          </Button>
        ) : isInstalled && configExists ? (
          <Button
            variant="ghost"
            size="sm"
            className="h-6 px-2 text-xs text-primary"
            onClick={onConfigure}
          >
            {t("settings.advanced.wsl.configure")}
          </Button>
        ) : (
          <span className="text-muted-foreground">—</span>
        )}
      </span>
    </div>
  );
}
