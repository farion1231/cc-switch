import React, { useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  Sparkles,
  Trash2,
  ExternalLink,
  RefreshCw,
  Search,
  GitCommitHorizontal,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  useInstalledSkills,
  useToggleSkillApp,
  useUninstallSkill,
  useScanUnmanagedSkills,
  useImportSkillsFromApps,
  useInstallSkillsFromZip,
  useSkillUpdates,
  useBatchUpdateSkills,
  type InstalledSkill,
} from "@/hooks/useSkills";
import type { AppId } from "@/lib/api/types";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { settingsApi, skillsApi } from "@/lib/api";
import { toast } from "sonner";
import { MCP_SKILLS_APP_IDS } from "@/config/appConfig";
import { AppCountBar } from "@/components/common/AppCountBar";
import { AppToggleGroup } from "@/components/common/AppToggleGroup";
import { ListItemRow } from "@/components/common/ListItemRow";

interface UnifiedSkillsPanelProps {
  onOpenDiscovery: () => void;
}

export interface UnifiedSkillsPanelHandle {
  openDiscovery: () => void;
  openImport: () => void;
  openInstallFromZip: () => void;
}

const UnifiedSkillsPanel = React.forwardRef<
  UnifiedSkillsPanelHandle,
  UnifiedSkillsPanelProps
>(({ onOpenDiscovery }, ref) => {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [confirmDialog, setConfirmDialog] = useState<{
    isOpen: boolean;
    title: string;
    message: string;
    onConfirm: () => void;
  } | null>(null);
  const [importDialogOpen, setImportDialogOpen] = useState(false);
  const [isCheckingUpdates, setIsCheckingUpdates] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [isBatchUninstalling, setIsBatchUninstalling] = useState(false);
  const [isBatchTogglingApps, setIsBatchTogglingApps] = useState(false);
  const [batchModeEnabled, setBatchModeEnabled] = useState(false);
  const [pendingAppChanges, setPendingAppChanges] = useState<
    Record<string, Partial<Record<AppId, boolean>>>
  >({});
  const [searchQuery, setSearchQuery] = useState("");
  const [filterStatus, setFilterStatus] = useState<
    "all" | "update_available" | "up_to_date" | "unknown"
  >("all");

  const { data: skills, isLoading } = useInstalledSkills();
  const toggleAppMutation = useToggleSkillApp();
  const uninstallMutation = useUninstallSkill();
  const { data: unmanagedSkills, refetch: scanUnmanaged } =
    useScanUnmanagedSkills();
  const importMutation = useImportSkillsFromApps();
  const installFromZipMutation = useInstallSkillsFromZip();
  const { data: updateStatuses, refetch: refetchUpdates } = useSkillUpdates();
  const batchUpdateMutation = useBatchUpdateSkills();

  const updateMap = useMemo(() => {
    const map = new Map<
      string,
      "update_available" | "up_to_date" | "unknown" | "not_found"
    >();
    (updateStatuses ?? []).forEach((item) => map.set(item.id, item.state));
    return map;
  }, [updateStatuses]);

  const enabledCounts = useMemo(() => {
    const counts = { claude: 0, codex: 0, gemini: 0, opencode: 0, openclaw: 0 };
    if (!skills) return counts;
    skills.forEach((skill) => {
      for (const app of MCP_SKILLS_APP_IDS) {
        if (skill.apps[app]) counts[app]++;
      }
    });
    return counts;
  }, [skills]);

  const filteredSkills = useMemo(() => {
    const byStatus = (skills ?? []).filter((skill) => {
      const state = updateMap.get(skill.id);
      if (filterStatus === "update_available") {
        return state === "update_available";
      }
      if (filterStatus === "up_to_date") {
        return state === "up_to_date";
      }
      if (filterStatus === "unknown") {
        return state === "unknown" || state === "not_found";
      }
      return true;
    });

    if (!searchQuery.trim()) return byStatus;

    const query = searchQuery.toLowerCase();
    return byStatus.filter((skill) => {
      const name = skill.name.toLowerCase();
      const repo =
        skill.repoOwner && skill.repoName
          ? `${skill.repoOwner}/${skill.repoName}`.toLowerCase()
          : "";
      return name.includes(query) || repo.includes(query);
    });
  }, [filterStatus, searchQuery, skills, updateMap]);

  const selectedUpdatableIds = useMemo(
    () =>
      filteredSkills
        .filter(
          (skill) =>
            selected.has(skill.id) &&
            updateMap.get(skill.id) === "update_available",
        )
        .map((skill) => skill.id),
    [filteredSkills, selected, updateMap],
  );

  const selectedCount = useMemo(
    () => (skills ?? []).filter((skill) => selected.has(skill.id)).length,
    [skills, selected],
  );

  const selectedSkillIds = useMemo(
    () =>
      (skills ?? [])
        .filter((skill) => selected.has(skill.id))
        .map((skill) => skill.id),
    [skills, selected],
  );

  const pendingChangeCount = useMemo(
    () =>
      Object.values(pendingAppChanges).reduce(
        (sum, appChanges) => sum + Object.keys(appChanges).length,
        0,
      ),
    [pendingAppChanges],
  );

  const getEffectiveApps = (skill: InstalledSkill): Record<AppId, boolean> => ({
    ...skill.apps,
    ...pendingAppChanges[skill.id],
  });

  const handleToggleApp = async (id: string, app: AppId, enabled: boolean) => {
    try {
      await toggleAppMutation.mutateAsync({ id, app, enabled });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleUninstall = (skill: InstalledSkill) => {
    setConfirmDialog({
      isOpen: true,
      title: t("skills.uninstall"),
      message: t("skills.uninstallConfirm", { name: skill.name }),
      onConfirm: async () => {
        try {
          await uninstallMutation.mutateAsync(skill.id);
          setConfirmDialog(null);
          toast.success(t("skills.uninstallSuccess", { name: skill.name }), {
            closeButton: true,
          });
        } catch (error) {
          toast.error(t("common.error"), { description: String(error) });
        }
      },
    });
  };

  const toggleSelect = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  const handleSelectAllVisible = () => {
    if (filteredSkills.length === 0) return;
    setSelected((prev) => {
      const next = new Set(prev);
      filteredSkills.forEach((skill) => next.add(skill.id));
      return next;
    });
  };

  const handleClearSelection = () => setSelected(new Set());

  const handleModeChange = (checked: boolean) => {
    if (!checked && pendingChangeCount > 0) {
      setConfirmDialog({
        isOpen: true,
        title: t("skills.pendingDiscardTitle", {
          defaultValue: "Discard pending changes?",
        }),
        message: t("skills.pendingDiscardMessage", {
          count: pendingChangeCount,
          defaultValue:
            "You have {{count}} pending app changes that have not been applied. Leaving batch mode will discard them.",
        }),
        onConfirm: () => {
          setPendingAppChanges({});
          setBatchModeEnabled(false);
          setConfirmDialog(null);
        },
      });
      return;
    }

    setBatchModeEnabled(checked);
  };

  const handleBatchUninstall = () => {
    const targets = filteredSkills.filter((skill) => selected.has(skill.id));
    if (targets.length === 0) {
      toast.info(
        t("skills.bulkUninstallNoop", {
          defaultValue: "没有可卸载的已选技能",
        }),
      );
      return;
    }

    setConfirmDialog({
      isOpen: true,
      title: t("skills.bulkUninstall", {
        defaultValue: "卸载已选（{{count}}）",
        count: targets.length,
      }),
      message: t("skills.bulkUninstallConfirm", {
        defaultValue:
          "确定要卸载已选的 {{count}} 个技能吗？这将从所有应用中移除这些技能。",
        count: targets.length,
      }),
      onConfirm: async () => {
        setIsBatchUninstalling(true);
        try {
          const results = await Promise.allSettled(
            targets.map((skill) => skillsApi.uninstallUnified(skill.id)),
          );
          const success = results.filter(
            (item) => item.status === "fulfilled",
          ).length;
          const failed = results.length - success;

          await Promise.all([
            queryClient.invalidateQueries({
              queryKey: ["skills", "installed"],
            }),
            queryClient.invalidateQueries({
              queryKey: ["skills", "discoverable"],
            }),
            queryClient.invalidateQueries({ queryKey: ["skills", "updates"] }),
          ]);

          setSelected(new Set());
          setConfirmDialog(null);

          if (failed === 0) {
            toast.success(
              t("skills.bulkUninstallSuccess", {
                defaultValue: "已卸载 {{count}} 个技能",
                count: success,
              }),
              { closeButton: true },
            );
          } else {
            const firstFailure = results.find(
              (item) => item.status === "rejected",
            ) as PromiseRejectedResult | undefined;
            toast.warning(
              t("skills.bulkUninstallPartial", {
                defaultValue: "卸载完成：成功 {{success}}，失败 {{failed}}",
                success,
                failed,
              }),
              {
                description: firstFailure?.reason
                  ? String(firstFailure.reason)
                  : undefined,
              },
            );
          }
        } catch (error) {
          toast.error(t("common.error"), { description: String(error) });
        } finally {
          setIsBatchUninstalling(false);
        }
      },
    });
  };

  const handleStageBatchToggle = (
    skillId: string,
    app: AppId,
    enabled: boolean,
  ) => {
    const targetIds =
      selectedSkillIds.length === 0 || !selected.has(skillId)
        ? Array.from(new Set([...selectedSkillIds, skillId]))
        : selectedSkillIds;

    if (!selected.has(skillId)) {
      setSelected((prev) => new Set(prev).add(skillId));
    }

    setPendingAppChanges((prev) => {
      const next = { ...prev };
      targetIds.forEach((id) => {
        next[id] = {
          ...(next[id] ?? {}),
          [app]: enabled,
        };
      });
      return next;
    });
  };

  const handleAppToggle = async (
    skill: InstalledSkill,
    app: AppId,
    enabled: boolean,
  ) => {
    if (batchModeEnabled) {
      handleStageBatchToggle(skill.id, app, enabled);
      return;
    }
    await handleToggleApp(skill.id, app, enabled);
  };

  const handleApplyPendingChanges = () => {
    if (pendingChangeCount === 0) {
      return;
    }

    setConfirmDialog({
      isOpen: true,
      title: t("skills.applyChangesTitle", {
        count: pendingChangeCount,
        defaultValue: `Apply changes (${pendingChangeCount})`,
      }),
      message: t("skills.applyChangesMessage", {
        count: pendingChangeCount,
        defaultValue: `Apply ${pendingChangeCount} staged app changes now?`,
      }),
      onConfirm: async () => {
        setIsBatchTogglingApps(true);
        try {
          const operations = Object.entries(pendingAppChanges).flatMap(
            ([id, appChanges]) =>
              Object.entries(appChanges).map(([app, enabled]) => ({
                id,
                app: app as AppId,
                enabled: enabled as boolean,
              })),
          );

          const results = await Promise.allSettled(
            operations.map(({ id, app, enabled }) =>
              skillsApi.toggleApp(id, app, enabled),
            ),
          );

          const failedKeys = new Set<string>();
          results.forEach((result, index) => {
            if (result.status === "rejected") {
              const operation = operations[index];
              failedKeys.add(`${operation.id}:${operation.app}`);
            }
          });

          const nextPending: Record<string, Partial<Record<AppId, boolean>>> = {};
          Object.entries(pendingAppChanges).forEach(([id, appChanges]) => {
            const retainedEntries = Object.entries(appChanges).filter(([app]) =>
              failedKeys.has(`${id}:${app}`),
            );
            if (retainedEntries.length > 0) {
              nextPending[id] = Object.fromEntries(
                retainedEntries,
              ) as Partial<Record<AppId, boolean>>;
            }
          });

          const success = results.filter(
            (item) => item.status === "fulfilled",
          ).length;
          const failed = results.length - success;

          await queryClient.invalidateQueries({
            queryKey: ["skills", "installed"],
          });

          setPendingAppChanges(nextPending);
          setConfirmDialog(null);

          if (failed === 0) {
            toast.success(
              t("skills.applyChangesSuccess", {
                count: success,
                defaultValue: `Applied ${success} app changes`,
              }),
              { closeButton: true },
            );
          } else {
            const firstFailure = results.find(
              (item) => item.status === "rejected",
            ) as PromiseRejectedResult | undefined;
            toast.warning(
              t("skills.applyChangesPartial", {
                success,
                failed,
                defaultValue: `Apply finished: ${success} succeeded, ${failed} failed`,
              }),
              {
                description: firstFailure?.reason
                  ? String(firstFailure.reason)
                  : undefined,
              },
            );
          }
        } catch (error) {
          toast.error(t("common.error"), { description: String(error) });
        } finally {
          setIsBatchTogglingApps(false);
        }
      },
    });
  };

  const handleOpenImport = async () => {
    try {
      const result = await scanUnmanaged();
      if (!result.data || result.data.length === 0) {
        toast.success(t("skills.noUnmanagedFound"), { closeButton: true });
        return;
      }
      setImportDialogOpen(true);
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleImport = async (directories: string[]) => {
    try {
      const imported = await importMutation.mutateAsync(directories);
      setImportDialogOpen(false);
      toast.success(t("skills.importSuccess", { count: imported.length }), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleInstallFromZip = async () => {
    try {
      const filePath = await skillsApi.openZipFileDialog();
      if (!filePath) return;

      const currentApp: AppId = "claude";
      const installed = await installFromZipMutation.mutateAsync({
        filePath,
        currentApp,
      });

      if (installed.length === 0) {
        toast.info(t("skills.installFromZip.noSkillsFound"), {
          closeButton: true,
        });
      } else if (installed.length === 1) {
        toast.success(
          t("skills.installFromZip.successSingle", { name: installed[0].name }),
          { closeButton: true },
        );
      } else {
        toast.success(
          t("skills.installFromZip.successMultiple", {
            count: installed.length,
          }),
          { closeButton: true },
        );
      }
    } catch (error) {
      toast.error(t("skills.installFailed"), { description: String(error) });
    }
  };

  const handleUpdateSkills = async (ids: string[]) => {
    if (ids.length === 0) return;
    try {
      const result = await batchUpdateMutation.mutateAsync({
        ids,
        forceRefresh: true,
      });
      if (result.failed.length === 0) {
        toast.success(
          t("skills.updateSuccess", { count: result.installed.length }),
          { closeButton: true },
        );
      } else {
        toast.warning(
          t("skills.updatePartial", {
            success: result.installed.length,
            failed: result.failed.length,
          }),
          { description: result.failed[0]?.error },
        );
      }
      await refetchUpdates();
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleCheckUpdates = async () => {
    setIsCheckingUpdates(true);
    try {
      const statuses = await skillsApi.checkInstalledUpdates(true);
      queryClient.setQueryData(["skills", "updates", false], statuses);
      queryClient.setQueryData(["skills", "updates", true], statuses);
      const updateCount = statuses.filter(
        (item) => item.state === "update_available",
      ).length;
      if (updateCount === 0) {
        toast.success(t("skills.noUpdatesFound"), { closeButton: true });
      }
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    } finally {
      setIsCheckingUpdates(false);
    }
  };

  const handleFilterStatusChange = (
    value: "all" | "update_available" | "up_to_date" | "unknown",
  ) => {
    setFilterStatus(value);
  };

  React.useImperativeHandle(ref, () => ({
    openDiscovery: onOpenDiscovery,
    openImport: handleOpenImport,
    openInstallFromZip: handleInstallFromZip,
  }));

  return (
    <div className="px-6 flex flex-col h-[calc(100vh-8rem)] overflow-hidden">
      <AppCountBar
        totalLabel={t("skills.installed", { count: skills?.length || 0 })}
        counts={enabledCounts}
        appIds={MCP_SKILLS_APP_IDS}
      />

      <div className="flex-1 overflow-y-auto overflow-x-hidden pb-8">
        {isLoading ? (
          <div className="text-center py-12 text-muted-foreground">
            {t("skills.loading")}
          </div>
        ) : !skills || skills.length === 0 ? (
          <div className="text-center py-12">
            <div className="w-16 h-16 mx-auto mb-4 bg-muted rounded-full flex items-center justify-center">
              <Sparkles size={24} className="text-muted-foreground" />
            </div>
            <h3 className="text-lg font-medium text-foreground mb-2">
              {t("skills.noInstalled")}
            </h3>
            <p className="text-muted-foreground text-sm">
              {t("skills.noInstalledDescription")}
            </p>
          </div>
        ) : (
          <TooltipProvider delayDuration={300}>
            <>
              <div className="mb-3 flex flex-col gap-3 md:flex-row md:items-center">
                <div className="relative flex-1 min-w-0">
                  <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                  <Input
                    type="text"
                    placeholder={t("skills.searchPlaceholder")}
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    className="pl-9 pr-3"
                  />
                </div>
                <div className="w-full md:w-56 flex items-center gap-2">
                  <Select
                    value={filterStatus}
                    onValueChange={(value) =>
                      handleFilterStatusChange(
                        value as
                          | "all"
                          | "update_available"
                          | "up_to_date"
                          | "unknown",
                      )
                    }
                  >
                    <SelectTrigger className="bg-card border shadow-sm text-foreground">
                      <SelectValue
                        placeholder={t("skills.filter.placeholder")}
                      />
                    </SelectTrigger>
                    <SelectContent className="bg-card text-foreground shadow-lg min-w-[var(--radix-select-trigger-width)]">
                      <SelectItem
                        value="all"
                        className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                      >
                        {t("skills.filter.all")}
                      </SelectItem>
                      <SelectItem
                        value="update_available"
                        className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                      >
                        {t("skills.filter.updatable", {
                          defaultValue: "Updatable",
                        })}
                      </SelectItem>
                      <SelectItem
                        value="up_to_date"
                        className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                      >
                        {t("skills.filter.upToDate", {
                          defaultValue: "Up to date",
                        })}
                      </SelectItem>
                      <SelectItem
                        value="unknown"
                        className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                      >
                        {t("skills.filter.unknown", {
                          defaultValue: "Pending / Unknown",
                        })}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                </div>
              </div>

              <div className="mb-4 flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
                <div className="flex flex-wrap items-center gap-2">
                  <div className="mr-2 flex items-center gap-2 rounded-lg border border-border-default px-3 py-1.5">
                    <Label
                      htmlFor="skills-batch-mode"
                      className="text-xs font-medium"
                    >
                      {t("skills.batchMode", {
                        defaultValue: "Batch mode",
                      })}
                    </Label>
                    <Switch
                      id="skills-batch-mode"
                      checked={batchModeEnabled}
                      onCheckedChange={handleModeChange}
                      disabled={isBatchTogglingApps}
                    />
                  </div>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={handleSelectAllVisible}
                    disabled={filteredSkills.length === 0}
                  >
                    {t("skills.selectAllVisible", {
                      defaultValue: "Select All Visible",
                    })}
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={handleClearSelection}
                    disabled={selectedCount === 0}
                  >
                    {t("skills.clearSelection", {
                      defaultValue: "清空选择",
                    })}
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={handleApplyPendingChanges}
                    disabled={
                      !batchModeEnabled ||
                      pendingChangeCount === 0 ||
                      isBatchTogglingApps
                    }
                  >
                    <GitCommitHorizontal size={14} />
                    {t("skills.applyChanges", {
                      count: pendingChangeCount,
                      defaultValue: `Apply changes (${pendingChangeCount})`,
                    })}
                  </Button>
                  <Button
                    variant="destructive"
                    size="sm"
                    onClick={handleBatchUninstall}
                    disabled={
                      selectedCount === 0 ||
                      isBatchUninstalling ||
                      isBatchTogglingApps
                    }
                  >
                    <Trash2 size={14} />
                    {t("skills.bulkUninstall", {
                      defaultValue: "卸载已选（{{count}}）",
                      count: selectedCount,
                    })}
                  </Button>
                </div>

                <div className="flex flex-wrap items-center gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => void handleCheckUpdates()}
                    disabled={isCheckingUpdates || isBatchUninstalling}
                  >
                    <RefreshCw
                      size={14}
                      className={
                        isCheckingUpdates ? "mr-2 animate-spin" : "mr-2"
                      }
                    />
                    {isCheckingUpdates
                      ? t("skills.refreshing")
                      : t("skills.checkUpdates")}
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() =>
                      void handleUpdateSkills(selectedUpdatableIds)
                    }
                    disabled={
                      selectedUpdatableIds.length === 0 ||
                      batchUpdateMutation.isPending ||
                      isBatchUninstalling ||
                      isBatchTogglingApps
                    }
                  >
                    <RefreshCw
                      size={14}
                      className={
                        batchUpdateMutation.isPending
                          ? "mr-2 animate-spin"
                          : "mr-2"
                      }
                    />
                    {t("skills.updateSelected", {
                      defaultValue: "更新已选（{{count}}）",
                      count: selectedUpdatableIds.length,
                    })}
                  </Button>
                </div>
              </div>
              {batchModeEnabled && pendingChangeCount > 0 && (
                <div className="mb-4 rounded-lg border border-sky-200 bg-sky-50 px-3 py-2 text-xs text-sky-700 dark:border-sky-900/40 dark:bg-sky-950/20 dark:text-sky-300">
                  {t("skills.pendingHint", {
                    count: pendingChangeCount,
                    defaultValue: `${pendingChangeCount} staged app changes are waiting to be applied.`,
                  })}
                </div>
              )}

              {filteredSkills.length === 0 ? (
                <div className="text-center py-12 text-muted-foreground">
                  {t("skills.noResults")}
                </div>
              ) : (
                <div className="rounded-xl border border-border-default overflow-hidden">
                  {filteredSkills.map((skill, index) => (
                    <InstalledSkillListItem
                      key={skill.id}
                      skill={skill}
                      hasUpdate={updateMap.get(skill.id) === "update_available"}
                      selected={selected.has(skill.id)}
                      onToggleSelect={() => toggleSelect(skill.id)}
                      onUpdate={() => void handleUpdateSkills([skill.id])}
                      apps={getEffectiveApps(skill)}
                      pendingApps={pendingAppChanges[skill.id]}
                      onToggleApp={(app, enabled) =>
                        handleAppToggle(skill, app, enabled)
                      }
                      disableAppToggle={isBatchTogglingApps}
                      onUninstall={() => handleUninstall(skill)}
                      isLast={index === filteredSkills.length - 1}
                    />
                  ))}
                </div>
              )}
            </>
          </TooltipProvider>
        )}
      </div>

      {confirmDialog && (
        <ConfirmDialog
          isOpen={confirmDialog.isOpen}
          title={confirmDialog.title}
          message={confirmDialog.message}
          onConfirm={confirmDialog.onConfirm}
          onCancel={() => setConfirmDialog(null)}
        />
      )}

      {importDialogOpen && unmanagedSkills && (
        <ImportSkillsDialog
          skills={unmanagedSkills}
          onImport={handleImport}
          onClose={() => setImportDialogOpen(false)}
        />
      )}
    </div>
  );
});

UnifiedSkillsPanel.displayName = "UnifiedSkillsPanel";

interface InstalledSkillListItemProps {
  skill: InstalledSkill;
  apps: Record<AppId, boolean>;
  pendingApps?: Partial<Record<AppId, boolean>>;
  hasUpdate: boolean;
  selected: boolean;
  onToggleSelect: () => void;
  onUpdate: () => void;
  onToggleApp: (app: AppId, enabled: boolean) => void;
  disableAppToggle?: boolean;
  onUninstall: () => void;
  isLast?: boolean;
}

const InstalledSkillListItem: React.FC<InstalledSkillListItemProps> = ({
  skill,
  apps,
  pendingApps,
  hasUpdate,
  selected,
  onToggleSelect,
  onUpdate,
  onToggleApp,
  disableAppToggle = false,
  onUninstall,
  isLast,
}) => {
  const { t } = useTranslation();

  const openDocs = async () => {
    if (!skill.readmeUrl) return;
    try {
      await settingsApi.openExternal(skill.readmeUrl);
    } catch {
      // ignore
    }
  };

  const sourceLabel = useMemo(() => {
    if (skill.repoOwner && skill.repoName) {
      return `${skill.repoOwner}/${skill.repoName}`;
    }
    return t("skills.local");
  }, [skill.repoOwner, skill.repoName, t]);

  return (
    <ListItemRow isLast={isLast}>
      <input
        type="checkbox"
        checked={selected}
        onChange={onToggleSelect}
        aria-label={t("skills.selectSkill", {
          defaultValue: "Select skill {{name}}",
          name: skill.name,
        })}
        className="h-4 w-4 rounded border-border-default"
      />
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5">
          <span className="font-medium text-sm text-foreground truncate">
            {skill.name}
          </span>
          {skill.readmeUrl && (
            <button
              type="button"
              onClick={openDocs}
              className="text-muted-foreground/60 hover:text-foreground flex-shrink-0"
            >
              <ExternalLink size={12} />
            </button>
          )}
          <span className="text-xs text-muted-foreground/50 flex-shrink-0">
            {sourceLabel}
          </span>
          {hasUpdate && (
            <span className="text-[11px] rounded-full px-2 py-0.5 bg-amber-100 text-amber-800 dark:bg-amber-900/30 dark:text-amber-300">
              {t("skills.updateAvailable")}
            </span>
          )}
        </div>
        {skill.description && (
          <p
            className="text-xs text-muted-foreground truncate"
            title={skill.description}
          >
            {skill.description}
          </p>
        )}
      </div>

      <AppToggleGroup
        apps={apps}
        pendingApps={pendingApps}
        disabled={disableAppToggle}
        onToggle={onToggleApp}
        appIds={MCP_SKILLS_APP_IDS}
      />

      <div className="flex-shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
        {hasUpdate && (
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-7 w-7"
            onClick={onUpdate}
            title={t("skills.update")}
          >
            <RefreshCw size={14} />
          </Button>
        )}
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-7 w-7 hover:text-red-500 hover:bg-red-100 dark:hover:text-red-400 dark:hover:bg-red-500/10"
          onClick={onUninstall}
          title={t("skills.uninstall")}
        >
          <Trash2 size={14} />
        </Button>
      </div>
    </ListItemRow>
  );
};

interface ImportSkillsDialogProps {
  skills: Array<{
    directory: string;
    name: string;
    description?: string;
    foundIn: string[];
    path: string;
  }>;
  onImport: (directories: string[]) => void;
  onClose: () => void;
}

const ImportSkillsDialog: React.FC<ImportSkillsDialogProps> = ({
  skills,
  onImport,
  onClose,
}) => {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<Set<string>>(
    new Set(skills.map((s) => s.directory)),
  );

  const toggleSelect = (directory: string) => {
    const newSelected = new Set(selected);
    if (newSelected.has(directory)) {
      newSelected.delete(directory);
    } else {
      newSelected.add(directory);
    }
    setSelected(newSelected);
  };

  const handleImport = () => {
    onImport(Array.from(selected));
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-background rounded-xl p-6 max-w-lg w-full mx-4 shadow-xl max-h-[80vh] flex flex-col">
        <h2 className="text-lg font-semibold mb-2">{t("skills.import")}</h2>
        <p className="text-sm text-muted-foreground mb-4">
          {t("skills.importDescription")}
        </p>

        <div className="flex-1 overflow-y-auto space-y-2 mb-4">
          {skills.map((skill) => (
            <label
              key={skill.directory}
              className="flex items-start gap-3 p-3 rounded-lg border hover:bg-muted cursor-pointer"
            >
              <input
                type="checkbox"
                checked={selected.has(skill.directory)}
                onChange={() => toggleSelect(skill.directory)}
                className="mt-1"
              />
              <div className="flex-1 min-w-0">
                <div className="font-medium">{skill.name}</div>
                {skill.description && (
                  <div className="text-sm text-muted-foreground line-clamp-1">
                    {skill.description}
                  </div>
                )}
                <div
                  className="text-xs text-muted-foreground/50 mt-1 truncate"
                  title={skill.path}
                >
                  {skill.path}
                </div>
              </div>
            </label>
          ))}
        </div>

        <div className="flex justify-end gap-3">
          <Button variant="outline" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button onClick={handleImport} disabled={selected.size === 0}>
            {t("skills.importSelected", { count: selected.size })}
          </Button>
        </div>
      </div>
    </div>
  );
};

export default UnifiedSkillsPanel;
