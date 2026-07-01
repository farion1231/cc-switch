import React, { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Sparkles,
  Trash2,
  ExternalLink,
  RefreshCw,
  Loader2,
  X,
  GitBranch,
  Search,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  type ImportSkillSelection,
  type SkillBackupEntry,
  type SkillRepo,
  useDeleteSkillBackup,
  useInstalledSkills,
  useSkillBackups,
  useRestoreSkillBackup,
  useToggleSkillApp,
  useUninstallSkill,
  useScanUnmanagedSkills,
  useImportSkillsFromApps,
  useInstallSkillsFromZip,
  useCheckSkillUpdates,
  useUpdateSkill,
  useBatchUpdateSkillSource,
  useSkillRepos,
  type InstalledSkill,
  type SkillUpdateInfo,
} from "@/hooks/useSkills";
import type { AppId } from "@/lib/api/types";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { settingsApi, skillsApi } from "@/lib/api";
import { toast } from "sonner";
import { SKILLS_APP_IDS, APP_ICON_MAP } from "@/config/appConfig";
import { AppCountBar } from "@/components/common/AppCountBar";
import { AppToggleGroup } from "@/components/common/AppToggleGroup";
import { ListItemRow } from "@/components/common/ListItemRow";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

interface UnifiedSkillsPanelProps {
  onOpenDiscovery: () => void;
  currentApp: AppId;
}

export interface UnifiedSkillsPanelHandle {
  openDiscovery: () => void;
  openImport: () => void;
  openInstallFromZip: () => void;
  openRestoreFromBackup: () => void;
  checkUpdates: () => void;
}

function formatSkillBackupDate(unixSeconds: number): string {
  const date = new Date(unixSeconds * 1000);
  return Number.isNaN(date.getTime())
    ? String(unixSeconds)
    : date.toLocaleString();
}

const UnifiedSkillsPanel = React.forwardRef<
  UnifiedSkillsPanelHandle,
  UnifiedSkillsPanelProps
>(({ onOpenDiscovery, currentApp }, ref) => {
  const { t } = useTranslation();
  const [confirmDialog, setConfirmDialog] = useState<{
    isOpen: boolean;
    title: string;
    message: string;
    confirmText?: string;
    variant?: "destructive" | "info";
    onConfirm: () => void;
  } | null>(null);
  const [importDialogOpen, setImportDialogOpen] = useState(false);
  const [restoreDialogOpen, setRestoreDialogOpen] = useState(false);

  const { data: skills, isLoading } = useInstalledSkills();
  const {
    data: skillBackups = [],
    refetch: refetchSkillBackups,
    isFetching: isFetchingSkillBackups,
  } = useSkillBackups();
  const deleteBackupMutation = useDeleteSkillBackup();
  const toggleAppMutation = useToggleSkillApp();
  const uninstallMutation = useUninstallSkill();
  const restoreBackupMutation = useRestoreSkillBackup();
  // enabled: true —— 进入 Skill 页面时自动静默扫描一次（绿点提示来源）
  const { data: unmanagedSkills, refetch: scanUnmanaged } =
    useScanUnmanagedSkills({ enabled: true });
  const importMutation = useImportSkillsFromApps();
  const installFromZipMutation = useInstallSkillsFromZip();
  const {
    data: skillUpdates,
    refetch: checkUpdates,
    isFetching: isCheckingUpdates,
  } = useCheckSkillUpdates();
  const updateSkillMutation = useUpdateSkill();
  const batchUpdateSourceMutation = useBatchUpdateSkillSource();
  const { data: skillRepos = [] } = useSkillRepos();
  const [isUpdatingAll, setIsUpdatingAll] = useState(false);
  const [filterSource, setFilterSource] = useState<string>("all");
  const [searchQuery, setSearchQuery] = useState<string>("");
  const [selectedSkills, setSelectedSkills] = useState<Set<string>>(new Set());
  const [isBatchOperating, setIsBatchOperating] = useState(false);
  const [changeSourceDialogOpen, setChangeSourceDialogOpen] = useState(false);
  const [changeSourceRepo, setChangeSourceRepo] = useState<{
    owner: string;
    name: string;
    branch: string;
    subdirectory?: string;
  }>({ owner: "", name: "", branch: "main", subdirectory: "" });
  const [detailSkill, setDetailSkill] = useState<InstalledSkill | null>(null);

  const sourceOptions = useMemo(() => {
    if (!skills) return [];
    const sources = new Map<string, string>();
    for (const skill of skills) {
      if (skill.repoOwner && skill.repoName) {
        const key = `${skill.repoOwner}/${skill.repoName}`;
        if (!sources.has(key)) sources.set(key, key);
      } else {
        if (!sources.has("__local__"))
          sources.set("__local__", t("skills.local"));
      }
    }
    return Array.from(sources.entries()).map(([key, label]) => ({
      key,
      label,
    }));
  }, [skills, t]);

  const filteredSkills = useMemo(() => {
    if (!skills) return skills;

    const searchTerms = searchQuery
      .trim()
      .toLowerCase()
      .split(/\s+/)
      .filter((t) => t.length > 0);

    return skills.filter((skill) => {
      const sourceKey =
        skill.repoOwner && skill.repoName
          ? `${skill.repoOwner}/${skill.repoName}`
          : "__local__";

      if (filterSource !== "all" && sourceKey !== filterSource) {
        return false;
      }

      if (searchTerms.length === 0) {
        return true;
      }

      const name = skill.name.toLowerCase();
      const repo =
        `${skill.repoOwner || ""}/${skill.repoName || ""}`.toLowerCase();

      return searchTerms.some(
        (term) => name.includes(term) || repo.includes(term),
      );
    });
  }, [skills, filterSource, searchQuery]);

  const handleFilterSourceChange = useCallback((value: string) => {
    setFilterSource(value);
    setSelectedSkills(new Set());
  }, []);

  const handleSearchChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      setSearchQuery(e.target.value);
      setSelectedSkills(new Set());
    },
    [],
  );

  const toggleSelectSkill = useCallback((id: string) => {
    setSelectedSkills((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const selectAllFiltered = useCallback(() => {
    if (!filteredSkills) return;
    setSelectedSkills(new Set(filteredSkills.map((s) => s.id)));
  }, [filteredSkills]);

  const clearSelection = useCallback(() => {
    setSelectedSkills(new Set());
  }, []);

  const selectedSkillsList = useMemo(() => {
    if (!skills || selectedSkills.size === 0) return [];
    return skills.filter((s) => selectedSkills.has(s.id));
  }, [skills, selectedSkills]);

  const batchAppsState = useMemo(() => {
    const state: Record<string, boolean> = {};
    if (selectedSkillsList.length === 0) return state;
    for (const app of SKILLS_APP_IDS) {
      state[app] = selectedSkillsList.every((s) => s.apps[app]);
    }
    return state;
  }, [selectedSkillsList]);

  const updatesMap = useMemo(() => {
    const map: Record<string, SkillUpdateInfo> = {};
    if (skillUpdates) {
      for (const u of skillUpdates) {
        map[u.id] = u;
      }
    }
    return map;
  }, [skillUpdates]);

  const enabledCounts = useMemo(() => {
    const counts = {
      claude: 0,
      "claude-desktop": 0,
      codex: 0,
      gemini: 0,
      opencode: 0,
      openclaw: 0,
      hermes: 0,
    };
    if (!filteredSkills) return counts;
    filteredSkills.forEach((skill) => {
      for (const app of SKILLS_APP_IDS) {
        if (skill.apps[app]) counts[app]++;
      }
    });
    return counts;
  }, [filteredSkills]);

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
          // 构建 skillKey 用于更新 discoverable 缓存
          const installName =
            skill.directory.split(/[/\\]/).pop()?.toLowerCase() ||
            skill.directory.toLowerCase();
          const skillKey = `${installName}:${skill.repoOwner?.toLowerCase() || ""}:${skill.repoName?.toLowerCase() || ""}`;

          const result = await uninstallMutation.mutateAsync({
            id: skill.id,
            skillKey,
          });
          setConfirmDialog(null);
          toast.success(t("skills.uninstallSuccess", { name: skill.name }), {
            description: result.backupPath
              ? t("skills.backup.location", { path: result.backupPath })
              : undefined,
            closeButton: true,
          });
        } catch (error) {
          toast.error(t("common.error"), { description: String(error) });
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

  const handleImport = async (imports: ImportSkillSelection[]) => {
    try {
      const imported = await importMutation.mutateAsync(imports);
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

  const handleCheckUpdates = async () => {
    try {
      const result = await checkUpdates();
      const updates = result.data || [];
      if (updates.length === 0) {
        toast.success(t("skills.noUpdates"), { closeButton: true });
      } else {
        toast.info(t("skills.updatesFound", { count: updates.length }), {
          closeButton: true,
        });
      }
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleUpdateSkill = async (skill: InstalledSkill) => {
    try {
      const updated = await updateSkillMutation.mutateAsync(skill.id);
      toast.success(t("skills.updateSuccess", { name: updated.name }), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(t("skills.updateFailed"), { description: String(error) });
    }
  };

  const handleUpdateAll = async () => {
    if (!skillUpdates || skillUpdates.length === 0) return;
    setIsUpdatingAll(true);
    let successCount = 0;
    for (const update of skillUpdates) {
      try {
        await updateSkillMutation.mutateAsync(update.id);
        successCount++;
      } catch (error) {
        toast.error(t("skills.updateFailed"), {
          description: `${update.name}: ${String(error)}`,
        });
      }
    }
    setIsUpdatingAll(false);
    if (successCount > 0) {
      toast.success(t("skills.updateAllSuccess", { count: successCount }), {
        closeButton: true,
      });
    }
  };

  const handleBatchToggleApp = async (app: AppId, enabled: boolean) => {
    if (selectedSkillsList.length === 0) return;
    const targets = selectedSkillsList;
    const actionLabel = enabled
      ? t("skills.batch.enableAction")
      : t("skills.batch.disableAction");
    setIsBatchOperating(true);
    let successCount = 0;
    for (const skill of targets) {
      try {
        await toggleAppMutation.mutateAsync({
          id: skill.id,
          app,
          enabled,
        });
        successCount++;
      } catch {
        // continue with remaining
      }
    }
    setIsBatchOperating(false);
    if (successCount > 0) {
      toast.success(
        t("skills.batch.toggleSuccess", {
          count: successCount,
          action: actionLabel,
          app: APP_ICON_MAP[app].label,
        }),
        { closeButton: true },
      );
    }
    setSelectedSkills(new Set());
  };

  const handleBatchUninstall = () => {
    if (selectedSkillsList.length === 0) return;
    const targets = selectedSkillsList;
    setConfirmDialog({
      isOpen: true,
      title: t("skills.batch.deleteConfirmTitle"),
      message: t("skills.batch.deleteConfirmMessage", {
        count: targets.length,
      }),
      variant: "destructive",
      onConfirm: async () => {
        setIsBatchOperating(true);
        setConfirmDialog(null);
        let successCount = 0;
        for (const skill of targets) {
          const installName =
            skill.directory.split(/[/\\]/).pop()?.toLowerCase() ||
            skill.directory.toLowerCase();
          const skillKey = `${installName}:${skill.repoOwner?.toLowerCase() || ""}:${skill.repoName?.toLowerCase() || ""}`;
          try {
            await uninstallMutation.mutateAsync({
              id: skill.id,
              skillKey,
            });
            successCount++;
          } catch {
            // continue with remaining
          }
        }
        setIsBatchOperating(false);
        if (successCount > 0) {
          toast.success(
            t("skills.batch.deleteSuccess", { count: successCount }),
            { closeButton: true },
          );
        }
        setSelectedSkills(new Set());
      },
    });
  };

  const handleBatchChangeSource = async () => {
    if (selectedSkillsList.length === 0) return;
    const { owner, name, branch, subdirectory } = changeSourceRepo;
    if (!owner || !name || !branch) return;
    const count = selectedSkillsList.length;
    const repoLabel = `${owner}/${name}`;
    setIsBatchOperating(true);
    try {
      await batchUpdateSourceMutation.mutateAsync({
        ids: selectedSkillsList.map((s) => s.id),
        repoOwner: owner,
        repoName: name,
        repoBranch: branch,
        subdirectory: subdirectory?.trim() || undefined,
      });
      toast.success(
        t("skills.batch.sourceChangeSuccess", { count, repo: repoLabel }),
        { closeButton: true },
      );
      setChangeSourceDialogOpen(false);
      setChangeSourceRepo({
        owner: "",
        name: "",
        branch: "main",
        subdirectory: "",
      });
      setSelectedSkills(new Set());
      setFilterSource("all");
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    } finally {
      setIsBatchOperating(false);
    }
  };

  const handleOpenRestoreFromBackup = async () => {
    setRestoreDialogOpen(true);
    try {
      await refetchSkillBackups();
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleRestoreFromBackup = async (backupId: string) => {
    try {
      const restored = await restoreBackupMutation.mutateAsync({
        backupId,
        currentApp,
      });
      setRestoreDialogOpen(false);
      toast.success(
        t("skills.restoreFromBackup.success", { name: restored.name }),
        {
          closeButton: true,
        },
      );
    } catch (error) {
      toast.error(t("skills.restoreFromBackup.failed"), {
        description: String(error),
      });
    }
  };

  const handleDeleteBackup = (backup: SkillBackupEntry) => {
    setConfirmDialog({
      isOpen: true,
      title: t("skills.restoreFromBackup.deleteConfirmTitle"),
      message: t("skills.restoreFromBackup.deleteConfirmMessage", {
        name: backup.skill.name,
      }),
      confirmText: t("skills.restoreFromBackup.delete"),
      variant: "destructive",
      onConfirm: async () => {
        try {
          await deleteBackupMutation.mutateAsync(backup.backupId);
          await refetchSkillBackups();
          setConfirmDialog(null);
          toast.success(
            t("skills.restoreFromBackup.deleteSuccess", {
              name: backup.skill.name,
            }),
            {
              closeButton: true,
            },
          );
        } catch (error) {
          toast.error(t("skills.restoreFromBackup.deleteFailed"), {
            description: String(error),
          });
        }
      },
    });
  };

  React.useImperativeHandle(ref, () => ({
    openDiscovery: onOpenDiscovery,
    openImport: handleOpenImport,
    openInstallFromZip: handleInstallFromZip,
    openRestoreFromBackup: handleOpenRestoreFromBackup,
    checkUpdates: handleCheckUpdates,
  }));

  return (
    <div className="px-6 flex flex-col flex-1 min-h-0 overflow-hidden">
      <div className="flex items-center justify-between">
        <AppCountBar
          totalLabel={t("skills.installed", {
            count: filteredSkills?.length || 0,
          })}
          counts={enabledCounts}
          appIds={SKILLS_APP_IDS}
        />
        <div className="flex items-center gap-1.5">
          <div className="relative w-80">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground pointer-events-none" />
            <Input
              value={searchQuery}
              onChange={handleSearchChange}
              placeholder={t("skills.searchPlaceholder")}
              className="pl-9 pr-8"
            />
            {searchQuery && (
              <button
                type="button"
                onClick={() => {
                  setSearchQuery("");
                  setSelectedSkills(new Set());
                }}
                className="absolute right-2.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
              >
                <X size={14} />
              </button>
            )}
          </div>
          {sourceOptions.length > 1 && (
            <Select
              value={filterSource}
              onValueChange={handleFilterSourceChange}
            >
              <SelectTrigger className="bg-card border shadow-sm text-foreground w-auto min-w-0">
                <SelectValue
                  placeholder={t("skills.filter.allRepos")}
                  className="text-left truncate"
                />
              </SelectTrigger>
              <SelectContent className="bg-card text-foreground shadow-lg max-h-64">
                <SelectItem
                  value="all"
                  className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                >
                  {t("skills.filter.allRepos")}
                </SelectItem>
                {sourceOptions.map((opt) => (
                  <SelectItem
                    key={opt.key}
                    value={opt.key}
                    className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                  >
                    <span className="truncate block max-w-[200px]">
                      {opt.label}
                    </span>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
          <div
            className="transition-all duration-300 ease-out overflow-hidden"
            style={{
              maxWidth:
                skillUpdates && skillUpdates.length > 0 ? "200px" : "0px",
              opacity: skillUpdates && skillUpdates.length > 0 ? 1 : 0,
            }}
          >
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-7 text-xs gap-1 whitespace-nowrap"
              onClick={handleUpdateAll}
              disabled={isUpdatingAll || updateSkillMutation.isPending}
            >
              {isUpdatingAll ? (
                <Loader2 size={12} className="animate-spin" />
              ) : (
                <RefreshCw size={12} />
              )}
              {isUpdatingAll
                ? t("skills.updatingAll")
                : t("skills.updateAll", { count: skillUpdates?.length ?? 0 })}
            </Button>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-7 text-xs gap-1"
            onClick={handleCheckUpdates}
            disabled={isCheckingUpdates || !skills || skills.length === 0}
          >
            {isCheckingUpdates ? (
              <Loader2 size={12} className="animate-spin" />
            ) : (
              <RefreshCw size={12} />
            )}
            {isCheckingUpdates
              ? t("skills.checkingUpdates")
              : t("skills.checkUpdates")}
          </Button>
        </div>
      </div>

      {selectedSkills.size > 0 && (
        <div className="flex items-center gap-2 py-2 px-1 rounded-lg bg-accent/50 mb-2 flex-wrap">
          <div className="flex items-center gap-2 px-2">
            <Checkbox
              checked={
                filteredSkills &&
                filteredSkills.length > 0 &&
                filteredSkills.every((s) => selectedSkills.has(s.id))
                  ? true
                  : filteredSkills?.some((s) => selectedSkills.has(s.id))
                    ? "indeterminate"
                    : false
              }
              onCheckedChange={(checked) => {
                if (checked === true || checked === "indeterminate") {
                  selectAllFiltered();
                } else {
                  clearSelection();
                }
              }}
              aria-label={t("skills.batch.selectAll")}
            />
            <span className="text-xs font-medium text-foreground">
              {t("skills.batch.selected", { count: selectedSkills.size })}
            </span>
          </div>

          <TooltipProvider delayDuration={300}>
            <div className="flex items-center gap-1.5 px-1">
              {SKILLS_APP_IDS.map((app) => {
                const { label, icon, activeClass } = APP_ICON_MAP[app];
                const enabled = batchAppsState[app] ?? false;
                return (
                  <Tooltip key={app}>
                    <TooltipTrigger asChild>
                      <button
                        type="button"
                        onClick={() => handleBatchToggleApp(app, !enabled)}
                        disabled={isBatchOperating}
                        className={`w-7 h-7 rounded-lg flex items-center justify-center transition-all ${
                          enabled ? activeClass : "opacity-35 hover:opacity-70"
                        }`}
                      >
                        {icon}
                      </button>
                    </TooltipTrigger>
                    <TooltipContent side="bottom">
                      <p>
                        {label}
                        {enabled ? " ✓" : ""}
                      </p>
                    </TooltipContent>
                  </Tooltip>
                );
              })}
            </div>
          </TooltipProvider>

          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 text-xs gap-1"
            onClick={() => setChangeSourceDialogOpen(true)}
            disabled={isBatchOperating}
          >
            <GitBranch size={12} />
            {t("skills.batch.changeSource")}
          </Button>

          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 text-xs gap-1 text-red-600 border-red-200 hover:bg-red-50 dark:border-red-900 dark:hover:bg-red-950"
            onClick={handleBatchUninstall}
            disabled={isBatchOperating}
          >
            <Trash2 size={12} />
            {t("skills.batch.deleteSelected")}
          </Button>

          <div className="flex-1" />

          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-7 w-7 p-0"
            onClick={clearSelection}
            title={t("common.clear")}
            aria-label={t("common.clear")}
          >
            <X size={14} />
          </Button>
        </div>
      )}

      <div className="flex-1 overflow-y-auto overflow-x-hidden pb-24">
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
        ) : !filteredSkills || filteredSkills.length === 0 ? (
          <div className="text-center py-12 text-muted-foreground text-sm">
            {t("skills.noResults")}
          </div>
        ) : (
          <TooltipProvider delayDuration={300}>
            <div className="rounded-xl border border-border-default overflow-hidden">
              {filteredSkills.map((skill, index) => (
                <InstalledSkillListItem
                  key={skill.id}
                  skill={skill}
                  hasUpdate={!!updatesMap[skill.id]}
                  isUpdating={
                    updateSkillMutation.isPending &&
                    updateSkillMutation.variables === skill.id
                  }
                  isSelected={selectedSkills.has(skill.id)}
                  onSelect={() => toggleSelectSkill(skill.id)}
                  onToggleApp={handleToggleApp}
                  onUninstall={() => handleUninstall(skill)}
                  onUpdate={() => handleUpdateSkill(skill)}
                  onViewDetail={() => setDetailSkill(skill)}
                  isLast={index === filteredSkills.length - 1}
                />
              ))}
            </div>
          </TooltipProvider>
        )}
      </div>

      {confirmDialog && (
        <ConfirmDialog
          isOpen={confirmDialog.isOpen}
          title={confirmDialog.title}
          message={confirmDialog.message}
          confirmText={confirmDialog.confirmText}
          variant={confirmDialog.variant}
          zIndex="top"
          onConfirm={confirmDialog.onConfirm}
          onCancel={() => setConfirmDialog(null)}
        />
      )}

      {importDialogOpen && unmanagedSkills && (
        <ImportSkillsDialog
          skills={unmanagedSkills}
          isImporting={importMutation.isPending}
          onImport={handleImport}
          onClose={() => setImportDialogOpen(false)}
        />
      )}

      <RestoreSkillsDialog
        backups={skillBackups}
        isDeleting={deleteBackupMutation.isPending}
        isLoading={isFetchingSkillBackups}
        onDelete={handleDeleteBackup}
        isRestoring={restoreBackupMutation.isPending}
        onRestore={handleRestoreFromBackup}
        onClose={() => setRestoreDialogOpen(false)}
        open={restoreDialogOpen}
      />

      <ChangeSourceDialog
        open={changeSourceDialogOpen}
        onClose={() => setChangeSourceDialogOpen(false)}
        repos={skillRepos}
        repo={changeSourceRepo}
        onRepoChange={setChangeSourceRepo}
        selectedCount={selectedSkillsList.length}
        isSubmitting={batchUpdateSourceMutation.isPending || isBatchOperating}
        onConfirm={handleBatchChangeSource}
      />

      {detailSkill && (
        <SkillDetailDialog
          skill={detailSkill}
          onClose={() => setDetailSkill(null)}
        />
      )}
    </div>
  );
});

UnifiedSkillsPanel.displayName = "UnifiedSkillsPanel";

interface InstalledSkillListItemProps {
  skill: InstalledSkill;
  hasUpdate?: boolean;
  isUpdating?: boolean;
  isSelected: boolean;
  onSelect: () => void;
  onToggleApp: (id: string, app: AppId, enabled: boolean) => void;
  onUninstall: () => void;
  onUpdate?: () => void;
  onViewDetail: () => void;
  isLast?: boolean;
}

const InstalledSkillListItem: React.FC<InstalledSkillListItemProps> = ({
  skill,
  hasUpdate,
  isUpdating,
  isSelected,
  onSelect,
  onToggleApp,
  onUninstall,
  onUpdate,
  onViewDetail,
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
      <Checkbox
        checked={isSelected}
        onCheckedChange={onSelect}
        aria-label={skill.name}
        className="flex-shrink-0"
      />
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5">
          <button
            type="button"
            onClick={onViewDetail}
            className="font-medium text-sm text-foreground truncate hover:text-primary hover:underline cursor-pointer text-left"
            title={skill.name}
          >
            {skill.name}
          </button>
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
            <Badge
              variant="outline"
              className="shrink-0 text-[10px] px-1.5 py-0 h-4 border-amber-500 text-amber-600 dark:text-amber-400"
            >
              {t("skills.updateAvailable")}
            </Badge>
          )}
        </div>
        {skill.description && (
          <button
            type="button"
            onClick={onViewDetail}
            className="text-xs text-muted-foreground truncate text-left w-full hover:text-foreground cursor-pointer"
          >
            {skill.description}
          </button>
        )}
      </div>

      <AppToggleGroup
        apps={skill.apps}
        onToggle={(app, enabled) => onToggleApp(skill.id, app, enabled)}
        appIds={SKILLS_APP_IDS}
      />

      <div
        className="flex-shrink-0 flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity"
        style={hasUpdate ? { opacity: 1 } : undefined}
      >
        {hasUpdate && onUpdate && (
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-7 w-7 hover:text-blue-500 hover:bg-blue-100 dark:hover:text-blue-400 dark:hover:bg-blue-500/10"
            onClick={onUpdate}
            disabled={isUpdating}
            title={t("skills.update")}
          >
            {isUpdating ? (
              <Loader2 size={14} className="animate-spin" />
            ) : (
              <RefreshCw size={14} />
            )}
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
  isImporting: boolean;
  onImport: (imports: ImportSkillSelection[]) => void;
  onClose: () => void;
}

interface RestoreSkillsDialogProps {
  backups: SkillBackupEntry[];
  isDeleting: boolean;
  isLoading: boolean;
  isRestoring: boolean;
  onDelete: (backup: SkillBackupEntry) => void;
  onRestore: (backupId: string) => void;
  onClose: () => void;
  open: boolean;
}

const RestoreSkillsDialog: React.FC<RestoreSkillsDialogProps> = ({
  backups,
  isDeleting,
  isLoading,
  isRestoring,
  onDelete,
  onRestore,
  onClose,
  open,
}) => {
  const { t } = useTranslation();

  return (
    <Dialog open={open} onOpenChange={(nextOpen) => !nextOpen && onClose()}>
      <DialogContent
        className="max-w-2xl max-h-[85vh] flex flex-col"
        zIndex="alert"
      >
        <DialogHeader>
          <DialogTitle>{t("skills.restoreFromBackup.title")}</DialogTitle>
          <DialogDescription>
            {t("skills.restoreFromBackup.description")}
          </DialogDescription>
        </DialogHeader>

        <div className="flex-1 overflow-y-auto px-6 py-4">
          {isLoading ? (
            <div className="py-10 text-center text-sm text-muted-foreground">
              {t("common.loading")}
            </div>
          ) : backups.length === 0 ? (
            <div className="py-10 text-center text-sm text-muted-foreground">
              {t("skills.restoreFromBackup.empty")}
            </div>
          ) : (
            <div className="space-y-3">
              {backups.map((backup) => (
                <div
                  key={backup.backupId}
                  className="rounded-xl border border-border-default bg-background/70 p-4 shadow-sm"
                >
                  <div className="flex items-start justify-between gap-4">
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <div className="font-medium text-sm text-foreground">
                          {backup.skill.name}
                        </div>
                        <div className="rounded-md bg-muted px-2 py-0.5 text-[11px] text-muted-foreground">
                          {backup.skill.directory}
                        </div>
                      </div>
                      {backup.skill.description && (
                        <div className="mt-2 text-sm text-muted-foreground">
                          {backup.skill.description}
                        </div>
                      )}
                      <div className="mt-3 space-y-1.5 text-xs text-muted-foreground">
                        <div>
                          {t("skills.restoreFromBackup.createdAt")}:{" "}
                          {formatSkillBackupDate(backup.createdAt)}
                        </div>
                        <div className="break-all" title={backup.backupPath}>
                          {t("skills.restoreFromBackup.path")}:{" "}
                          {backup.backupPath}
                        </div>
                      </div>
                    </div>

                    <div className="flex flex-col gap-2 sm:min-w-28">
                      <Button
                        type="button"
                        variant="outline"
                        onClick={() => onRestore(backup.backupId)}
                        disabled={isRestoring || isDeleting}
                      >
                        {isRestoring
                          ? t("skills.restoreFromBackup.restoring")
                          : t("skills.restoreFromBackup.restore")}
                      </Button>
                      <Button
                        type="button"
                        variant="destructive"
                        onClick={() => onDelete(backup)}
                        disabled={isRestoring || isDeleting}
                      >
                        {isDeleting
                          ? t("skills.restoreFromBackup.deleting")
                          : t("skills.restoreFromBackup.delete")}
                      </Button>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        <DialogFooter>
          <Button type="button" variant="outline" onClick={onClose}>
            {t("common.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};

const ImportSkillsDialog: React.FC<ImportSkillsDialogProps> = ({
  skills,
  isImporting,
  onImport,
  onClose,
}) => {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<Set<string>>(
    new Set(skills.map((s) => s.directory)),
  );
  const [selectedApps, setSelectedApps] = useState<
    Record<string, ImportSkillSelection["apps"]>
  >(() =>
    Object.fromEntries(
      skills.map((skill) => [
        skill.directory,
        {
          claude: skill.foundIn.includes("claude"),
          codex: skill.foundIn.includes("codex"),
          gemini: skill.foundIn.includes("gemini"),
          opencode: skill.foundIn.includes("opencode"),
          openclaw: false,
          hermes: skill.foundIn.includes("hermes"),
        },
      ]),
    ),
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
    onImport(
      Array.from(selected).map((directory) => ({
        directory,
        apps: selectedApps[directory] ?? {
          claude: false,
          codex: false,
          gemini: false,
          opencode: false,
          openclaw: false,
          hermes: false,
        },
      })),
    );
  };

  return (
    <TooltipProvider delayDuration={300}>
      <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
        <div className="bg-background rounded-xl p-6 max-w-lg w-full mx-4 shadow-xl max-h-[80vh] flex flex-col">
          <h2 className="text-lg font-semibold mb-2">{t("skills.import")}</h2>
          <p className="text-sm text-muted-foreground mb-4">
            {t("skills.importDescription")}
          </p>

          <div className="flex-1 overflow-y-auto space-y-2 mb-4">
            {skills.map((skill) => (
              <div
                key={skill.directory}
                className="flex items-start gap-3 p-3 rounded-lg border hover:bg-muted"
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
                  <div className="mt-2">
                    <AppToggleGroup
                      apps={
                        selectedApps[skill.directory] ?? {
                          claude: false,
                          codex: false,
                          gemini: false,
                          opencode: false,
                          openclaw: false,
                          hermes: false,
                        }
                      }
                      onToggle={(app, enabled) => {
                        setSelectedApps((prev) => ({
                          ...prev,
                          [skill.directory]: {
                            ...(prev[skill.directory] ?? {
                              claude: false,
                              codex: false,
                              gemini: false,
                              opencode: false,
                              openclaw: false,
                              hermes: false,
                            }),
                            [app]: enabled,
                          },
                        }));
                      }}
                      appIds={SKILLS_APP_IDS}
                    />
                  </div>
                  <div
                    className="text-xs text-muted-foreground/50 mt-1 truncate"
                    title={skill.path}
                  >
                    {skill.path}
                  </div>
                </div>
              </div>
            ))}
          </div>

          <div className="flex justify-end gap-3">
            <Button variant="outline" onClick={onClose} disabled={isImporting}>
              {t("common.cancel")}
            </Button>
            <Button
              onClick={handleImport}
              disabled={selected.size === 0 || isImporting}
            >
              {t("skills.importSelected", { count: selected.size })}
            </Button>
          </div>
        </div>
      </div>
    </TooltipProvider>
  );
};

interface SkillDetailDialogProps {
  skill: InstalledSkill;
  onClose: () => void;
}

const SkillDetailDialog: React.FC<SkillDetailDialogProps> = ({
  skill,
  onClose,
}) => {
  const { t } = useTranslation();

  const sourceLabel =
    skill.repoOwner && skill.repoName
      ? `${skill.repoOwner}/${skill.repoName}`
      : t("skills.detail.local");

  const enabledApps = SKILLS_APP_IDS.filter((app) => skill.apps[app]);

  const formatDate = (ts: number) => {
    if (!ts) return "—";
    return new Date(ts * 1000).toLocaleString();
  };

  const openGitHub = async () => {
    if (!skill.readmeUrl) return;
    try {
      await settingsApi.openExternal(skill.readmeUrl);
    } catch {
      // ignore
    }
  };

  return (
    <Dialog open onOpenChange={(nextOpen) => !nextOpen && onClose()}>
      <DialogContent className="max-w-lg" zIndex="alert">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            {skill.name}
            {skill.readmeUrl && (
              <button
                type="button"
                onClick={openGitHub}
                className="text-muted-foreground/60 hover:text-foreground"
                title={t("skills.detail.viewOnGitHub")}
              >
                <ExternalLink size={14} />
              </button>
            )}
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-4 px-6 py-4">
          <div>
            <div className="text-xs font-medium text-muted-foreground mb-1">
              {t("skills.detail.description")}
            </div>
            <p className="text-sm text-foreground whitespace-pre-wrap">
              {skill.description || t("skills.detail.noDescription")}
            </p>
          </div>

          <div className="grid grid-cols-2 gap-x-6 gap-y-3 text-sm">
            <div>
              <div className="text-xs text-muted-foreground">
                {t("skills.detail.source")}
              </div>
              <div className="font-medium">{sourceLabel}</div>
              {skill.repoBranch && (
                <div className="text-xs text-muted-foreground/70 mt-0.5">
                  {skill.repoBranch}
                </div>
              )}
            </div>
            <div>
              <div className="text-xs text-muted-foreground">
                {t("skills.detail.directory")}
              </div>
              <div className="font-medium break-all">{skill.directory}</div>
            </div>
            <div>
              <div className="text-xs text-muted-foreground">
                {t("skills.detail.installedAt")}
              </div>
              <div>{formatDate(skill.installedAt)}</div>
            </div>
            <div>
              <div className="text-xs text-muted-foreground">
                {t("skills.detail.updatedAt")}
              </div>
              <div>{formatDate(skill.updatedAt)}</div>
            </div>
          </div>

          {enabledApps.length > 0 && (
            <div>
              <div className="text-xs font-medium text-muted-foreground mb-2">
                {t("skills.detail.enabledApps")}
              </div>
              <div className="flex flex-wrap gap-1.5">
                {enabledApps.map((app) => (
                  <Badge
                    key={app}
                    variant="secondary"
                    className="text-xs gap-1"
                  >
                    {APP_ICON_MAP[app].icon}
                    {APP_ICON_MAP[app].label}
                  </Badge>
                ))}
              </div>
            </div>
          )}
        </div>

        <DialogFooter>
          <Button type="button" variant="outline" onClick={onClose}>
            {t("common.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};

interface ChangeSourceDialogProps {
  open: boolean;
  onClose: () => void;
  repos: SkillRepo[];
  repo: { owner: string; name: string; branch: string; subdirectory?: string };
  onRepoChange: (repo: {
    owner: string;
    name: string;
    branch: string;
    subdirectory?: string;
  }) => void;
  selectedCount: number;
  isSubmitting: boolean;
  onConfirm: () => void;
}

const ChangeSourceDialog: React.FC<ChangeSourceDialogProps> = ({
  open,
  onClose,
  repos,
  repo,
  onRepoChange,
  selectedCount,
  isSubmitting,
  onConfirm,
}) => {
  const { t } = useTranslation();
  const enabledRepos = repos.filter((r) => r.enabled);

  const selectRepo = (r: SkillRepo) => {
    onRepoChange({
      owner: r.owner,
      name: r.name,
      branch: r.branch,
      subdirectory: repo.subdirectory,
    });
  };

  const isCurrentSelected = (r: SkillRepo) =>
    r.owner === repo.owner && r.name === repo.name && r.branch === repo.branch;

  const canConfirm =
    repo.owner.trim() && repo.name.trim() && repo.branch.trim();

  return (
    <Dialog open={open} onOpenChange={(nextOpen) => !nextOpen && onClose()}>
      <DialogContent className="max-w-md" zIndex="alert">
        <DialogHeader>
          <DialogTitle>{t("skills.batch.changeSourceTitle")}</DialogTitle>
          <DialogDescription>
            {t("skills.batch.changeSourceDescription", {
              count: selectedCount,
            })}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 px-6 py-4">
          {enabledRepos.length > 0 && (
            <div className="space-y-2">
              <div className="text-xs font-medium text-muted-foreground">
                {t("skills.batch.selectRepo")}
              </div>
              <div className="space-y-1 max-h-40 overflow-y-auto">
                {enabledRepos.map((r) => (
                  <button
                    key={`${r.owner}/${r.name}:${r.branch}`}
                    type="button"
                    onClick={() => selectRepo(r)}
                    className={`w-full text-left px-3 py-2 rounded-lg text-xs border transition-colors ${
                      isCurrentSelected(r)
                        ? "border-primary bg-primary/10 text-foreground"
                        : "border-border-default hover:bg-muted text-muted-foreground"
                    }`}
                  >
                    <div className="font-medium">
                      {r.owner}/{r.name}
                    </div>
                    <div className="text-muted-foreground/70 mt-0.5">
                      {r.branch}
                    </div>
                  </button>
                ))}
              </div>
            </div>
          )}

          <div className="space-y-2">
            <div className="text-xs font-medium text-muted-foreground">
              {t("skills.batch.customRepo")}
            </div>
            <div className="grid grid-cols-2 gap-2">
              <Input
                placeholder={t("skills.batch.repoOwner")}
                value={repo.owner}
                onChange={(e) =>
                  onRepoChange({ ...repo, owner: e.target.value })
                }
                className="h-8 text-xs"
              />
              <Input
                placeholder={t("skills.batch.repoName")}
                value={repo.name}
                onChange={(e) =>
                  onRepoChange({ ...repo, name: e.target.value })
                }
                className="h-8 text-xs"
              />
            </div>
            <Input
              placeholder={t("skills.batch.repoBranch")}
              value={repo.branch}
              onChange={(e) =>
                onRepoChange({ ...repo, branch: e.target.value })
              }
              className="h-8 text-xs"
            />
            <Input
              placeholder={t("skills.batch.subdirectory")}
              value={repo.subdirectory || ""}
              onChange={(e) =>
                onRepoChange({ ...repo, subdirectory: e.target.value })
              }
              className="h-8 text-xs"
            />
          </div>
        </div>

        <DialogFooter>
          <Button type="button" variant="outline" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button
            type="button"
            onClick={onConfirm}
            disabled={!canConfirm || isSubmitting}
          >
            {isSubmitting ? (
              <Loader2 size={14} className="animate-spin mr-1.5" />
            ) : null}
            {t("common.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};

export default UnifiedSkillsPanel;
