import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  AlertTriangle,
  FolderInput,
  FolderOpen,
  RefreshCw,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type {
  PluginInfo,
  PluginListResult,
  PluginReloadResult,
} from "@/types/plugin";

// 插件系统管理面板：调用 src-tauri 的 plugin_* 系列命令（契约见 docs/dev/plugin-system-contract.md 2.8）

const I18N_PREFIX = "settings.advanced.plugins";

interface PluginImportResult {
  pluginId: string;
  dirName: string;
}

export function PluginSettingsPanel() {
  const { t } = useTranslation();
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [isReloading, setIsReloading] = useState(false);
  const [reloadErrors, setReloadErrors] = useState<string[]>([]);
  // 全局总开关：由后端显式返回（绝不能从"全部条目是否启用"推导——单个插件的
  // 用户覆盖会被误当总开关，Codex 审查 P1）
  const [globalEnabled, setGlobalEnabled] = useState(true);
  // 优先级输入草稿：失焦/回车时才提交，避免每敲一个字符调一次后端
  const [priorityDrafts, setPriorityDrafts] = useState<Record<string, string>>(
    {},
  );

  const loadPlugins = useCallback(async () => {
    try {
      const result = await invoke<PluginListResult>("plugin_list");
      setPlugins(result.plugins);
      setGlobalEnabled(result.globalEnabled);
      // 注意：这里不能清空 reloadErrors——重载成功后紧接着会调用本函数刷新列表，
      // 若在此清空，重载返回的加载错误提示会闪现即逝
      setPriorityDrafts(
        Object.fromEntries(
          result.plugins.map((p) => [p.id, String(p.priority)]),
        ),
      );
    } catch (e) {
      console.error("Failed to load plugins:", e);
      toast.error(String(e));
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadPlugins();
  }, [loadPlugins]);

  const handleToggleGlobal = async (checked: boolean) => {
    const previous = globalEnabled;
    setGlobalEnabled(checked);
    // 乐观更新：总开关关闭时所有插件视为停用
    setPlugins((prev) =>
      prev.map((p) => (p.error ? p : { ...p, enabled: checked })),
    );
    try {
      await invoke("plugin_set_all_enabled", { enabled: checked });
    } catch (e) {
      console.error("Failed to set all plugins enabled:", e);
      toast.error(String(e));
      setGlobalEnabled(previous);
      setPlugins((prev) =>
        prev.map((p) => (p.error ? p : { ...p, enabled: previous })),
      );
    }
  };

  const handleTogglePlugin = async (plugin: PluginInfo, checked: boolean) => {
    const previous = plugin.enabled;
    setPlugins((prev) =>
      prev.map((p) => (p.id === plugin.id ? { ...p, enabled: checked } : p)),
    );
    try {
      await invoke("plugin_set_enabled", { id: plugin.id, enabled: checked });
    } catch (e) {
      console.error("Failed to set plugin enabled:", e);
      toast.error(String(e));
      setPlugins((prev) =>
        prev.map((p) => (p.id === plugin.id ? { ...p, enabled: previous } : p)),
      );
    }
  };

  const commitPriority = async (plugin: PluginInfo) => {
    const draft = priorityDrafts[plugin.id];
    if (draft === undefined) return;
    const priority = Number(draft);
    if (draft.trim() === "" || !Number.isFinite(priority)) {
      setPriorityDrafts((prev) => ({
        ...prev,
        [plugin.id]: String(plugin.priority),
      }));
      return;
    }
    if (priority === plugin.priority) return;

    setPlugins((prev) =>
      prev.map((p) => (p.id === plugin.id ? { ...p, priority } : p)),
    );
    try {
      await invoke("plugin_set_priority", { id: plugin.id, priority });
    } catch (e) {
      console.error("Failed to set plugin priority:", e);
      toast.error(String(e));
      setPlugins((prev) =>
        prev.map((p) =>
          p.id === plugin.id ? { ...p, priority: plugin.priority } : p,
        ),
      );
      setPriorityDrafts((prev) => ({
        ...prev,
        [plugin.id]: String(plugin.priority),
      }));
    }
  };

  const handleReload = async () => {
    setIsReloading(true);
    try {
      const result = await invoke<PluginReloadResult>("plugin_reload");
      setReloadErrors(result.errors ?? []);
      toast.success(
        t(`${I18N_PREFIX}.reloadSuccess`, { loaded: result.loaded }),
      );
      await loadPlugins();
    } catch (e) {
      console.error("Failed to reload plugins:", e);
      toast.error(String(e));
    } finally {
      setIsReloading(false);
    }
  };

  const handleOpenDir = async () => {
    try {
      await invoke("plugin_open_dir");
    } catch (e) {
      console.error("Failed to open plugin dir:", e);
      toast.error(String(e));
    }
  };

  const handleImport = async () => {
    // 文件夹选择器：取消返回 null
    const selected = await open({
      directory: true,
      multiple: false,
      title: t(`${I18N_PREFIX}.importDialogTitle`),
    });
    if (!selected) return;

    setIsReloading(true);
    try {
      const result = await invoke<PluginImportResult>("plugin_import", {
        sourceDir: selected,
      });
      toast.success(
        t(`${I18N_PREFIX}.importSuccess`, {
          name: result.dirName,
          id: result.pluginId,
        }),
      );
      await loadPlugins();
    } catch (e) {
      console.error("Failed to import plugin:", e);
      toast.error(String(e));
    } finally {
      setIsReloading(false);
    }
  };

  if (isLoading) return null;

  return (
    <div className="space-y-6">
      {/* 顶部：全局总开关 + 重载 + 打开插件目录 */}
      <div className="flex items-center justify-between gap-4">
        <div className="space-y-0.5">
          <Label>{t(`${I18N_PREFIX}.globalEnabled`)}</Label>
          <p className="text-xs text-muted-foreground">
            {t(`${I18N_PREFIX}.globalEnabledDescription`)}
          </p>
        </div>
        <Switch
          checked={globalEnabled}
          onCheckedChange={(checked) => void handleToggleGlobal(checked)}
        />
      </div>

      <div className="flex items-center gap-2">
        <Button
          variant="outline"
          size="sm"
          onClick={() => void handleReload()}
          disabled={isReloading}
        >
          <RefreshCw
            className={`mr-2 h-4 w-4 ${isReloading ? "animate-spin" : ""}`}
          />
          {t(`${I18N_PREFIX}.reload`)}
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void handleImport()}
          disabled={isReloading}
        >
          <FolderInput className="mr-2 h-4 w-4" />
          {t(`${I18N_PREFIX}.import`)}
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void handleOpenDir()}
        >
          <FolderOpen className="mr-2 h-4 w-4" />
          {t(`${I18N_PREFIX}.openDir`)}
        </Button>
      </div>

      {/* 重载后的加载错误列表 */}
      {reloadErrors.length > 0 && (
        <div className="rounded-lg bg-yellow-500/10 border border-yellow-500/20 p-4 space-y-1.5">
          <p className="flex items-center gap-2 text-sm font-medium text-yellow-600 dark:text-yellow-400">
            <AlertTriangle className="h-4 w-4" />
            {t(`${I18N_PREFIX}.reloadErrorsTitle`)}
          </p>
          <ul className="list-disc pl-6 text-xs text-yellow-600 dark:text-yellow-400 space-y-1">
            {reloadErrors.map((err, i) => (
              <li key={i} className="break-all">
                {err}
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* 插件列表 */}
      {plugins.length === 0 ? (
        <p className="text-sm text-muted-foreground">
          {t(`${I18N_PREFIX}.empty`)}
        </p>
      ) : (
        <div className="rounded-lg border border-border/50 divide-y divide-border/50">
          {plugins.map((plugin) => {
            const failed = plugin.error !== null;
            return (
              <div
                key={plugin.id}
                className={`flex items-start justify-between gap-4 px-4 py-3 ${
                  failed ? "opacity-60" : ""
                }`}
              >
                <div className="min-w-0 space-y-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-sm font-medium">
                      {plugin.displayName}
                    </span>
                    <Badge
                      variant={
                        failed
                          ? "destructive"
                          : plugin.isBuiltin
                            ? "secondary"
                            : "default"
                      }
                      className="px-1.5 py-0 text-[10px]"
                    >
                      {failed
                        ? t(`${I18N_PREFIX}.loadFailed`)
                        : plugin.isBuiltin
                          ? t(`${I18N_PREFIX}.badgeBuiltin`)
                          : t(`${I18N_PREFIX}.badgeUser`)}
                    </Badge>
                    {plugin.stages.map((stage) => (
                      <span
                        key={stage}
                        title={t(`${I18N_PREFIX}.stageHint`)}
                        className="inline-flex items-center rounded bg-muted px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground"
                      >
                        {stage}
                      </span>
                    ))}
                  </div>
                  <p className="text-xs text-muted-foreground font-mono truncate">
                    {plugin.id}
                  </p>
                  {plugin.description && (
                    <p className="text-xs text-muted-foreground">
                      {plugin.description}
                    </p>
                  )}
                  {failed && (
                    <p className="text-xs text-destructive break-all">
                      {plugin.error}
                    </p>
                  )}
                </div>

                <div className="flex flex-shrink-0 items-start gap-3">
                  <div className="flex flex-col items-end gap-1">
                    <span className="text-[10px] leading-none text-muted-foreground">
                      {t(`${I18N_PREFIX}.priority`)}
                    </span>
                    <Input
                      type="number"
                      aria-label={`${t(`${I18N_PREFIX}.priority`)}: ${plugin.displayName}`}
                      title={t(`${I18N_PREFIX}.priorityHint`)}
                      className="h-8 w-20"
                      value={
                        priorityDrafts[plugin.id] ?? String(plugin.priority)
                      }
                      disabled={failed || !globalEnabled}
                      onChange={(e) =>
                        setPriorityDrafts((prev) => ({
                          ...prev,
                          [plugin.id]: e.target.value,
                        }))
                      }
                      onBlur={() => void commitPriority(plugin)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") {
                          (e.target as HTMLInputElement).blur();
                        }
                      }}
                    />
                  </div>
                  <div className="flex flex-col items-end gap-1">
                    <span className="text-[10px] leading-none text-muted-foreground">
                      {t(`${I18N_PREFIX}.enabledLabel`)}
                    </span>
                    <Switch
                      checked={plugin.enabled && !failed}
                      disabled={failed || !globalEnabled}
                      onCheckedChange={(checked) =>
                        void handleTogglePlugin(plugin, checked)
                      }
                    />
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
