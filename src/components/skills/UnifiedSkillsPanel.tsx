import React, { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Sparkles,
  Trash2,
  ExternalLink,
  RefreshCw,
  Loader2,
  Search,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  type ImportSkillSelection,
  type SkillBackupEntry,
  useDeleteSkillBackup,
  useInstalledSkills,
  useSkillBackups,
  useRestoreSkillBackup,
  useBulkToggleSkillApp,
  useBulkUninstallSkill,
  useToggleSkillApp,
  useUninstallSkill,
  useScanUnmanagedSkills,
  useImportSkillsFromApps,
  useInstallSkillsFromZip,
  useCheckSkillUpdates,
  useUpdateSkill,
  type InstalledSkill,
  type SkillUpdateInfo,
} from "@/hooks/useSkills";
import type { AppId } from "@/lib/api/types";
import { cn } from "@/lib/utils";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { settingsApi, skillsApi } from "@/lib/api";
import { toast } from "sonner";
import { SKILLS_APP_IDS } from "@/config/appConfig";
import { usePagedList } from "@/hooks/usePagedList";
import { AppCountBar } from "@/components/common/AppCountBar";
import { AppToggleGroup } from "@/components/common/AppToggleGroup";
import { ListItemRow } from "@/components/common/ListItemRow";
import { ManagementListSearch } from "@/components/common/ManagementListSearch";
import {
  ManagementFilterChips,
  type ManagementFilterOption,
} from "@/components/common/ManagementFilterChips";
import { ManagementBulkBar } from "@/components/common/ManagementBulkBar";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

const IMPORT_SKILLS_APP_IDS = SKILLS_APP_IDS.filter((app) => app !== "pi");

/**
 * 一次渲染多少行。列表里可能装着成百上千条 skill，全量挂载会把首屏时间
 * 全部花在 DOM 上；分页之后每一页都是常数成本。
 */
const SKILLS_PAGE_SIZE = 60;

/**
 * 确认框点名隐藏选中项的名字数量上限：选择集可以无限积累，确认框不能。
 * 超出的部分折叠成省略号，反正下面还有真实卸载对象的完整列表兜底。
 */
const BULK_UNINSTALL_HIDDEN_NAMES_LIMIT = 8;

type SkillAppFilter = AppId;
type SkillSourceFilter = "repo" | "local";
type SkillUpdateFilter = "updated";

/** ManagementFilterChips 的选项类型别名（该组件是 app 无关的）。 */
type SourceFilterOption = ManagementFilterOption;
type UpdateFilterOption = ManagementFilterOption;

interface UnifiedSkillsPanelProps {
  onOpenDiscovery: () => void;
  currentApp: AppId;
  onInteractionBlockedChange?: (blocked: boolean) => void;
  onNavigationBlockedChange?: (blocked: boolean) => void;
  onCheckUpdatesStateChange?: (state: SkillsCheckUpdatesState) => void;
}

export interface SkillsCheckUpdatesState {
  isChecking: boolean;
  hasSkills: boolean;
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
>((props, ref) => {
  const {
    onOpenDiscovery,
    currentApp,
    onInteractionBlockedChange,
    onNavigationBlockedChange,
    onCheckUpdatesStateChange,
  } = props;
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
  const [searchQuery, setSearchQuery] = useState("");
  const [filterApps, setFilterApps] = useState<SkillAppFilter[]>([]);
  const [filterAppMode, setFilterAppMode] = useState<"any" | "all">("any");
  const [filterSources, setFilterSources] = useState<SkillSourceFilter[]>([]);
  const [filterUpdates, setFilterUpdates] = useState<SkillUpdateFilter[]>([]);
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [writePending, setWritePending] = useState(false);
  const writeLockRef = React.useRef(false);
  const checkUpdatesLockRef = React.useRef(false);

  const { data: skills, isLoading } = useInstalledSkills();
  const {
    data: skillBackups = [],
    refetch: refetchSkillBackups,
    isFetching: isFetchingSkillBackups,
  } = useSkillBackups();
  const deleteBackupMutation = useDeleteSkillBackup();
  const toggleAppMutation = useToggleSkillApp();
  const bulkToggleAppMutation = useBulkToggleSkillApp();
  const bulkUninstallMutation = useBulkUninstallSkill();
  const uninstallMutation = useUninstallSkill();
  const restoreBackupMutation = useRestoreSkillBackup();
  // enabled: true — 进入 Skill 页面时自动静默扫描一次（绿点提示来源）
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
  const [isUpdatingAll, setIsUpdatingAll] = useState(false);
  const visibleSkillAppIds =
    currentApp === "pi" ? SKILLS_APP_IDS : IMPORT_SKILLS_APP_IDS;

  const mutationPending =
    deleteBackupMutation.isPending ||
    toggleAppMutation.isPending ||
    bulkToggleAppMutation.isPending ||
    bulkUninstallMutation.isPending ||
    uninstallMutation.isPending ||
    restoreBackupMutation.isPending ||
    importMutation.isPending ||
    installFromZipMutation.isPending ||
    updateSkillMutation.isPending ||
    isUpdatingAll;
  const dialogOpen =
    importDialogOpen || restoreDialogOpen || confirmDialog !== null;
  const navigationBlocked = writePending || mutationPending || dialogOpen;
  const interactionBlocked = navigationBlocked || isCheckingUpdates;

  React.useEffect(() => {
    onInteractionBlockedChange?.(interactionBlocked);
  }, [interactionBlocked, onInteractionBlockedChange]);

  React.useEffect(() => {
    onNavigationBlockedChange?.(navigationBlocked);
  }, [navigationBlocked, onNavigationBlockedChange]);

  React.useEffect(
    () => () => {
      onInteractionBlockedChange?.(false);
      onNavigationBlockedChange?.(false);
    },
    [onInteractionBlockedChange, onNavigationBlockedChange],
  );

  const hasSkills = (skills?.length ?? 0) > 0;

  React.useEffect(() => {
    onCheckUpdatesStateChange?.({
      isChecking: isCheckingUpdates,
      hasSkills,
    });
  }, [hasSkills, isCheckingUpdates, onCheckUpdatesStateChange]);

  React.useEffect(
    () => () =>
      onCheckUpdatesStateChange?.({ isChecking: false, hasSkills: false }),
    [onCheckUpdatesStateChange],
  );

  const beginWrite = (allowOpenDialog = false) => {
    if (
      checkUpdatesLockRef.current ||
      isCheckingUpdates ||
      writeLockRef.current ||
      mutationPending ||
      (!allowOpenDialog && dialogOpen)
    ) {
      return false;
    }
    writeLockRef.current = true;
    setWritePending(true);
    return true;
  };

  const endWrite = () => {
    writeLockRef.current = false;
    setWritePending(false);
  };

  const applicableSkillUpdates = useMemo(() => {
    const installedIds = new Set((skills ?? []).map((skill) => skill.id));
    return (skillUpdates ?? []).filter((update) => installedIds.has(update.id));
  }, [skillUpdates, skills]);

  const updatesMap = useMemo(() => {
    const map: Record<string, SkillUpdateInfo> = {};
    for (const update of applicableSkillUpdates) {
      map[update.id] = update;
    }
    return map;
  }, [applicableSkillUpdates]);

  const enabledCounts = useMemo(() => {
    const counts = {
      claude: 0,
      "claude-desktop": 0,
      codex: 0,
      gemini: 0,
      grokbuild: 0,
      opencode: 0,
      openclaw: 0,
      hermes: 0,
      pi: 0,
      mcode: 0,
    };
    if (!skills) return counts;
    skills.forEach((skill) => {
      for (const app of SKILLS_APP_IDS) {
        if (skill.apps[app]) {
          counts[app]++;
        }
      }
    });
    return counts;
  }, [skills]);

  // 已安装 skill 的可筛选维度（app / 来源 / 有更新）与其计数。
  const appFilterOptions = useMemo(() => {
    if (!skills) return [];
    return visibleSkillAppIds.map((app) => ({
      value: app,
      label: t("skills.apps." + app, app),
      count: skills.filter((skill) => skill.apps[app]).length,
    }));
  }, [skills, visibleSkillAppIds, t]);

  const sourceFilterOptions = useMemo<SourceFilterOption[]>(() => {
    if (!skills) return [];
    const repoCount = skills.filter(
      (skill) => skill.repoOwner && skill.repoName,
    ).length;
    return [
      { value: "repo", label: t("skills.manage.sourceRepo"), count: repoCount },
      {
        value: "local",
        label: t("skills.manage.sourceLocal"),
        count: skills.length - repoCount,
      },
    ];
  }, [skills, t]);

  const updateFilterOptions = useMemo<UpdateFilterOption[]>(() => {
    if (!applicableSkillUpdates.length) return [];
    return [
      {
        value: "updated",
        label: t("skills.manage.updatedAvailable"),
        count: applicableSkillUpdates.length,
      },
    ];
  }, [applicableSkillUpdates, t]);

  const hasActiveFilters =
    filterApps.length > 0 ||
    filterSources.length > 0 ||
    filterUpdates.length > 0 ||
    searchQuery.trim() !== "";

  const filteredSkills = useMemo(() => {
    if (!skills) return [];

    const query = searchQuery.trim().toLocaleLowerCase();
    const updatesMatch = filterUpdates.includes("updated");
    const sourcesMatch = filterSources.map(String);

    return skills.filter((skill) => {
      if (
        filterApps.length > 0 &&
        !(filterAppMode === "any"
          ? filterApps.some((app) => skill.apps[app])
          : filterApps.every((app) => skill.apps[app]))
      ) {
        return false;
      }

      if (filterSources.length > 0) {
        const isRepo = Boolean(skill.repoOwner && skill.repoName);
        if (!sourcesMatch.includes(isRepo ? "repo" : "local")) return false;
      }

      if (updatesMatch && !updatesMap[skill.id]) return false;

      if (!query) return true;

      const searchableValues = [
        skill.name,
        skill.id,
        skill.description,
        skill.directory,
        skill.repoOwner,
        skill.repoName,
        skill.repoOwner && skill.repoName
          ? `${skill.repoOwner}/${skill.repoName}`
          : undefined,
      ];

      return searchableValues.some((value) =>
        value?.toLocaleLowerCase().includes(query),
      );
    });
  }, [
    skills,
    searchQuery,
    filterApps,
    filterAppMode,
    filterSources,
    filterUpdates,
    updatesMap,
  ]);

  // 筛选签名：任一维度变化都让分页回到第一页。
  const activeFilterKey = useMemo(
    () =>
      [
        searchQuery.trim().toLocaleLowerCase(),
        filterAppMode,
        [...filterApps].sort().join(","),
        [...filterSources].sort().join(","),
        [...filterUpdates].sort().join(","),
      ].join("|"),
    [searchQuery, filterAppMode, filterApps, filterSources, filterUpdates],
  );

  // 筛选变化时分页回到第一页（filterKey 变了就是结果集换了）。
  const {
    page: pagedSkills,
    total: filteredTotal,
    hasMore,
    showMore: loadMoreSkills,
  } = usePagedList(filteredSkills, SKILLS_PAGE_SIZE, activeFilterKey);

  // 选择集按 skill 本体保留，跨筛选不清空：常见工作流是"筛一处勾几个、换
  // 筛选再勾几个、最后一次卸完"。收敛到筛选结果会直接砍掉这个流程。
  //
  // 代价是选择集里可能有当前看不见的行，所以 UI 必须把三件事分开讲清楚：
  //   1. 列表刷新时丢掉已不存在的 id（skill 被卸掉了）；
  //   2. 工具栏分子用「可见集合中的已选数」，分母用可见总数——描述屏幕；
  //   3. 隐藏的选中单独计数并明示，全选判定只看当前筛选项，确认框列出真实
  //      卸载对象（含筛掉的），而不是只报一个总数。
  // 此前那版把选择集收敛到筛选结果，计数/全选/卸载三者虽然一致了，却把跨
  // 筛选多选这一整类用法弄没了。
  const knownSkillIds = useMemo(
    () => new Set((skills ?? []).map((skill) => skill.id)),
    [skills],
  );

  // id → skill 本体：给隐藏选中项点名用（确认框要报名字，不是 id）。
  const skillById = useMemo(
    () => new Map((skills ?? []).map((skill) => [skill.id, skill])),
    [skills],
  );

  useEffect(() => {
    setSelectedIds((previous) => {
      const next = previous.filter((id) => knownSkillIds.has(id));
      return next.length === previous.length ? previous : next;
    });
  }, [knownSkillIds]);

  const visibleSkillIds = useMemo(
    () => filteredSkills.map((skill) => skill.id),
    [filteredSkills],
  );

  // 筛选结果可能上千行、选择集也可能积累到很大，includes 式成员判定是
  // O(n*m)，一律换 Set。
  const visibleSkillIdSet = useMemo(
    () => new Set(visibleSkillIds),
    [visibleSkillIds],
  );

  /** 当前可见行里被选中的数量（工具栏的分子，描述屏幕）。 */
  const visibleSelectedCount = useMemo(
    () => selectedIds.filter((id) => visibleSkillIdSet.has(id)).length,
    [selectedIds, visibleSkillIdSet],
  );

  /**
   * 被当前筛选藏起来的选中项本体，不只是计数：确认框必须点出真实卸载对象，
   * 「只报一个总数」等于让用户盲删看不见的东西。
   *
   * 用真实 skill 对象算，而不是 selectedIds.length - visibleSelectedCount：
   * 那样会把「正在被裁剪的 id」（已在 selectedIds 里但 skill 已消失）算成
   * 「隐藏」，确认框于是多报一个马上就不存在的卸载对象。
   */
  const hiddenSelectedSkills = useMemo<InstalledSkill[]>(
    () =>
      selectedIds
        .filter((id) => !visibleSkillIdSet.has(id))
        .map((id) => skillById.get(id))
        .filter((skill): skill is InstalledSkill => Boolean(skill)),
    [selectedIds, visibleSkillIdSet, skillById],
  );

  /** 行级 checkbox 的受控判定也走 Set：每页 60 行 × 选择集大小，别做乘法。 */
  const selectedIdSet = useMemo(() => new Set(selectedIds), [selectedIds]);

  /**
   * 分子（可见选中数）只描述屏幕，而清空/卸载按钮作用于整个选择集。只要
   * 有隐藏选中，按钮文案必须自报范围，否则「3 / 12」旁边的「卸载（5）」
   * 会让用户以为只有屏幕上那 3 个受影响。
   */
  const hiddenSelectedCount = hiddenSelectedSkills.length;

  const toggleSelected = useCallback((id: string) => {
    setSelectedIds((previous) =>
      previous.includes(id)
        ? previous.filter((item) => item !== id)
        : [...previous, id],
    );
  }, []);

  const clearSelection = useCallback(() => setSelectedIds([]), []);

  const selectAllFiltered = useCallback(() => {
    // 只并入当前筛选项，不动跨筛选积累的其他选择。
    setSelectedIds((previous) => {
      const existing = new Set(previous);
      return [
        ...previous,
        ...visibleSkillIds.filter((id) => !existing.has(id)),
      ];
    });
  }, [visibleSkillIds]);

  const resetFilters = useCallback(() => {
    setFilterApps([]);
    setFilterSources([]);
    setFilterUpdates([]);
    setSearchQuery("");
  }, []);

  const pendingApp = bulkToggleAppMutation.isPending
    ? bulkToggleAppMutation.variables?.app
    : toggleAppMutation.isPending
      ? toggleAppMutation.variables?.app
      : null;

  const handleToggleApp = async (id: string, app: AppId, enabled: boolean) => {
    if (!beginWrite()) return;

    try {
      await toggleAppMutation.mutateAsync({ id, app, enabled });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    } finally {
      endWrite();
    }
  };

  const handleToggleAll = async (app: AppId, enabled: boolean) => {
    if (!skills || !beginWrite()) return;

    const ids = skills
      .filter((skill) => Boolean(skill.apps[app]) !== enabled)
      .map((skill) => skill.id);
    if (ids.length === 0) {
      endWrite();
      return;
    }

    try {
      const result = await bulkToggleAppMutation.mutateAsync({
        ids,
        app,
        enabled,
      });
      if (result.failed.length > 0) {
        toast.error(
          t("common.bulkToggleFailed", { count: result.failed.length }),
          { description: String(result.failed[0].error) },
        );
      }
    } catch (error) {
      toast.error(t("common.bulkToggleFailed", { count: ids.length }), {
        description: String(error),
      });
    } finally {
      endWrite();
    }
  };

  const handleUninstall = (skill: InstalledSkill) => {
    if (
      checkUpdatesLockRef.current ||
      writeLockRef.current ||
      interactionBlocked
    ) {
      return;
    }
    setConfirmDialog({
      isOpen: true,
      title: t("skills.uninstall"),
      message: t("skills.uninstallConfirm", { name: skill.name }),
      onConfirm: async () => {
        if (!beginWrite(true)) return;
        try {
          const result = await uninstallMutation.mutateAsync(skill.id);
          setConfirmDialog(null);
          const piCleanupIncomplete =
            result.piCleanupIncomplete || Boolean(result.preservedPiPath);
          const toastOptions = {
            description: result.preservedPiPath
              ? t("skills.uninstallPiPreserved", {
                  path: result.preservedPiPath,
                })
              : result.piCleanupIncomplete
                ? t("skills.uninstallPiCleanupIncomplete")
                : result.backupPath
                  ? t("skills.backup.location", { path: result.backupPath })
                  : undefined,
            closeButton: true,
          };
          if (piCleanupIncomplete) {
            toast.warning(
              t("skills.uninstallSuccess", { name: skill.name }),
              toastOptions,
            );
          } else {
            toast.success(
              t("skills.uninstallSuccess", { name: skill.name }),
              toastOptions,
            );
          }
        } catch (error) {
          toast.error(t("common.error"), { description: String(error) });
        } finally {
          endWrite();
        }
      },
    });
  };

  const handleBulkUninstall = () => {
    if (selectedIds.length === 0) return;
    if (
      checkUpdatesLockRef.current ||
      writeLockRef.current ||
      interactionBlocked
    ) {
      return;
    }
    // 被筛选藏起来的选中项必须逐个点名：屏幕上看不到的东西只报一个总数，
    // 用户无从核对就要删。可见的选中项屏幕上本来就有，不必重复。
    const hiddenNames = hiddenSelectedSkills.map((skill) => skill.name);
    const listedNames = hiddenNames.slice(0, BULK_UNINSTALL_HIDDEN_NAMES_LIMIT);
    const unlistedCount = hiddenNames.length - listedNames.length;
    // 名字行不进 i18n 插值：封顶逻辑与语言无关，而且名字是数据不是文案。
    const hiddenNotice =
      hiddenNames.length > 0
        ? [
            t("skills.manage.bulkUninstallHidden", {
              count: hiddenNames.length,
            }),
            listedNames.join(", ") + (unlistedCount > 0 ? "…" : ""),
          ].join("\n")
        : null;
    // 截断时必须点破「没列全」，否则省略号读起来像「隐藏集合已知」，40 个
    // 藏起来的也能悄悄溜过确认。
    const unlistedNotice =
      unlistedCount > 0
        ? t("skills.manage.hiddenSelectedUnlisted", { count: unlistedCount })
        : null;
    setConfirmDialog({
      isOpen: true,
      title: t("skills.uninstall"),
      message: [
        t("skills.manage.bulkUninstallConfirm", {
          count: selectedIds.length,
        }),
        hiddenNotice,
        unlistedNotice,
      ]
        .filter((part): part is string => part !== null)
        .join("\n"),
      confirmText: t("skills.manage.bulkUninstall", {
        count: selectedIds.length,
      }),
      variant: "destructive",
      onConfirm: async () => {
        if (!beginWrite(true)) return;
        const ids = [...selectedIds];
        try {
          const result = await bulkUninstallMutation.mutateAsync(ids);
          setConfirmDialog(null);
          // 后端在 Pi 目录无法解析、同名副本归属不明或删除失败时，仍会删掉
          // 管理记录，但通过 Ok 响应里的 piCleanupIncomplete / preservedPiPath
          // 告知还有文件没清。这类"记录已删、文件残留"必须让用户知道，否则
          // 界面显示全部成功而残留目录无人处理。
          const cleanup = result.succeeded.filter(
            (entry) =>
              entry.result.piCleanupIncomplete || entry.result.preservedPiPath,
          );

          if (result.failed.length > 0) {
            toast.warning(
              t("skills.manage.bulkUninstallPartial", {
                succeeded: result.succeeded.length,
                failed: result.failed.length,
              }),
              {
                description: String(result.failed[0].error),
                closeButton: true,
              },
            );
          } else if (cleanup.length > 0) {
            toast.warning(
              t("skills.manage.bulkUninstallSuccess", {
                count: result.succeeded.length,
              }),
              {
                description: cleanup[0].result.preservedPiPath
                  ? t("skills.uninstallPiPreserved", {
                      path: cleanup[0].result.preservedPiPath,
                    })
                  : t("skills.uninstallPiCleanupIncomplete"),
                closeButton: true,
              },
            );
          } else {
            toast.success(
              t("skills.manage.bulkUninstallSuccess", {
                count: result.succeeded.length,
              }),
              { closeButton: true },
            );
          }

          // 多于一条残留时补一句总量，避免只报第一条造成"只有一条有问题"的误解。
          if (cleanup.length > 1) {
            toast.info(
              t("skills.manage.bulkUninstallCleanupMore", {
                count: cleanup.length - 1,
              }),
              { closeButton: true },
            );
          }

          clearSelection();
        } catch (error) {
          toast.error(t("skills.manage.bulkUninstallFailed"), {
            description: String(error),
          });
        } finally {
          endWrite();
        }
      },
    });
  };

  const handleOpenImport = async () => {
    if (!beginWrite()) return;
    try {
      const result = await scanUnmanaged();
      if (!result.data || result.data.length === 0) {
        toast.success(t("skills.noUnmanagedFound"), { closeButton: true });
        return;
      }
      setImportDialogOpen(true);
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    } finally {
      endWrite();
    }
  };

  const handleImport = async (imports: ImportSkillSelection[]) => {
    if (!beginWrite(true)) return;
    try {
      const imported = await importMutation.mutateAsync(imports);
      setImportDialogOpen(false);
      toast.success(t("skills.importSuccess", { count: imported.length }), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    } finally {
      endWrite();
    }
  };

  const handleInstallFromZip = async () => {
    if (!beginWrite()) return;
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
          t("skills.installFromZip.successSingle", {
            name: installed[0].name,
          }),
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
    } finally {
      endWrite();
    }
  };

  const handleCheckUpdates = async () => {
    if (
      checkUpdatesLockRef.current ||
      writeLockRef.current ||
      interactionBlocked
    ) {
      return;
    }
    checkUpdatesLockRef.current = true;
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
    } finally {
      checkUpdatesLockRef.current = false;
    }
  };

  const handleUpdateSkill = async (skill: InstalledSkill) => {
    if (!beginWrite()) return;
    try {
      const updated = await updateSkillMutation.mutateAsync(skill.id);
      toast.success(t("skills.updateSuccess", { name: updated.name }), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(t("skills.updateFailed"), { description: String(error) });
    } finally {
      endWrite();
    }
  };

  const handleUpdateAll = async () => {
    if (applicableSkillUpdates.length === 0 || !beginWrite()) {
      return;
    }
    setIsUpdatingAll(true);
    let successCount = 0;
    try {
      for (const update of applicableSkillUpdates) {
        try {
          await updateSkillMutation.mutateAsync(update.id);
          successCount++;
        } catch (error) {
          toast.error(t("skills.updateFailed"), {
            description: `${update.name}: ${String(error)}`,
          });
        }
      }
    } finally {
      setIsUpdatingAll(false);
      endWrite();
    }
    if (successCount > 0) {
      toast.success(t("skills.updateAllSuccess", { count: successCount }), {
        closeButton: true,
      });
    }
  };

  const handleOpenRestoreFromBackup = async () => {
    if (!beginWrite()) return;
    setRestoreDialogOpen(true);
    try {
      await refetchSkillBackups({ throwOnError: true });
    } catch (error) {
      setRestoreDialogOpen(false);
      toast.error(t("common.error"), { description: String(error) });
    } finally {
      endWrite();
    }
  };

  const handleRestoreFromBackup = async (backupId: string) => {
    if (!beginWrite(true)) return;
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
    } finally {
      endWrite();
    }
  };

  const handleDeleteBackup = (backup: SkillBackupEntry) => {
    if (checkUpdatesLockRef.current || writeLockRef.current) return;
    setConfirmDialog({
      isOpen: true,
      title: t("skills.restoreFromBackup.deleteConfirmTitle"),
      message: t("skills.restoreFromBackup.deleteConfirmMessage", {
        name: backup.skill.name,
      }),
      confirmText: t("skills.restoreFromBackup.delete"),
      variant: "destructive",
      onConfirm: async () => {
        if (!beginWrite(true)) return;
        try {
          let deleteSucceeded = false;
          let deleteError: unknown;
          try {
            await deleteBackupMutation.mutateAsync(backup.backupId);
            deleteSucceeded = true;
          } catch (error) {
            deleteError = error;
          }

          // The backups query is disabled by default, so invalidation alone
          // does not fetch authoritative data. Explicitly refresh after both
          // success and failure (remove_dir_all may have made partial progress).
          let refreshedBackups: SkillBackupEntry[] | undefined;
          try {
            const result = await refetchSkillBackups({ throwOnError: true });
            refreshedBackups = result.data;
          } catch (error) {
            // A refresh failure must not turn a completed deletion into a false
            // "delete failed" report, or replace the original deletion error.
            console.error(
              "Failed to refresh Skill backups after deletion:",
              error,
            );
          }

          if (!deleteSucceeded) {
            // remove_dir_all may finish removing the directory but still
            // report an error. If the authoritative refresh confirms that the
            // item is gone, close the now-stale confirmation dialog.
            if (
              refreshedBackups &&
              !refreshedBackups.some(
                (entry) => entry.backupId === backup.backupId,
              )
            ) {
              setConfirmDialog(null);
            }
            toast.error(t("skills.restoreFromBackup.deleteFailed"), {
              description: String(deleteError),
            });
          } else {
            setConfirmDialog(null);
            toast.success(
              t("skills.restoreFromBackup.deleteSuccess", {
                name: backup.skill.name,
              }),
              {
                closeButton: true,
              },
            );
          }
        } finally {
          endWrite();
        }
      },
    });
  };

  React.useImperativeHandle(ref, () => ({
    openDiscovery: () => {
      if (
        !checkUpdatesLockRef.current &&
        !writeLockRef.current &&
        !interactionBlocked
      ) {
        onOpenDiscovery();
      }
    },
    openImport: handleOpenImport,
    openInstallFromZip: handleInstallFromZip,
    openRestoreFromBackup: handleOpenRestoreFromBackup,
    checkUpdates: handleCheckUpdates,
  }));

  return (
    <div className="px-6 flex flex-col flex-1 min-h-0 overflow-hidden">
      <div className="flex items-center justify-between gap-2">
        <div className="min-w-0 flex-1">
          <AppCountBar
            totalLabel={t("skills.installed", { count: skills?.length || 0 })}
            counts={enabledCounts}
            appIds={visibleSkillAppIds}
            totalCount={skills?.length ?? 0}
            onToggleAll={handleToggleAll}
            pendingApp={pendingApp}
            disabled={interactionBlocked}
          />
        </div>
        <div
          className="mb-4 overflow-hidden transition-all duration-300 ease-out"
          style={{
            maxWidth: applicableSkillUpdates.length > 0 ? "200px" : "0px",
            opacity: applicableSkillUpdates.length > 0 ? 1 : 0,
          }}
        >
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 text-xs gap-1 whitespace-nowrap disabled:opacity-100"
            onClick={handleUpdateAll}
            disabled={interactionBlocked}
          >
            {isUpdatingAll ? (
              <Loader2 size={12} className="animate-spin" />
            ) : (
              <RefreshCw size={12} />
            )}
            {isUpdatingAll
              ? t("skills.updatingAll")
              : t("skills.updateAll", {
                  count: applicableSkillUpdates.length,
                })}
          </Button>
        </div>
      </div>

      <ManagementListSearch
        value={searchQuery}
        onValueChange={setSearchQuery}
        placeholder={t("skills.installedSearchPlaceholder")}
        ariaLabel={t("skills.installedSearchAriaLabel")}
        clearLabel={t("common.clear")}
      />

      {/* 筛选芯片行：app / 来源 / 有更新 */}
      {skills && skills.length > 0 && (
        <div className="mb-3 space-y-2">
          <ManagementFilterChips
            label={t("skills.manage.filterApp")}
            options={appFilterOptions}
            selected={filterApps}
            onSelectionChange={(next) =>
              setFilterApps(next as SkillAppFilter[])
            }
            disabled={interactionBlocked}
          />
          {filterApps.length > 1 && (
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              <span>{t("skills.manage.filterAppMode")}</span>
              {(["any", "all"] as const).map((mode) => (
                <button
                  key={mode}
                  type="button"
                  aria-pressed={filterAppMode === mode}
                  onClick={() => setFilterAppMode(mode)}
                  className={cn(
                    "rounded-full border px-2 py-0.5 text-[11px] transition-colors",
                    filterAppMode === mode
                      ? "border-primary/40 bg-primary/10 text-foreground"
                      : "border-border-default hover:bg-muted",
                  )}
                >
                  {t(
                    mode === "any"
                      ? "skills.manage.filterAppModeAny"
                      : "skills.manage.filterAppModeAll",
                  )}
                </button>
              ))}
            </div>
          )}
          <div className="flex flex-wrap items-center gap-2">
            <ManagementFilterChips
              label={t("skills.manage.filterSource")}
              options={sourceFilterOptions}
              selected={filterSources}
              onSelectionChange={(next) =>
                setFilterSources(next as SkillSourceFilter[])
              }
              disabled={interactionBlocked}
            />
            {updateFilterOptions.length > 0 && (
              <ManagementFilterChips
                label={t("skills.manage.filterUpdated")}
                options={updateFilterOptions}
                selected={filterUpdates}
                onSelectionChange={(next) =>
                  setFilterUpdates(next as SkillUpdateFilter[])
                }
                disabled={interactionBlocked}
              />
            )}
            {hasActiveFilters && (
              <div className="flex items-center gap-2 pl-1 text-xs text-muted-foreground">
                <span>
                  {t("skills.manage.visibleCount", {
                    visible: pagedSkills.length,
                    total: filteredTotal,
                  })}
                </span>
                <button
                  type="button"
                  className="underline hover:text-foreground"
                  onClick={resetFilters}
                >
                  {t("common.reset")}
                </button>
              </div>
            )}
          </div>
        </div>
      )}

      <ManagementBulkBar
        selectedCount={visibleSelectedCount}
        totalCount={filteredTotal}
        onSelectAll={selectAllFiltered}
        onClear={clearSelection}
        clearLabel={
          hiddenSelectedCount > 0
            ? t("skills.manage.clearSelectionAll", {
                count: hiddenSelectedCount,
              })
            : t("skills.manage.clearSelection")
        }
        selectAllLabel={t("skills.manage.selectAll")}
        toolbarLabel={t("skills.manage.bulkActions")}
        disabled={interactionBlocked}
      >
        {hiddenSelectedCount > 0 && (
          <span className="text-xs text-muted-foreground">
            {t("skills.manage.hiddenSelected", { count: hiddenSelectedCount })}
          </span>
        )}
        <Button
          type="button"
          variant="destructive"
          size="sm"
          className="h-7 text-xs"
          disabled={interactionBlocked}
          onClick={handleBulkUninstall}
        >
          <Trash2 size={12} className="mr-1" />
          {hiddenSelectedCount > 0
            ? t("skills.manage.bulkUninstallAll", {
                count: selectedIds.length,
                hidden: hiddenSelectedCount,
              })
            : t("skills.manage.bulkUninstall", { count: selectedIds.length })}
        </Button>
      </ManagementBulkBar>

      <ScrollArea className="-mr-3 flex-1 min-h-0" type="auto">
        <div className="pb-24 pr-3">
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
          ) : filteredSkills.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-12 text-center text-muted-foreground">
              <Search className="mb-4 h-10 w-10 opacity-40" />
              <p className="text-sm">{t("skills.noInstalledSearchResults")}</p>
            </div>
          ) : (
            <TooltipProvider delayDuration={300}>
              <div className="rounded-xl border border-border-default overflow-hidden">
                {pagedSkills.map((skill, index) => (
                  <InstalledSkillListItem
                    key={skill.id}
                    skill={skill}
                    selected={selectedIdSet.has(skill.id)}
                    onSelectedChange={() => toggleSelected(skill.id)}
                    hasUpdate={!!updatesMap[skill.id]}
                    isUpdating={
                      updateSkillMutation.isPending &&
                      updateSkillMutation.variables === skill.id
                    }
                    actionsDisabled={interactionBlocked}
                    appIds={visibleSkillAppIds}
                    onToggleApp={handleToggleApp}
                    onUninstall={() => handleUninstall(skill)}
                    onUpdate={() => handleUpdateSkill(skill)}
                    isLast={index === pagedSkills.length - 1 && !hasMore}
                  />
                ))}
              </div>
              {hasMore && (
                <div className="mt-3 flex justify-center">
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    disabled={interactionBlocked}
                    onClick={loadMoreSkills}
                  >
                    {t("skills.manage.loadMore")}
                  </Button>
                </div>
              )}
            </TooltipProvider>
          )}
        </div>
      </ScrollArea>

      {confirmDialog && (
        <ConfirmDialog
          isOpen={confirmDialog.isOpen}
          title={confirmDialog.title}
          message={confirmDialog.message}
          confirmText={confirmDialog.confirmText}
          variant={confirmDialog.variant}
          zIndex="top"
          pending={writePending}
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
    </div>
  );
});

UnifiedSkillsPanel.displayName = "UnifiedSkillsPanel";

interface InstalledSkillListItemProps {
  skill: InstalledSkill;
  appIds: AppId[];
  hasUpdate?: boolean;
  isUpdating?: boolean;
  actionsDisabled?: boolean;
  selected?: boolean;
  onSelectedChange?: () => void;
  onToggleApp: (id: string, app: AppId, enabled: boolean) => void;
  onUninstall: () => void;
  onUpdate?: () => void;
  isLast?: boolean;
}

/**
 * 已安装 skill 的行。
 *
 * 用 React.memo 包住：父组件每约 2 秒轮询一次代理状态，搜索框里每个按键
 * 也会让整表重筛——不记忆化的话几百行都要在每次按键时重建。回调 props 由
 * 父组件用 useCallback 稳定住，因此只有自身相关字段变化时才会重渲染。
 */
const InstalledSkillListItem: React.FC<InstalledSkillListItemProps> =
  React.memo(
    ({
      skill,
      appIds,
      hasUpdate,
      isUpdating,
      actionsDisabled,
      selected = false,
      onSelectedChange,
      onToggleApp,
      onUninstall,
      onUpdate,
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
            className="h-3.5 w-3.5 flex-shrink-0 accent-primary"
            checked={selected}
            disabled={actionsDisabled}
            onChange={() => onSelectedChange?.()}
            aria-label={t("skills.manage.selectSkill", { name: skill.name })}
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
                <Badge
                  variant="outline"
                  className="shrink-0 text-[10px] px-1.5 py-0 h-4 border-amber-500 text-amber-600 dark:text-amber-400"
                >
                  {t("skills.updateAvailable")}
                </Badge>
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
            apps={skill.apps}
            onToggle={(app, enabled) => onToggleApp(skill.id, app, enabled)}
            appIds={appIds}
            disabled={actionsDisabled}
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
                className={cn(
                  "h-7 w-7 hover:text-blue-500 hover:bg-blue-100 dark:hover:text-blue-400 dark:hover:bg-blue-500/10",
                  actionsDisabled && !isUpdating && "disabled:opacity-100",
                )}
                onClick={onUpdate}
                disabled={actionsDisabled || isUpdating}
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
              className="h-7 w-7 hover:text-red-500 hover:bg-red-100 disabled:opacity-100 dark:hover:text-red-400 dark:hover:bg-red-500/10"
              onClick={onUninstall}
              disabled={actionsDisabled}
              title={t("skills.uninstall")}
            >
              <Trash2 size={14} />
            </Button>
          </div>
        </ListItemRow>
      );
    },
  );
InstalledSkillListItem.displayName = "InstalledSkillListItem";

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
  const actionPending = isRestoring || isDeleting;

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => !nextOpen && !actionPending && onClose()}
    >
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
          <Button
            type="button"
            variant="outline"
            onClick={onClose}
            disabled={actionPending}
          >
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
          grokbuild: skill.foundIn.includes("grokbuild"),
          opencode: skill.foundIn.includes("opencode"),
          openclaw: false,
          hermes: skill.foundIn.includes("hermes"),
          pi: false,
          mcode: skill.foundIn.includes("mcode"),
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
          grokbuild: false,
          opencode: false,
          openclaw: false,
          hermes: false,
          pi: false,
          mcode: false,
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
                  aria-label={skill.name}
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
                          grokbuild: false,
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
                              grokbuild: false,
                              opencode: false,
                              openclaw: false,
                              hermes: false,
                            }),
                            [app]: enabled,
                          },
                        }));
                      }}
                      appIds={IMPORT_SKILLS_APP_IDS}
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

export default UnifiedSkillsPanel;
