import { createPortal } from "react-dom";
import React, { useEffect, useMemo, useRef, useState } from "react";
import { useDelayedFlag } from "@/hooks/useDelayedFlag";
import { useTranslation } from "react-i18next";
import {
  ArrowUpCircle,
  Check,
  ChevronDown,
  Loader2,
  MoreHorizontal,
  Plus,
  RefreshCw,
} from "lucide-react";
import { toast } from "@/lib/toast";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { Notice, NoticeSlot } from "@/components/ui/notice";
import { PageTabs } from "@/components/ui/page-tabs";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { HoverTip } from "@/components/ui/hover-tip";
import { Checkbox } from "@/components/ui/checkbox";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { SkillsIcon } from "@/components/BrandIcons";
import {
  type ImportSkillSelection,
  type InstalledSkill,
  type SkillBackupEntry,
  type SkillUpdateInfo,
  useBulkToggleSkillApp,
  useCheckSkillUpdates,
  useDeleteSkillBackup,
  useDiscoverableSkills,
  useImportSkillsFromApps,
  useInstallSkillsFromZip,
  useInstalledSkills,
  useRestoreSkillBackup,
  useScanUnmanagedSkills,
  useSkillBackups,
  useSkillRepos,
  useToggleSkillApp,
  useUninstallSkill,
  useUpdateSkill,
} from "@/hooks/useSkills";
import type { AppId } from "@/lib/api/types";
import { SKILLS_APP_IDS } from "@/config/appConfig";
import { settingsApi, skillsApi } from "@/lib/api";
import { copyText } from "@/lib/clipboard";
import { extractErrorMessage } from "@/utils/errorUtils";
import { cn } from "@/lib/utils";
import {
  MatrixCell,
  MatrixColumnHeader,
  MatrixColumnHighlight,
  MatrixSearch,
  NeutralBadge,
  resolveBulkScope,
  showUndoToast,
} from "@/components/mcp/AppMatrix";
import { V7ConfirmDialog } from "@/components/mcp/formBits";
import { useVisibleAppIds } from "@/components/mcp/useVisibleAppIds";
import { SkillsPage, useSkillRepoActions } from "./SkillsPage";
import { RepoManagerPanel } from "./RepoManagerPanel";
import { SkillImportDialog } from "./SkillImportDialog";
import { SkillRestoreDialog } from "./SkillRestoreDialog";
import { SkillsStorageSheet } from "./SkillsStorageSheet";
import { useSkillInstallTargets } from "./useSkillInstallTargets";
import { describeRepoFailures } from "./repoFailures";
import type { ZipSkippedSkill } from "@/lib/api/skills";

const BACKUP_DIR = "~/.cc-switch/skill-backups";

type SkillsView = "installed" | "discover";
type StatusFilter = "all" | "updates" | "none" | `app:${AppId}`;

interface WriteFailure {
  desired: boolean;
  error: string;
}
const failKey = (id: string, app: AppId) => `${id}\u0000${app}`;

type ConfirmState =
  | { kind: "uninstall"; ids: string[] }
  | { kind: "updateAll" }
  | { kind: "deleteBackup"; backup: SkillBackupEntry };

interface UnifiedSkillsPanelProps {
  /** 从旧的 skillsDiscovery 视图进来时直接打开「发现」段 */
  initialView?: SkillsView;
  onInteractionBlockedChange?: (blocked: boolean) => void;
  onNavigationBlockedChange?: (blocked: boolean) => void;
}

function isEditableTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  if (target instanceof HTMLTextAreaElement) return true;
  if (target instanceof HTMLInputElement) {
    return (
      target.type !== "checkbox" &&
      target.type !== "radio" &&
      target.value !== ""
    );
  }
  return false;
}

/**
 * Skills 全局页（v7）：「已安装」矩阵 +「发现」合成一页。
 * Pi 列常驻（应用页里隐藏 Pi 时才不显示）；Pi 的状态来自后端对 ~/.pi/agent/skills 的目录检查，
 * get_installed_skills 返回前会用这个检查结果覆盖 apps.pi，所以这里显示的不是数据库里的开关。
 */
const UnifiedSkillsPanel: React.FC<UnifiedSkillsPanelProps> = ({
  initialView = "installed",
  onInteractionBlockedChange,
  onNavigationBlockedChange,
}) => {
  const { t, i18n } = useTranslation();
  const appIds = useVisibleAppIds(SKILLS_APP_IDS);
  const { targets, setTargets, installTo } = useSkillInstallTargets(appIds);

  const [view, setView] = useState<SkillsView>(initialView);
  // 「发现」段第一次打开后就一直挂着（隐藏而不卸载），来回切不再重新加载仓库列表
  const [discoverMounted, setDiscoverMounted] = useState(
    initialView === "discover",
  );
  useEffect(() => {
    if (view === "discover") setDiscoverMounted(true);
  }, [view]);
  const [discoverQuery, setDiscoverQuery] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [sourceFilter, setSourceFilter] = useState("all");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [fails, setFails] = useState<Record<string, WriteFailure>>({});
  const [confirm, setConfirm] = useState<ConfirmState | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [restoreOpen, setRestoreOpen] = useState(false);
  const [storageOpen, setStorageOpen] = useState(false);
  const [repoManagerOpen, setRepoManagerOpen] = useState(false);
  const [dismissedUnmanaged, setDismissedUnmanaged] = useState<number | null>(
    null,
  );
  /** 关掉的是哪一次检查的「仓库没读到」横幅（按检查时间记） */
  const [dismissedRepoFailAt, setDismissedRepoFailAt] = useState<number | null>(
    null,
  );
  const [highlightId, setHighlightId] = useState<string | null>(null);
  const [writePending, setWritePending] = useState(false);
  const [isUpdatingMany, setIsUpdatingMany] = useState(false);
  // 更新进度：确认框点完就关，进度显示在搜索框旁的按钮上
  const [updateProgress, setUpdateProgress] = useState<{
    done: number;
    total: number;
  } | null>(null);
  const writeLockRef = useRef(false);
  const checkUpdatesLockRef = useRef(false);

  const {
    data: skills,
    isLoading,
    isError,
    error: loadError,
    refetch,
  } = useInstalledSkills();
  const {
    data: skillBackups = [],
    refetch: refetchSkillBackups,
    isFetching: isFetchingSkillBackups,
  } = useSkillBackups();
  const deleteBackupMutation = useDeleteSkillBackup();
  const toggleAppMutation = useToggleSkillApp();
  const bulkToggleAppMutation = useBulkToggleSkillApp();
  const uninstallMutation = useUninstallSkill();
  const restoreBackupMutation = useRestoreSkillBackup();
  // 进入页面时静默扫描一次「本机已有但未管理」的 Skill
  const { data: unmanagedSkills, refetch: scanUnmanaged } =
    useScanUnmanagedSkills({ enabled: true });
  const importMutation = useImportSkillsFromApps();
  const installFromZipMutation = useInstallSkillsFromZip();
  const {
    data: updateCheck,
    refetch: checkUpdates,
    isFetching: isCheckingUpdates,
    dataUpdatedAt: updatesCheckedAt,
  } = useCheckSkillUpdates();
  const skillUpdates = updateCheck?.updates;
  const updateRepoFailures = updateCheck?.failures ?? [];
  const updateSkillMutation = useUpdateSkill();

  const mutationPending =
    deleteBackupMutation.isPending ||
    toggleAppMutation.isPending ||
    bulkToggleAppMutation.isPending ||
    uninstallMutation.isPending ||
    restoreBackupMutation.isPending ||
    importMutation.isPending ||
    installFromZipMutation.isPending ||
    updateSkillMutation.isPending ||
    isUpdatingMany;
  const dialogOpen =
    importOpen || restoreOpen || confirm !== null || repoManagerOpen;
  const navigationBlocked = writePending || mutationPending || dialogOpen;
  const interactionBlocked = navigationBlocked || isCheckingUpdates;
  // 外观上的禁用晚 300ms 才出现：点一个格子写得很快时不让整页按钮闪一下变灰。
  // 写入本身仍由写锁（writeLockRef / interactionBlocked）拦着。
  const controlsDisabled = useDelayedFlag(interactionBlocked);

  useEffect(() => {
    onInteractionBlockedChange?.(interactionBlocked);
  }, [interactionBlocked, onInteractionBlockedChange]);

  useEffect(() => {
    onNavigationBlockedChange?.(navigationBlocked);
  }, [navigationBlocked, onNavigationBlockedChange]);

  useEffect(
    () => () => {
      onInteractionBlockedChange?.(false);
      onNavigationBlockedChange?.(false);
    },
    [onInteractionBlockedChange, onNavigationBlockedChange],
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

  const listSeparator = t("mcpPage.listSeparator");
  const names = (apps: AppId[]) =>
    apps.map((app) => APP_DISPLAY_NAME[app]).join(listSeparator);
  const noun = t("skillsPage.noun");
  const installedSkills = skills ?? [];

  // ─── 更新 ───────────────────────────────────────────────────────────
  const applicableSkillUpdates = useMemo(() => {
    const installedIds = new Set(installedSkills.map((skill) => skill.id));
    return (skillUpdates ?? []).filter((update) => installedIds.has(update.id));
  }, [skillUpdates, installedSkills]);

  const updatesMap = useMemo(() => {
    const map: Record<string, SkillUpdateInfo> = {};
    for (const update of applicableSkillUpdates) map[update.id] = update;
    return map;
  }, [applicableSkillUpdates]);

  const lastCheckedText = useMemo(() => {
    if (!updatesCheckedAt) return "";
    const minutes = Math.floor((Date.now() - updatesCheckedAt) / 60_000);
    if (minutes < 1) return t("skillsPage.justNow");
    try {
      const rtf = new Intl.RelativeTimeFormat(i18n.language, {
        numeric: "auto",
      });
      return minutes < 60
        ? rtf.format(-minutes, "minute")
        : rtf.format(-Math.floor(minutes / 60), "hour");
    } catch {
      return "";
    }
  }, [updatesCheckedAt, i18n.language, t]);

  // ─── 来源 / 筛选 ────────────────────────────────────────────────────
  const sourceKey = (skill: InstalledSkill) =>
    skill.repoOwner && skill.repoName
      ? `repo:${skill.repoOwner}/${skill.repoName}`
      : "local";

  const sourceLabel = (key: string) =>
    key === "local" ? t("skillsPage.source.local") : key.slice(5);

  const sourceOptions = useMemo(() => {
    const counts = new Map<string, number>();
    for (const skill of installedSkills) {
      const key = sourceKey(skill);
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
    return [...counts.entries()].sort(([a], [b]) => {
      if (a === "local") return 1;
      if (b === "local") return -1;
      return a.localeCompare(b);
    });
  }, [installedSkills]);

  const enabledCount = (skill: InstalledSkill) =>
    SKILLS_APP_IDS.filter((app) => skill.apps[app]).length;

  const normalizedQuery = searchQuery.trim().toLocaleLowerCase();
  const filteredSkills = useMemo(() => {
    return installedSkills.filter((skill) => {
      if (statusFilter === "updates" && !updatesMap[skill.id]) return false;
      if (statusFilter === "none" && enabledCount(skill) > 0) return false;
      if (statusFilter.startsWith("app:")) {
        const app = statusFilter.slice(4) as AppId;
        if (!skill.apps[app]) return false;
      }
      if (sourceFilter !== "all" && sourceKey(skill) !== sourceFilter) {
        return false;
      }
      if (!normalizedQuery) return true;
      return [
        skill.name,
        skill.id,
        skill.description,
        skill.directory,
        skill.repoOwner,
        skill.repoName,
        skill.repoOwner && skill.repoName
          ? `${skill.repoOwner}/${skill.repoName}`
          : undefined,
      ].some((value) => value?.toLocaleLowerCase().includes(normalizedQuery));
    });
  }, [
    installedSkills,
    statusFilter,
    sourceFilter,
    normalizedQuery,
    updatesMap,
  ]);

  // 「可更新」筛选下把最后一个也更新完：回到全部，免得停在空表上
  const nApplicableUpdates = applicableSkillUpdates.length;
  const prevUpdatesRef = useRef(nApplicableUpdates);
  useEffect(() => {
    if (
      prevUpdatesRef.current > 0 &&
      nApplicableUpdates === 0 &&
      statusFilter === "updates"
    ) {
      setStatusFilter("all");
    }
    prevUpdatesRef.current = nApplicableUpdates;
  }, [nApplicableUpdates, statusFilter]);

  const filtersActive = statusFilter !== "all" || sourceFilter !== "all";
  const scope = resolveBulkScope(
    installedSkills,
    filteredSkills,
    normalizedQuery ? "search" : filtersActive ? "filter" : null,
  );

  // 已经不在列表里的勾选自动丢掉
  useEffect(() => {
    if (selected.size === 0) return;
    const ids = new Set(installedSkills.map((skill) => skill.id));
    const next = new Set([...selected].filter((id) => ids.has(id)));
    if (next.size !== selected.size) setSelected(next);
  }, [installedSkills, selected]);

  // 从「发现」跳回来时定位到那一行
  useEffect(() => {
    if (!highlightId || view !== "installed") return;
    const row = document.getElementById(`sk-row-${highlightId}`);
    row?.scrollIntoView?.({ block: "center" });
    row?.focus?.({ preventScroll: true });
  }, [highlightId, view]);

  // 在「发现」段按 Escape 回到「已安装」
  useEffect(() => {
    if (view !== "discover") return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      if (document.body.style.overflow === "hidden") return;
      if (isEditableTarget(event.target)) return;
      event.preventDefault();
      setView("installed");
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [view]);

  // ─── 写入与失败记录 ────────────────────────────────────────────────
  const recordResult = (
    id: string,
    app: AppId,
    desired: boolean,
    error?: unknown,
  ) =>
    setFails((prev) => {
      const next = { ...prev };
      const key = failKey(id, app);
      if (error === undefined) delete next[key];
      else next[key] = { desired, error: extractErrorMessage(error) };
      return next;
    });

  const recordEnableFailures = (
    failures: Array<{ id: string; app: AppId; error: unknown }>,
  ) => {
    for (const failure of failures) {
      recordResult(failure.id, failure.app, true, failure.error);
    }
  };

  const writeOne = async (id: string, app: AppId, enabled: boolean) => {
    if (!beginWrite()) return;
    try {
      await toggleAppMutation.mutateAsync({ id, app, enabled });
      recordResult(id, app, enabled);
    } catch (error) {
      recordResult(id, app, enabled, error);
    } finally {
      endWrite();
    }
  };

  const writeMany = async (ids: string[], app: AppId, enabled: boolean) => {
    if (ids.length === 0) return { succeeded: [] as string[], failed: 0 };
    try {
      const result = await bulkToggleAppMutation.mutateAsync({
        ids,
        app,
        enabled,
      });
      for (const id of result.succeeded) recordResult(id, app, enabled);
      for (const failure of result.failed) {
        recordResult(failure.item, app, enabled, failure.error);
      }
      return { succeeded: result.succeeded, failed: result.failed.length };
    } catch (error) {
      for (const id of ids) recordResult(id, app, enabled, error);
      return { succeeded: [] as string[], failed: ids.length };
    }
  };

  const bulkToggle = async (
    ids: string[],
    app: AppId,
    enabled: boolean,
    toastKey: { on: string; off: string },
  ) => {
    if (!beginWrite()) return;
    try {
      const { succeeded, failed } = await writeMany(ids, app, enabled);
      let text = t(enabled ? toastKey.on : toastKey.off, {
        app: APP_DISPLAY_NAME[app],
        count: succeeded.length,
        noun,
      });
      if (failed) text += t("appMatrix.toast.partialFail", { count: failed });
      showUndoToast(
        text,
        t("appMatrix.undo"),
        succeeded.length
          ? () => {
              void (async () => {
                if (!beginWrite()) return;
                try {
                  await writeMany(succeeded, app, !enabled);
                  toast.success(t("appMatrix.toast.undone"), {
                    closeButton: true,
                  });
                } finally {
                  endWrite();
                }
              })();
            }
          : undefined,
      );
    } finally {
      endWrite();
    }
  };

  const handleColumnBulk = (app: AppId, enabled: boolean) => {
    const ids = scope.rows
      .filter((skill) => Boolean(skill.apps[app]) !== enabled)
      .map((skill) => skill.id);
    if (ids.length === 0) return;
    void bulkToggle(ids, app, enabled, {
      on: "appMatrix.toast.enabled",
      off: "appMatrix.toast.disabled",
    });
  };

  const handleSelectionToggle = (app: AppId, enabled: boolean) => {
    const chosen = installedSkills.filter((skill) => selected.has(skill.id));
    const ids = chosen
      .filter((skill) => Boolean(skill.apps[app]) !== enabled)
      .map((skill) => skill.id);
    if (ids.length === 0) {
      toast.info(
        enabled
          ? t("skillsPage.toast.selectionAlreadyOn", {
              count: chosen.length,
              app: APP_DISPLAY_NAME[app],
            })
          : t("skillsPage.toast.selectionAlreadyOff", {
              count: chosen.length,
              app: APP_DISPLAY_NAME[app],
            }),
        { closeButton: true },
      );
      return;
    }
    void bulkToggle(ids, app, enabled, {
      on: "appMatrix.toast.enabled",
      off: "skillsPage.toast.disabledSelection",
    });
  };

  // ─── 卸载 / 更新 ────────────────────────────────────────────────────
  const uninstallIds = async (ids: string[]) => {
    if (!beginWrite(true)) return;
    const done: InstalledSkill[] = [];
    let piWarning: string | undefined;
    let backupPath: string | undefined;
    try {
      for (const id of ids) {
        const skill = installedSkills.find((item) => item.id === id);
        try {
          const result = await uninstallMutation.mutateAsync(id);
          if (skill) done.push(skill);
          if (result.preservedPiPath) {
            piWarning = t("skills.uninstallPiPreserved", {
              path: result.preservedPiPath,
            });
          } else if (result.piCleanupIncomplete) {
            piWarning = t("skills.uninstallPiCleanupIncomplete");
          }
          if (result.backupPath) backupPath = result.backupPath;
        } catch (error) {
          toast.error(t("common.error"), {
            description: `${skill?.name ?? id}: ${String(error)}`,
          });
        }
      }
      setConfirm(null);
      setSelected((prev) => {
        const next = new Set(prev);
        for (const id of ids) next.delete(id);
        return next;
      });
    } finally {
      endWrite();
    }
    if (done.length === 0) return;
    const text =
      done.length === 1
        ? t("skills.uninstallSuccess", { name: done[0].name })
        : t("skillsPage.toast.uninstalledMany", { count: done.length });
    const options = {
      description:
        piWarning ??
        (done.length === 1 && backupPath
          ? t("skills.backup.location", { path: backupPath })
          : t("skillsPage.toast.backupAt", { path: BACKUP_DIR })),
      closeButton: true,
    };
    if (piWarning) toast.warning(text, options);
    else toast.success(text, options);
  };

  const updateIds = async (ids: string[], allowOpenDialog = false) => {
    if (ids.length === 0 || !beginWrite(allowOpenDialog)) return;
    // 不让确认框等着整批更新跑完：点了确认就关，每行的 ⋯ 和页头按钮显示进度
    setConfirm(null);
    setIsUpdatingMany(ids.length > 1);
    setUpdateProgress({ done: 0, total: ids.length });
    const updated: string[] = [];
    try {
      for (const [index, id] of ids.entries()) {
        try {
          const result = await updateSkillMutation.mutateAsync(id);
          updated.push(result.name);
        } catch (error) {
          const name = installedSkills.find((skill) => skill.id === id)?.name;
          toast.error(t("skills.updateFailed"), {
            description: `${name ?? id}: ${String(error)}`,
          });
        }
        setUpdateProgress({ done: index + 1, total: ids.length });
      }
    } finally {
      setIsUpdatingMany(false);
      setUpdateProgress(null);
      endWrite();
    }
    if (updated.length === 1) {
      toast.success(t("skills.updateSuccess", { name: updated[0] }), {
        closeButton: true,
      });
    } else if (updated.length > 1) {
      toast.success(t("skills.updateAllSuccess", { count: updated.length }), {
        closeButton: true,
      });
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
      const installedIds = new Set(installedSkills.map((skill) => skill.id));
      const updates = (result.data?.updates ?? []).filter((update) =>
        installedIds.has(update.id),
      );
      const failures = result.data?.failures ?? [];
      setDismissedRepoFailAt(null);
      if (updates.length === 0 && failures.length > 0) {
        // 有仓库没读到时不能说「全部最新」；哪些仓库、为什么写在横幅里
        toast.warning(
          t("skills.updatesIncomplete", { count: failures.length }),
          { closeButton: true },
        );
      } else if (updates.length === 0) {
        toast.success(t("skills.noUpdates"), { closeButton: true });
      } else {
        // 查到就直接问要不要全部更新，不用再去筛选里找「全部更新」
        setConfirm({ kind: "updateAll" });
      }
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    } finally {
      checkUpdatesLockRef.current = false;
    }
  };

  // ─── 导入 / ZIP / 备份 ───────────────────────────────────────────────
  const handleOpenImport = async () => {
    if (!beginWrite()) return;
    try {
      const result = await scanUnmanaged();
      if (!result.data || result.data.length === 0) {
        toast.success(t("skills.noUnmanagedFound"), { closeButton: true });
        return;
      }
      setImportOpen(true);
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
      setImportOpen(false);
      toast.success(t("skills.importSuccess", { count: imported.length }), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    } finally {
      endWrite();
    }
  };

  const reportEnableFailures = (
    failures: Array<{ id: string; app: AppId; error: unknown }>,
  ) => {
    if (failures.length === 0) return;
    recordEnableFailures(failures);
    toast.warning(
      t("skillsPage.toast.enableFailed", {
        apps: names([...new Set(failures.map((failure) => failure.app))]),
      }),
      { closeButton: true },
    );
  };

  const handleInstallFromZip = async () => {
    if (!beginWrite()) return;
    try {
      const filePath = await skillsApi.openZipFileDialog();
      if (!filePath) return;
      let skipped: ZipSkippedSkill[] = [];
      const { installed, failures } = await installTo(async (firstApp) => {
        const result = await installFromZipMutation.mutateAsync({
          filePath,
          currentApp: firstApp,
        });
        skipped = result.skipped;
        return result.installed;
      });
      if (skipped.length > 0) {
        // #3749：同名被跳过要说清楚跳过了哪些、被谁占用，不能说成「ZIP 里没有技能」
        const list = skipped
          .map((item) =>
            t("skills.installFromZip.skippedItem", {
              directory: item.directory,
              name: item.existingName,
            }),
          )
          .join(listSeparator);
        toast.warning(
          installed.length === 0
            ? t("skills.installFromZip.allSkipped", { count: skipped.length })
            : t("skills.installFromZip.skippedTitle", {
                count: skipped.length,
              }),
          {
            description: t("skills.installFromZip.skippedBody", { list }),
            closeButton: true,
          },
        );
      }
      if (installed.length === 0) {
        if (skipped.length === 0) {
          toast.info(t("skills.installFromZip.noSkillsFound"), {
            closeButton: true,
          });
        }
      } else if (installed.length === 1) {
        toast.success(
          t("skillsPage.toast.installed", {
            name: installed[0].name,
            apps: names(targets),
          }),
          { closeButton: true },
        );
      } else {
        toast.success(
          t("skillsPage.toast.installedMany", {
            count: installed.length,
            apps: names(targets),
          }),
          { closeButton: true },
        );
      }
      reportEnableFailures(failures);
    } catch (error) {
      toast.error(t("skills.installFailed"), { description: String(error) });
    } finally {
      endWrite();
    }
  };

  const handleOpenRestoreFromBackup = async () => {
    if (!beginWrite()) return;
    setRestoreOpen(true);
    try {
      await refetchSkillBackups({ throwOnError: true });
    } catch (error) {
      setRestoreOpen(false);
      toast.error(t("common.error"), { description: String(error) });
    } finally {
      endWrite();
    }
  };

  const handleRestoreFromBackup = async (backupId: string) => {
    if (!beginWrite(true)) return;
    try {
      const { installed, failures } = await installTo(async (firstApp) => [
        await restoreBackupMutation.mutateAsync({
          backupId,
          currentApp: firstApp,
        }),
      ]);
      setRestoreOpen(false);
      toast.success(
        t("skills.restoreFromBackup.success", { name: installed[0]?.name }),
        { closeButton: true },
      );
      reportEnableFailures(failures);
    } catch (error) {
      toast.error(t("skills.restoreFromBackup.failed"), {
        description: String(error),
      });
    } finally {
      endWrite();
    }
  };

  const confirmDeleteBackup = async (backup: SkillBackupEntry) => {
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
        console.error("Failed to refresh Skill backups after deletion:", error);
      }

      if (!deleteSucceeded) {
        if (
          refreshedBackups &&
          !refreshedBackups.some((entry) => entry.backupId === backup.backupId)
        ) {
          setConfirm(null);
        }
        toast.error(t("skills.restoreFromBackup.deleteFailed"), {
          description: String(deleteError),
        });
      } else {
        setConfirm(null);
        toast.success(
          t("skills.restoreFromBackup.deleteSuccess", {
            name: backup.skill.name,
          }),
          { closeButton: true },
        );
      }
    } finally {
      endWrite();
    }
  };

  const openDocs = async (skill: InstalledSkill) => {
    const url =
      skill.readmeUrl ||
      (skill.repoOwner && skill.repoName
        ? `https://github.com/${skill.repoOwner}/${skill.repoName}`
        : undefined);
    if (!url) return;
    try {
      await settingsApi.openExternal(url);
    } catch {
      // ignore
    }
  };

  const copyDirectory = async (skill: InstalledSkill) => {
    try {
      await copyText(skill.directory);
      toast.success(t("skillsPage.toast.copiedDir", { dir: skill.directory }), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const showInstalled = (skillId: string) => {
    setSearchQuery("");
    setStatusFilter("all");
    setSourceFilter("all");
    setView("installed");
    setHighlightId(skillId);
  };

  // ─── 渲染：页头 ─────────────────────────────────────────────────────
  const nInstalled = installedSkills.length;
  const nUpdates = applicableSkillUpdates.length;
  const nUnmanaged = unmanagedSkills?.length ?? 0;
  const loadOk = !isLoading && !isError;
  const updatesOnly = view === "installed" && statusFilter === "updates";
  const toggleUpdatesFilter = () => {
    if (updatesOnly) {
      setStatusFilter("all");
      return;
    }
    setView("installed");
    setStatusFilter("updates");
  };

  const header = (
    <AppPageHeader
      icon={<SkillsIcon size={20} />}
      title="Skills"
      titleExtra={
        <>
          <HelpTip title={t("skillsPage.helpTitle")}>
            {t("skillsPage.help")}
          </HelpTip>
          {/* 已安装数量只写在页签上；页头只留可点的「N 个可更新」 */}
          {loadOk && nUpdates > 0 && (
            <span className="ms-2 flex items-center whitespace-nowrap text-body">
              {/* 可点：把表格筛到可更新的项，再点恢复；「全部更新」在搜索框旁 */}
              <HoverTip
                content={
                  updatesOnly
                    ? t("skillsPage.headerUpdatesClear")
                    : t("skillsPage.headerUpdatesShow")
                }
              >
                <button
                  type="button"
                  aria-pressed={updatesOnly}
                  onClick={toggleUpdatesFilter}
                  className={cn(
                    "-mx-1 inline-flex h-6 items-center rounded-control px-1.5 text-body font-medium transition-colors duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                    updatesOnly
                      ? "bg-selected text-fg-1"
                      : "text-fg-2 underline decoration-border-strong underline-offset-[3px] hover:bg-subtle hover:text-fg-1",
                  )}
                >
                  {t("skillsPage.headerUpdates", { count: nUpdates })}
                </button>
              </HoverTip>
            </span>
          )}
        </>
      }
      actions={
        <>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                type="button"
                variant="solid"
                size="regular"
                className="pe-2.5"
                disabled={controlsDisabled}
              >
                <Plus className="h-4 w-4" strokeWidth={2} />
                {t("skillsPage.add")}
                <ChevronDown className="h-3.5 w-3.5" strokeWidth={2} />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="min-w-[220px]">
              <DropdownMenuItem onSelect={() => setView("discover")}>
                {t("skillsPage.addMenu.discover")}
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={() => void handleInstallFromZip()}>
                {t("skillsPage.addMenu.zip")}
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={() => void handleOpenImport()}>
                <span className="flex-1">{t("skillsPage.addMenu.import")}</span>
                {nUnmanaged > 0 && <NeutralBadge>{nUnmanaged}</NeutralBadge>}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <DropdownMenu>
            <HoverTip content={t("common.more")}>
              <DropdownMenuTrigger asChild>
                <Button
                  type="button"
                  variant="quiet"
                  size="icon-compact"
                  className="h-8 w-8"
                  aria-label={t("skills.moreActions")}
                >
                  <MoreHorizontal className="h-4 w-4" strokeWidth={1.5} />
                </Button>
              </DropdownMenuTrigger>
            </HoverTip>
            <DropdownMenuContent align="end" className="min-w-[200px]">
              <DropdownMenuItem
                disabled={controlsDisabled || nInstalled === 0}
                onSelect={() => void handleCheckUpdates()}
              >
                {t("skills.checkUpdates")}
              </DropdownMenuItem>
              <DropdownMenuItem
                disabled={controlsDisabled}
                onSelect={() => void handleOpenRestoreFromBackup()}
              >
                {t("skillsPage.moreMenu.restore")}
              </DropdownMenuItem>
              <DropdownMenuItem
                disabled={controlsDisabled}
                onSelect={() => setRepoManagerOpen(true)}
              >
                {t("skillsPage.moreMenu.repos")}
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={() => setStorageOpen(true)}>
                {t("skills.storageSheet.open")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </>
      }
    />
  );

  // 「已安装 / 发现」换的是整块内容：页面级导航，用下划线页签（不是分段控件）。
  // 页签行只渲染一份（两段共用，下划线才滑得过去）；各段右侧的控件用 portal 放进 trailing 槽。
  const [tabsTrailingSlot, setTabsTrailingSlot] =
    useState<HTMLDivElement | null>(null);
  const viewTabsTrailing = (owner: SkillsView, trailing: React.ReactNode) =>
    view === owner && tabsTrailingSlot
      ? createPortal(trailing, tabsTrailingSlot)
      : null;
  const viewTabs = (
    <PageTabs<SkillsView>
      aria-label={t("skillsPage.viewAria")}
      idPrefix="skills-view"
      className="h-11"
      value={view}
      onValueChange={(next) => {
        if (next === "discover" && navigationBlocked) return;
        setView(next);
      }}
      items={[
        {
          value: "installed",
          label: loadOk
            ? t("skillsPage.viewInstalledCount", { count: nInstalled })
            : t("skillsPage.viewInstalled"),
        },
        {
          value: "discover",
          label: t("skillsPage.viewDiscover"),
        },
      ]}
      trailing={<div ref={setTabsTrailingSlot} className="flex items-center" />}
    />
  );

  // ─── 渲染：已安装段 ─────────────────────────────────────────────────
  const statusLabel = (value: StatusFilter) => {
    if (value === "updates") return t("skillsPage.filter.updates");
    if (value === "none") return t("skillsPage.filter.none");
    if (value.startsWith("app:"))
      return t("skillsPage.filter.inApp", {
        app: APP_DISPLAY_NAME[value.slice(4) as AppId],
      });
    return t("skillsPage.filter.all");
  };

  const renderInstalledBody = () => {
    if (isLoading) {
      return (
        <div className="py-12 text-center text-body text-fg-2">
          {t("skills.loading")}
        </div>
      );
    }
    if (isError) {
      return (
        <div className="flex flex-1 flex-col items-center justify-center gap-2.5 px-10 pb-12 text-center">
          <h2 className="m-0 text-section">{t("skillsPage.loadFailed")}</h2>
          <code className="max-w-[520px] rounded-[8px] bg-subtle px-3 py-2 text-left font-mono text-caption text-fg-2 [overflow-wrap:anywhere]">
            {extractErrorMessage(loadError) || String(loadError)}
          </code>
          <p className="m-0 text-body text-fg-2">
            {t("mcpPage.noFilesChanged")}
          </p>
          <Button
            type="button"
            variant="neutral"
            size="regular"
            className="mt-1"
            onClick={() => void refetch()}
          >
            {t("common.retry")}
          </Button>
        </div>
      );
    }
    if (nInstalled === 0) {
      return (
        <div className="flex flex-1 flex-col items-center justify-center gap-2 px-10 pb-12 text-center">
          <h2 className="m-0 text-section">{t("skillsPage.emptyTitle")}</h2>
          <p className="m-0 text-body text-fg-2">{t("skillsPage.emptyBody")}</p>
          <div className="mt-2 flex gap-2">
            <Button
              type="button"
              variant="neutral"
              size="regular"
              onClick={() => setView("discover")}
            >
              {t("skillsPage.goDiscover")}
            </Button>
            <Button
              type="button"
              variant="neutral"
              size="regular"
              disabled={controlsDisabled}
              onClick={() => void handleOpenImport()}
            >
              {nUnmanaged
                ? t("skillsPage.importCount", { count: nUnmanaged })
                : t("skillsPage.addMenu.import")}
            </Button>
          </div>
        </div>
      );
    }

    const selectionActive = selected.size > 0;
    const nVisibleSelected = filteredSkills.filter((skill) =>
      selected.has(skill.id),
    ).length;
    const allVisibleSelected =
      filteredSkills.length > 0 && nVisibleSelected === filteredSkills.length;
    return (
      <div
        data-testid="skills-matrix"
        className="min-h-0 overflow-auto scroll-stable rounded-panel border border-border bg-surface"
      >
        <MatrixColumnHighlight>
          <div className="min-w-[600px]">
            <div className="sticky top-0 z-10 flex h-11 items-center border-b border-border bg-subtle px-2">
              <span className="flex w-6 shrink-0 justify-center">
                <Checkbox
                  aria-label={t("skillsPage.selectAllAria")}
                  disabled={filteredSkills.length === 0}
                  checked={
                    allVisibleSelected
                      ? true
                      : nVisibleSelected > 0
                        ? "indeterminate"
                        : false
                  }
                  onCheckedChange={() =>
                    // 只管当前筛选出来的这些行；半选时点一下补齐
                    setSelected((prev) => {
                      const next = new Set(prev);
                      for (const skill of filteredSkills) {
                        if (allVisibleSelected) next.delete(skill.id);
                        else next.add(skill.id);
                      }
                      return next;
                    })
                  }
                />
              </span>
              <div className="-ms-0.5 flex min-w-0 flex-1 items-center gap-0.5">
                {selectionActive ? (
                  <>
                    <span className="shrink-0 whitespace-nowrap pe-1.5 ps-2.5 text-body font-medium tabular-nums">
                      {t("skillsPage.bulk.selected", { count: selected.size })}
                    </span>
                    <AppMenuButton
                      label={t("skillsPage.bulk.enableTo")}
                      ariaLabel={t("skillsPage.bulk.enableToAria", {
                        count: selected.size,
                      })}
                      apps={appIds}
                      disabled={controlsDisabled}
                      onPick={(app) => handleSelectionToggle(app, true)}
                    />
                    <AppMenuButton
                      label={t("skillsPage.bulk.disable")}
                      ariaLabel={t("skillsPage.bulk.disableAria", {
                        count: selected.size,
                      })}
                      apps={appIds}
                      disabled={controlsDisabled}
                      onPick={(app) => handleSelectionToggle(app, false)}
                    />
                    <Button
                      type="button"
                      variant="quiet"
                      size="compact"
                      className="px-2"
                      disabled={controlsDisabled}
                      onClick={() => {
                        const ids = [...selected].filter(
                          (id) => updatesMap[id],
                        );
                        if (ids.length === 0) {
                          toast.info(
                            t("skillsPage.toast.noUpdatesInSelection"),
                            {
                              closeButton: true,
                            },
                          );
                          return;
                        }
                        void updateIds(ids);
                      }}
                    >
                      {t("skills.update")}
                    </Button>
                    <Button
                      type="button"
                      variant="quiet"
                      size="compact"
                      className="px-2 text-danger-text hover:text-danger-text"
                      disabled={controlsDisabled}
                      onClick={() =>
                        setConfirm({ kind: "uninstall", ids: [...selected] })
                      }
                    >
                      {t("skillsPage.bulk.uninstall")}
                    </Button>
                    <div className="flex-1" />
                    <Button
                      type="button"
                      variant="quiet"
                      size="compact"
                      className="px-2 text-fg-2"
                      onClick={() => setSelected(new Set())}
                    >
                      {t("common.cancel")}
                    </Button>
                  </>
                ) : (
                  <>
                    <FilterMenu
                      label={statusLabel(statusFilter)}
                      active={statusFilter !== "all"}
                      items={(["all", "updates", "none"] as StatusFilter[]).map(
                        (value) => ({
                          key: value,
                          label: statusLabel(value),
                          checked: statusFilter === value,
                          onSelect: () => setStatusFilter(value),
                        }),
                      )}
                    />
                    <FilterMenu
                      label={t("skillsPage.filter.sourceButton", {
                        source:
                          sourceFilter === "all"
                            ? t("skillsPage.filter.all")
                            : sourceLabel(sourceFilter),
                      })}
                      active={sourceFilter !== "all"}
                      items={[
                        {
                          key: "all",
                          label: t("skillsPage.filter.all"),
                          checked: sourceFilter === "all",
                          onSelect: () => setSourceFilter("all"),
                        },
                        ...sourceOptions.map(([key, count]) => ({
                          key,
                          label: sourceLabel(key),
                          mono: key !== "local",
                          count,
                          checked: sourceFilter === key,
                          onSelect: () => setSourceFilter(key),
                        })),
                      ]}
                    />
                  </>
                )}
              </div>
              <div className="flex shrink-0">
                {appIds.map((app) => {
                  const isPi = app === "pi";
                  const onInScope = scope.rows.filter(
                    (skill) => skill.apps[app],
                  ).length;
                  const appFilter: StatusFilter = `app:${app}`;
                  return (
                    <MatrixColumnHeader
                      key={app}
                      app={app}
                      enabledCount={
                        installedSkills.filter((skill) => skill.apps[app])
                          .length
                      }
                      totalCount={nInstalled}
                      scopeTotal={scope.rows.length}
                      scopeEnabled={onInScope}
                      scopeFailed={
                        scope.rows.filter(
                          (skill) => fails[failKey(skill.id, app)],
                        ).length
                      }
                      scopeKind={scope.kind}
                      noun={noun}
                      disabled={controlsDisabled}
                      title={isPi ? t("skillsPage.piColumnTitle") : undefined}
                      help={
                        isPi
                          ? {
                              title: t("skillsPage.piHelpTitle"),
                              body: t("skillsPage.piHelp"),
                            }
                          : undefined
                      }
                      extraAction={{
                        label:
                          statusFilter === appFilter
                            ? t("skillsPage.pop.showAll")
                            : t("skillsPage.pop.onlyApp", {
                                app: APP_DISPLAY_NAME[app],
                              }),
                        onClick: () =>
                          setStatusFilter(
                            statusFilter === appFilter ? "all" : appFilter,
                          ),
                      }}
                      onEnableRest={() => handleColumnBulk(app, true)}
                      onDisableAll={() => handleColumnBulk(app, false)}
                    />
                  );
                })}
              </div>
              <span aria-hidden="true" className="w-16 shrink-0" />
            </div>

            {filteredSkills.length === 0 ? (
              <div className="flex flex-col items-center gap-2 px-6 py-10 text-center">
                <h2 className="m-0 text-section">
                  {normalizedQuery
                    ? t("skillsPage.noMatch", { query: searchQuery.trim() })
                    : t("skillsPage.noFilterMatch")}
                </h2>
                <div className="flex flex-wrap justify-center gap-2 pt-1">
                  {normalizedQuery ? (
                    <>
                      <Button
                        type="button"
                        variant="neutral"
                        size="regular"
                        onClick={() => setSearchQuery("")}
                      >
                        {t("mcpPage.clearSearch")}
                      </Button>
                      <Button
                        type="button"
                        variant="neutral"
                        size="regular"
                        onClick={() => {
                          setDiscoverQuery(searchQuery.trim());
                          setSearchQuery("");
                          setView("discover");
                        }}
                      >
                        {t("skillsPage.searchInDiscover", {
                          query: searchQuery.trim(),
                        })}
                      </Button>
                    </>
                  ) : (
                    <Button
                      type="button"
                      variant="neutral"
                      size="regular"
                      onClick={() => {
                        setStatusFilter("all");
                        setSourceFilter("all");
                      }}
                    >
                      {t("skillsPage.clearFilters")}
                    </Button>
                  )}
                </div>
              </div>
            ) : (
              <ul
                aria-label={t("skillsPage.listLabel")}
                className="m-0 list-none p-0"
              >
                {filteredSkills.map((skill, index) => (
                  <InstalledRow
                    key={skill.id}
                    skill={skill}
                    first={index === 0}
                    appIds={appIds}
                    checked={selected.has(skill.id)}
                    hasUpdate={Boolean(updatesMap[skill.id])}
                    isUpdating={
                      updateSkillMutation.isPending &&
                      updateSkillMutation.variables === skill.id
                    }
                    highlighted={highlightId === skill.id}
                    fails={fails}
                    disabled={controlsDisabled}
                    sourceText={
                      skill.repoOwner && skill.repoName
                        ? `${skill.repoOwner}/${skill.repoName}`
                        : t("skillsPage.source.local")
                    }
                    onPick={(checked) =>
                      setSelected((prev) => {
                        const next = new Set(prev);
                        if (checked) next.add(skill.id);
                        else next.delete(skill.id);
                        return next;
                      })
                    }
                    onCell={(app) => {
                      const failure = fails[failKey(skill.id, app)];
                      void writeOne(
                        skill.id,
                        app,
                        failure ? failure.desired : !skill.apps[app],
                      );
                    }}
                    onOpenSource={() => void openDocs(skill)}
                    onFixSync={() => setStorageOpen(true)}
                    onUpdate={() => void updateIds([skill.id])}
                    onOpenDocs={() => void openDocs(skill)}
                    onCopyDir={() => void copyDirectory(skill)}
                    onUninstall={() => {
                      if (writeLockRef.current || interactionBlocked) return;
                      setConfirm({ kind: "uninstall", ids: [skill.id] });
                    }}
                  />
                ))}
              </ul>
            )}
          </div>
        </MatrixColumnHighlight>
      </div>
    );
  };

  const showUnmanagedBanner =
    nUnmanaged > 0 && dismissedUnmanaged !== nUnmanaged && nInstalled > 0;
  const showUpdateRepoFailBanner =
    updateRepoFailures.length > 0 && dismissedRepoFailAt !== updatesCheckedAt;

  const uninstallTargets =
    confirm?.kind === "uninstall"
      ? installedSkills.filter((skill) => confirm.ids.includes(skill.id))
      : [];

  return (
    <>
      {header}
      <div id="main-content" className="flex min-h-0 flex-1 flex-col">
        <div className="shrink-0 px-6 pt-1">{viewTabs}</div>
        {/* 两段打开过就一直挂着，切回来不重新加载；不在看的那段只是隐藏 */}
        {discoverMounted && (
          <div
            className={cn(
              "min-h-0 flex-1 flex-col",
              view === "discover" ? "flex" : "hidden",
            )}
          >
            <SkillsPage
              key={discoverQuery}
              initialQuery={discoverQuery}
              renderViewTabs={(trailing) =>
                viewTabsTrailing("discover", trailing)
              }
              visibleAppIds={appIds}
              installTargets={targets}
              onInstallTargetsChange={setTargets}
              installTo={installTo}
              onShowInstalled={showInstalled}
              onOpenRepoManager={() => setRepoManagerOpen(true)}
              onEnableFailures={recordEnableFailures}
            />
          </div>
        )}
        <div
          className={cn(
            "min-h-0 flex-1 flex-col",
            view === "installed" ? "flex" : "hidden",
          )}
        >
          {viewTabsTrailing(
            "installed",
            loadOk && nInstalled > 0 ? (
              <div className="flex items-center gap-2">
                <MatrixSearch
                  className="w-[280px] min-w-[160px] shrink"
                  value={searchQuery}
                  onValueChange={setSearchQuery}
                  placeholder={t("skillsPage.searchPlaceholder")}
                  ariaLabel={t("skills.installedSearchAriaLabel")}
                  status={
                    normalizedQuery
                      ? t("appMatrix.found", {
                          count: filteredSkills.length,
                        })
                      : ""
                  }
                />
                <HoverTip
                  content={
                    lastCheckedText
                      ? t("skillsPage.lastChecked", {
                          when: lastCheckedText,
                        })
                      : undefined
                  }
                >
                  {/* 已知有更新时这颗按钮就是「全部更新」；想重新检查走「⋯」菜单 */}
                  {updateProgress ? (
                    <Button
                      type="button"
                      variant="neutral"
                      size="regular"
                      className="shrink-0"
                      disabled
                    >
                      <Loader2 className="h-4 w-4 animate-spin" />
                      {t("skillsPage.updatingProgress", {
                        done: Math.min(
                          updateProgress.done + 1,
                          updateProgress.total,
                        ),
                        total: updateProgress.total,
                      })}
                    </Button>
                  ) : nUpdates > 0 && !isCheckingUpdates ? (
                    <Button
                      type="button"
                      variant="neutral"
                      size="regular"
                      className="shrink-0"
                      disabled={controlsDisabled}
                      onClick={() => setConfirm({ kind: "updateAll" })}
                    >
                      <ArrowUpCircle className="h-4 w-4" strokeWidth={2} />
                      {t("skillsPage.updateAllCount", { count: nUpdates })}
                    </Button>
                  ) : (
                    <Button
                      type="button"
                      variant="quiet"
                      size="regular"
                      className="shrink-0"
                      disabled={controlsDisabled}
                      onClick={() => void handleCheckUpdates()}
                    >
                      {isCheckingUpdates ? (
                        <Loader2 className="h-4 w-4 animate-spin" />
                      ) : (
                        <RefreshCw className="h-4 w-4" strokeWidth={2} />
                      )}
                      {isCheckingUpdates
                        ? t("skills.checkingUpdates")
                        : t("skills.checkUpdates")}
                    </Button>
                  )}
                </HoverTip>
              </div>
            ) : null,
          )}

          <NoticeSlot
            className={cn(
              (showUnmanagedBanner || showUpdateRepoFailBanner) && "px-6 pt-3",
            )}
          >
            {showUpdateRepoFailBanner && (
              <Notice
                tone="warning"
                title={t("skillsPage.repoFail.title", {
                  count: updateRepoFailures.length,
                  repos: describeRepoFailures(updateRepoFailures, t),
                })}
                actions={
                  <Button
                    type="button"
                    variant="neutral"
                    size="compact"
                    disabled={controlsDisabled}
                    onClick={() => void handleCheckUpdates()}
                  >
                    {t("common.retry")}
                  </Button>
                }
                onDismiss={() => setDismissedRepoFailAt(updatesCheckedAt)}
                dismissLabel={t("skillsPage.banner.close")}
              >
                {t("skillsPage.repoFail.updatesBody")}
              </Notice>
            )}
            {showUnmanagedBanner && (
              <Notice
                title={t("skillsPage.banner.unmanaged", {
                  count: nUnmanaged,
                })}
                actions={
                  <>
                    <Button
                      type="button"
                      variant="quiet"
                      size="compact"
                      disabled={controlsDisabled}
                      onClick={() => void handleOpenImport()}
                    >
                      {t("skillsPage.banner.reviewImport")}
                    </Button>
                    <Button
                      type="button"
                      variant="quiet"
                      size="compact"
                      className="text-fg-2"
                      onClick={() => setDismissedUnmanaged(nUnmanaged)}
                    >
                      {t("skillsPage.banner.ignore")}
                    </Button>
                  </>
                }
              />
            )}
          </NoticeSlot>

          <div className="flex min-h-0 flex-1 flex-col px-6 pb-5 pt-3">
            {renderInstalledBody()}
          </div>
        </div>
      </div>

      <V7ConfirmDialog
        open={confirm?.kind === "uninstall"}
        // 卸载前会备份、能从备份恢复：可恢复操作，不用红色确认键
        danger={false}
        title={
          uninstallTargets.length === 1
            ? t("skillsPage.confirm.uninstallOne", {
                name: uninstallTargets[0].name,
              })
            : t("skillsPage.confirm.uninstallMany", {
                count: uninstallTargets.length,
              })
        }
        body={t("skillsPage.confirm.uninstallBody", { path: BACKUP_DIR })}
        confirmLabel={
          uninstallTargets.length === 1
            ? t("skills.uninstall")
            : t("skillsPage.confirm.uninstallManyButton", {
                count: uninstallTargets.length,
              })
        }
        pending={writePending}
        onConfirm={() =>
          confirm?.kind === "uninstall" && void uninstallIds(confirm.ids)
        }
        onCancel={() => setConfirm(null)}
      />

      <V7ConfirmDialog
        open={confirm?.kind === "updateAll"}
        danger={false}
        title={t("skillsPage.confirm.updateAll", { count: nUpdates })}
        body={t("skillsPage.confirm.updateBody", { path: BACKUP_DIR })}
        details={
          <ul
            aria-label={t("skillsPage.confirm.updateListAria")}
            className="max-h-60 divide-y divide-border overflow-y-auto rounded-control border border-border"
          >
            {applicableSkillUpdates.map((update) => {
              const skill = installedSkills.find(
                (item) => item.id === update.id,
              );
              const repo =
                skill?.repoOwner && skill.repoName
                  ? `${skill.repoOwner}/${skill.repoName}`
                  : null;
              return (
                <li
                  key={update.id}
                  className="flex items-baseline gap-2 px-3 py-2 text-body"
                >
                  <span className="min-w-0 truncate font-medium text-fg-1">
                    {skill?.name ?? update.name}
                  </span>
                  {repo && skill && (
                    // 和列表行的来源链接一样：有 README 地址开 README，否则开仓库首页
                    <button
                      type="button"
                      onClick={() => void openDocs(skill)}
                      className="ms-auto shrink-0 rounded-sm font-mono text-caption text-fg-3 underline decoration-border-strong underline-offset-[3px] hover:text-fg-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    >
                      {repo}
                    </button>
                  )}
                </li>
              );
            })}
          </ul>
        }
        confirmLabel={t("skillsPage.confirm.updateAllButton", {
          count: nUpdates,
        })}
        pending={writePending || isUpdatingMany}
        onConfirm={() =>
          void updateIds(
            applicableSkillUpdates.map((update) => update.id),
            true,
          )
        }
        onCancel={() => setConfirm(null)}
      />

      {importOpen && unmanagedSkills && (
        <SkillImportDialog
          skills={unmanagedSkills}
          visibleAppIds={appIds}
          isImporting={importMutation.isPending}
          onImport={(imports) => void handleImport(imports)}
          onClose={() => setImportOpen(false)}
        />
      )}

      <SkillRestoreDialog
        open={restoreOpen}
        backups={skillBackups}
        isLoading={isFetchingSkillBackups}
        isRestoring={restoreBackupMutation.isPending}
        isDeleting={deleteBackupMutation.isPending}
        targetsLabel={names(targets)}
        onRestore={(backupId) => void handleRestoreFromBackup(backupId)}
        onDelete={(backup) => {
          if (checkUpdatesLockRef.current || writeLockRef.current) return;
          setConfirm({ kind: "deleteBackup", backup });
        }}
        onClose={() => setRestoreOpen(false)}
      />

      <V7ConfirmDialog
        open={confirm?.kind === "deleteBackup"}
        title={t("skills.restoreFromBackup.deleteConfirmTitle")}
        body={
          confirm?.kind === "deleteBackup"
            ? t("skills.restoreFromBackup.deleteConfirmMessage", {
                name: confirm.backup.skill.name,
              })
            : ""
        }
        confirmLabel={t("skills.restoreFromBackup.delete")}
        pending={writePending}
        onConfirm={() =>
          confirm?.kind === "deleteBackup" &&
          void confirmDeleteBackup(confirm.backup)
        }
        onCancel={() => setConfirm(null)}
      />

      <SkillsStorageSheet open={storageOpen} onOpenChange={setStorageOpen} />

      {repoManagerOpen && (
        <RepoManagerContainer onClose={() => setRepoManagerOpen(false)} />
      )}
    </>
  );
};

UnifiedSkillsPanel.displayName = "UnifiedSkillsPanel";

// ─── 已安装的一行 ────────────────────────────────────────────────────────
interface InstalledRowProps {
  skill: InstalledSkill;
  first: boolean;
  appIds: AppId[];
  checked: boolean;
  hasUpdate: boolean;
  isUpdating: boolean;
  highlighted: boolean;
  fails: Record<string, WriteFailure>;
  disabled: boolean;
  sourceText: string;
  onPick: (checked: boolean) => void;
  onCell: (app: AppId) => void;
  onOpenSource: () => void;
  onFixSync: () => void;
  onUpdate: () => void;
  onOpenDocs: () => void;
  onCopyDir: () => void;
  onUninstall: () => void;
}

function InstalledRow({
  skill,
  first,
  appIds,
  checked,
  hasUpdate,
  isUpdating,
  highlighted,
  fails,
  disabled,
  sourceText,
  onPick,
  onCell,
  onOpenSource,
  onFixSync,
  onUpdate,
  onOpenDocs,
  onCopyDir,
  onUninstall,
}: InstalledRowProps) {
  const { t } = useTranslation();
  const enabled = SKILLS_APP_IDS.some((app) => skill.apps[app]);
  const showDir =
    skill.directory &&
    skill.directory.trim().toLowerCase() !== skill.name.trim().toLowerCase();
  const failedApp = appIds.find((app) => fails[failKey(skill.id, app)]);
  const failure = failedApp ? fails[failKey(skill.id, failedApp)] : undefined;
  const failId = `sk-fail-${skill.id}`;
  const hasRepo = Boolean(skill.repoOwner && skill.repoName);

  return (
    <li
      id={`sk-row-${skill.id}`}
      tabIndex={-1}
      className={cn(
        "group relative flex h-14 items-center px-2 outline-none transition-colors duration-150 hover:bg-subtle",
        !first && "border-t border-border",
        highlighted && "bg-subtle",
      )}
    >
      <span className="flex w-6 shrink-0 justify-center">
        <input
          type="checkbox"
          // 勾选框一直显示，不等悬停（批量操作入口要看得见）
          className="ui-checkbox"
          aria-label={t("skillsPage.selectAria", { name: skill.name })}
          checked={checked}
          onChange={(event) => onPick(event.target.checked)}
        />
      </span>
      <div className="ms-2 flex min-w-0 flex-1 flex-col pe-3">
        <div className="flex min-w-0 items-center gap-1.5">
          <span
            title={skill.name}
            className="min-w-0 truncate text-body font-medium"
          >
            {skill.name}
          </span>
          {showDir && (
            <span
              title={skill.directory}
              className="min-w-0 shrink truncate font-mono text-caption text-fg-3"
            >
              {skill.directory}
            </span>
          )}
          {hasUpdate && (
            <span className="inline-flex h-[18px] shrink-0 items-center whitespace-nowrap rounded-full border border-transparent bg-warning-soft px-1.5 text-badge font-medium text-warning-text">
              {t("skills.updateAvailable")}
            </span>
          )}
          {!enabled && <NeutralBadge>{t("mcpPage.notEnabled")}</NeutralBadge>}
        </div>
        <div className="flex min-w-0 whitespace-nowrap text-caption text-fg-2">
          {skill.description && (
            <>
              <span className="min-w-0 truncate" title={skill.description}>
                {skill.description}
              </span>
              <span aria-hidden="true" className="shrink-0">
                &nbsp;·&nbsp;
              </span>
            </>
          )}
          {hasRepo ? (
            <button
              type="button"
              onClick={onOpenSource}
              className="shrink-0 underline underline-offset-[3px] hover:text-fg-1"
            >
              {sourceText}
            </button>
          ) : (
            <span className="shrink-0">{sourceText}</span>
          )}
          {failure && failedApp && (
            <>
              <span
                id={failId}
                className="ms-1.5 min-w-0 truncate text-warning-text"
              >
                {t("skillsPage.rowFail", {
                  app: APP_DISPLAY_NAME[failedApp],
                  error: failure.error,
                })}
                &nbsp;·&nbsp;
              </span>
              <button
                type="button"
                aria-describedby={failId}
                onClick={onFixSync}
                className="shrink-0 text-warning-text underline underline-offset-[3px]"
              >
                {t("skillsPage.fixSync")}
              </button>
            </>
          )}
        </div>
      </div>
      <div className="flex shrink-0">
        {appIds.map((app) => {
          const fail = fails[failKey(skill.id, app)];
          const on = Boolean(skill.apps[app]);
          const state = fail ? "fail" : on ? "on" : "off";
          return (
            <MatrixCell
              key={app}
              app={app}
              state={state}
              disabled={disabled}
              label={t(`appMatrix.cell.${state}`, {
                name: skill.name,
                app: APP_DISPLAY_NAME[app],
              })}
              onClick={() => onCell(app)}
            />
          );
        })}
      </div>
      <div className="flex w-16 shrink-0 justify-end">
        <DropdownMenu>
          <HoverTip content={t("common.more")}>
            <DropdownMenuTrigger asChild>
              <Button
                type="button"
                variant="quiet"
                size="icon-compact"
                aria-label={t("skillsPage.rowMoreAria", { name: skill.name })}
                disabled={disabled}
              >
                {isUpdating ? (
                  <Loader2 className="h-[15px] w-[15px] animate-spin" />
                ) : (
                  <MoreHorizontal
                    className="h-[15px] w-[15px]"
                    strokeWidth={1.5}
                  />
                )}
              </Button>
            </DropdownMenuTrigger>
          </HoverTip>
          <DropdownMenuContent align="end" className="min-w-[180px]">
            {hasUpdate && (
              <DropdownMenuItem onSelect={onUpdate}>
                {t("skills.update")}
              </DropdownMenuItem>
            )}
            {(skill.readmeUrl || hasRepo) && (
              <DropdownMenuItem onSelect={onOpenDocs}>
                {t("mcpPage.openDocs")}
              </DropdownMenuItem>
            )}
            <DropdownMenuItem onSelect={onCopyDir}>
              {t("skillsPage.copyDir")}
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              className="text-danger-text focus:text-danger-text"
              onSelect={onUninstall}
            >
              {t("skillsPage.uninstallEllipsis")}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </li>
  );
}

// ─── 列头左侧的筛选按钮 ──────────────────────────────────────────────────
function FilterMenu({
  label,
  active,
  items,
}: {
  label: string;
  active: boolean;
  items: Array<{
    key: string;
    label: string;
    checked: boolean;
    onSelect: () => void;
    count?: number;
    mono?: boolean;
  }>;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="quiet"
          size="compact"
          className={cn(
            "shrink-0 pe-1.5 ps-2.5",
            active ? "text-fg-1" : "text-fg-2",
          )}
        >
          <span className="max-w-[180px] truncate">{label}</span>
          <ChevronDown className="h-3.5 w-3.5" strokeWidth={2} />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="start"
        className="min-w-[210px] max-w-[280px]"
      >
        {items.map((item) => (
          <DropdownMenuItem key={item.key} onSelect={item.onSelect}>
            <span className="flex w-4 shrink-0 justify-center">
              {item.checked && (
                <Check className="h-3.5 w-3.5" strokeWidth={2} />
              )}
            </span>
            <span
              className={cn(
                "min-w-0 flex-1 truncate",
                item.mono && "font-mono text-caption",
              )}
            >
              {item.label}
            </span>
            {item.count !== undefined && (
              <span className="shrink-0 text-caption tabular-nums text-fg-3">
                {item.count}
              </span>
            )}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

// ─── 多选条的「启用到 / 停用」应用菜单 ────────────────────────────────────
function AppMenuButton({
  label,
  ariaLabel,
  apps,
  disabled,
  onPick,
}: {
  label: string;
  ariaLabel: string;
  apps: AppId[];
  disabled: boolean;
  onPick: (app: AppId) => void;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="quiet"
          size="compact"
          className="shrink-0 pe-1.5 ps-2"
          disabled={disabled}
        >
          {label}
          <ChevronDown className="h-3.5 w-3.5" strokeWidth={2} />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="start"
        aria-label={ariaLabel}
        className="min-w-[200px]"
      >
        {apps.map((app) => (
          <DropdownMenuItem key={app} onSelect={() => onPick(app)}>
            <AppGlyph app={app} size={16} badgeClassName="bg-surface" />
            {APP_DISPLAY_NAME[app]}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

// ─── 仓库管理抽屉（打开时才挂载，避免进页面就去读仓库） ─────────────────────
function RepoManagerContainer({ onClose }: { onClose: () => void }) {
  const { data: repos = [] } = useSkillRepos();
  const { data: discoverable = [] } = useDiscoverableSkills();
  const { addRepo, removeRepo, setRepoEnabled } = useSkillRepoActions();
  return (
    <RepoManagerPanel
      repos={repos}
      skills={discoverable}
      onAdd={addRepo}
      onRemove={removeRepo}
      onToggle={setRepoEnabled}
      onClose={onClose}
    />
  );
}

export default UnifiedSkillsPanel;
