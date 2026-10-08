import React, { useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useTranslation } from "react-i18next";
import {
  ChevronDown,
  ChevronRight,
  Download,
  Loader2,
  RefreshCw,
} from "lucide-react";
import { toast } from "@/lib/toast";
import * as PopoverPrimitive from "@radix-ui/react-popover";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { Notice } from "@/components/ui/notice";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { HoverTip } from "@/components/ui/hover-tip";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import {
  useAddSkillRepo,
  useDiscoverableSkills,
  useDiscoverableSkillsFailures,
  useInstallSkill,
  useInstalledSkills,
  useRemoveSkillRepo,
  useSearchSkillsSh,
  useSkillRepos,
} from "@/hooks/useSkills";
import type { AppId } from "@/lib/api/types";
import type {
  DiscoverableSkill,
  InstalledSkill,
  SkillRepo,
  SkillRepoFailure,
  SkillsShDiscoverableSkill,
} from "@/lib/api/skills";
import { SKILLS_APP_IDS } from "@/config/appConfig";
import {
  formatSkillError,
  skillErrorReason,
} from "@/lib/errors/skillErrorParser";
import { cn } from "@/lib/utils";
import { MatrixSearch, NeutralBadge } from "@/components/mcp/AppMatrix";
import { CHECKBOX_CLASS } from "@/components/mcp/formBits";
import { countRepoSkills } from "./RepoManagerPanel";
import { describeRepoFailures, repoFailureKey } from "./repoFailures";

/** 发现列表每行高度（h-14），虚拟化按这个算 */
const ROW_HEIGHT = 56;
export type SkillsPageSource = "repos" | "skillssh";

type InstallRunner = <T extends { id: string }>(
  install: (firstApp: AppId) => Promise<T[]>,
  apps?: AppId[],
) => Promise<{
  installed: T[];
  failures: Array<{ id: string; app: AppId; error: unknown }>;
}>;

const SKILLSSH_PAGE_SIZE = 20;
const POPOVER_CLASS =
  "z-[110] rounded-panel border border-border bg-surface p-1 text-fg-1 shadow-v7-md outline-none data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[side=bottom]:slide-in-from-top-2 data-[side=top]:slide-in-from-bottom-2";

/** 仓库的增删和停用（发现段的「仓库」弹层和仓库管理抽屉共用） */
export function useSkillRepoActions() {
  const { t } = useTranslation();
  const { refetch: refetchDiscoverable } = useDiscoverableSkills();
  const addRepoMutation = useAddSkillRepo();
  const removeRepoMutation = useRemoveSkillRepo();

  const addRepo = async (repo: SkillRepo) => {
    await addRepoMutation.mutateAsync(repo);
    const { data: fresh } = await refetchDiscoverable();
    toast.success(
      t("skills.repo.addSuccess", {
        owner: repo.owner,
        name: repo.name,
        count: countRepoSkills(fresh ?? [], repo),
      }),
      { closeButton: true },
    );
  };

  const removeRepo = async (owner: string, name: string) => {
    try {
      await removeRepoMutation.mutateAsync({ owner, name });
      toast.success(t("skills.repo.removeSuccess", { owner, name }), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  /** 勾掉 = 停用：add_skill_repo 是 INSERT OR REPLACE，带 enabled=false 写回即可 */
  const setRepoEnabled = async (repo: SkillRepo, enabled: boolean) => {
    try {
      await addRepoMutation.mutateAsync({ ...repo, enabled });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  return { addRepo, removeRepo, setRepoEnabled };
}

interface SkillsPageProps {
  /**
   * 「已安装 / 发现」页签（由外层渲染，两段共用一条页签行）；
   * 传入的内容放在页签行右侧（安装到、搜索）。
   */
  renderViewTabs: (trailing: React.ReactNode) => React.ReactNode;
  visibleAppIds: AppId[];
  installTargets: AppId[];
  onInstallTargetsChange: (apps: AppId[]) => void;
  installTo: InstallRunner;
  /** 「已安装 · 3 个应用 ›」：切到已安装段并定位到那一行 */
  onShowInstalled: (skillId: string) => void;
  onOpenRepoManager: () => void;
  /** 安装后有应用没能启用：交给已安装段标 ⚠ */
  onEnableFailures: (
    failures: Array<{ id: string; app: AppId; error: unknown }>,
  ) => void;
  initialQuery?: string;
}

interface DiscoverRow {
  key: string;
  name: string;
  directory: string;
  description: string;
  repoOwner: string;
  repoName: string;
  installs?: number;
  skill: DiscoverableSkill;
}

function installedKey(directory: string, owner?: string, name?: string) {
  const dir = directory.split(/[/\\]/).pop()?.toLowerCase() ?? "";
  return `${dir}:${(owner ?? "").toLowerCase()}:${(name ?? "").toLowerCase()}`;
}

function formatInstalls(count: number, locale: string): string {
  try {
    return new Intl.NumberFormat(locale, {
      notation: "compact",
      maximumFractionDigits: 1,
    }).format(count);
  } catch {
    return String(count);
  }
}

/**
 * Skills 的「发现」段：仓库 / skills.sh 两个来源，结果是行列表；显式「安装到」哪些应用。
 */
export function SkillsPage({
  renderViewTabs,
  visibleAppIds,
  installTargets,
  onInstallTargetsChange,
  installTo,
  onShowInstalled,
  onOpenRepoManager,
  onEnableFailures,
  initialQuery = "",
}: SkillsPageProps) {
  const { t, i18n } = useTranslation();
  const [source, setSource] = useState<SkillsPageSource>("repos");
  const [query, setQuery] = useState(initialQuery);
  const [statusFilter, setStatusFilter] = useState<"all" | "uninstalled">(
    "all",
  );
  const [skillsShQuery, setSkillsShQuery] = useState("");
  const [skillsShOffset, setSkillsShOffset] = useState(0);
  const [accumulated, setAccumulated] = useState<SkillsShDiscoverableSkill[]>(
    [],
  );
  const [installing, setInstalling] = useState<Set<string>>(new Set());
  const [failed, setFailed] = useState<Record<string, string>>({});

  const {
    data: discoverable,
    isLoading: loadingDiscoverable,
    isFetching: fetchingDiscoverable,
    isError: discoverError,
    error: discoverErrorValue,
    refetch: refetchDiscoverable,
  } = useDiscoverableSkills();
  const { data: repoFailures = [] } = useDiscoverableSkillsFailures();
  const { data: installedSkills = [] } = useInstalledSkills();
  const {
    data: repos = [],
    isLoading: loadingRepos,
    refetch: refetchRepos,
  } = useSkillRepos();
  const {
    data: skillsShResult,
    isLoading: loadingSkillsSh,
    isFetching: fetchingSkillsSh,
    isPlaceholderData: placeholderSkillsSh,
    isError: skillsShError,
    refetch: refetchSkillsSh,
  } = useSearchSkillsSh(skillsShQuery, SKILLSSH_PAGE_SIZE, skillsShOffset);
  const installMutation = useInstallSkill();
  const { setRepoEnabled } = useSkillRepoActions();

  useEffect(() => {
    if (skillsShResult && !placeholderSkillsSh) {
      setAccumulated((prev) =>
        skillsShOffset === 0
          ? skillsShResult.skills
          : [...prev, ...skillsShResult.skills],
      );
    }
  }, [skillsShResult, skillsShOffset, placeholderSkillsSh]);

  // 没配置任何仓库时直接看 skills.sh；仓库读出来是空的仍留在仓库，方便重试
  const effectiveSource: SkillsPageSource =
    source === "repos" && !loadingRepos && repos.length === 0
      ? "skillssh"
      : source;
  const isSkillsSh = effectiveSource === "skillssh";
  const enabledRepos = repos.filter((repo) => repo.enabled);

  const submitSkillsSh = () => {
    const trimmed = query.trim();
    if (trimmed.length < 2) return;
    if (trimmed === skillsShQuery && skillsShOffset === 0) return;
    setSkillsShOffset(0);
    setAccumulated([]);
    setSkillsShQuery(trimmed);
  };

  // 第一页失败时 query key 没变，光把 offset 设回 0 什么都不会发生，要显式重新请求
  const retrySkillsSh = () => {
    setAccumulated([]);
    if (skillsShOffset === 0) {
      void refetchSkillsSh();
    } else {
      setSkillsShOffset(0);
    }
  };

  // ─── 已安装 / 目录名占用 ────────────────────────────────────────────
  const installedByKey = useMemo(() => {
    const map = new Map<string, InstalledSkill>();
    for (const skill of installedSkills) {
      map.set(
        installedKey(skill.directory, skill.repoOwner, skill.repoName),
        skill,
      );
    }
    return map;
  }, [installedSkills]);

  const installedByDir = useMemo(() => {
    const map = new Map<string, InstalledSkill>();
    for (const skill of installedSkills) {
      map.set(skill.directory.toLowerCase(), skill);
    }
    return map;
  }, [installedSkills]);

  const enabledAppCount = (skill: InstalledSkill) =>
    SKILLS_APP_IDS.filter((app) => skill.apps[app]).length;

  const sourceLabel = (skill: InstalledSkill) =>
    skill.repoOwner && skill.repoName
      ? `${skill.repoOwner}/${skill.repoName}`
      : t("skillsPage.source.local");

  // ─── 行数据 ─────────────────────────────────────────────────────────
  const repoRows: DiscoverRow[] = useMemo(
    () =>
      (discoverable ?? []).map((skill) => ({
        key: skill.key,
        name: skill.name,
        directory: skill.directory,
        description: skill.description,
        repoOwner: skill.repoOwner,
        repoName: skill.repoName,
        skill,
      })),
    [discoverable],
  );

  const skillsShRows: DiscoverRow[] = useMemo(
    () =>
      accumulated.map((item) => ({
        key: item.key,
        name: item.name,
        directory: item.directory,
        description: "",
        repoOwner: item.repoOwner,
        repoName: item.repoName,
        installs: item.installs,
        skill: {
          key: item.key,
          name: item.name,
          description: "",
          directory: item.directory,
          repoOwner: item.repoOwner,
          repoName: item.repoName,
          repoBranch: item.repoBranch,
          readmeUrl: item.readmeUrl,
        },
      })),
    [accumulated],
  );

  const statusOf = (row: DiscoverRow) => {
    const installed = installedByKey.get(
      installedKey(row.directory, row.repoOwner, row.repoName),
    );
    if (installed) return { kind: "installed" as const, skill: installed };
    const dir = row.directory.split(/[/\\]/).pop()?.toLowerCase() ?? "";
    const owner = installedByDir.get(dir);
    if (owner) return { kind: "conflict" as const, skill: owner };
    return { kind: "available" as const };
  };

  const normalizedQuery = query.trim().toLowerCase();
  const visibleRows = isSkillsSh
    ? skillsShRows
    : repoRows.filter((row) => {
        if (
          statusFilter === "uninstalled" &&
          statusOf(row).kind !== "available"
        )
          return false;
        if (!normalizedQuery) return true;
        return [
          row.name,
          row.directory,
          row.description,
          `${row.repoOwner}/${row.repoName}`,
        ].some((value) => value?.toLowerCase().includes(normalizedQuery));
      });

  // 列表虚拟化：几个仓库加起来上千行，只渲染可视区域和前后各几行（每行固定 56）
  const listScrollRef = useRef<HTMLDivElement>(null);
  const rowVirtualizer = useVirtualizer({
    count: visibleRows.length,
    getScrollElement: () => listScrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    getItemKey: (index) => visibleRows[index]?.key ?? index,
    overscan: 8,
  });

  // ─── 安装 ───────────────────────────────────────────────────────────
  const names = (apps: AppId[]) =>
    apps.map((app) => APP_DISPLAY_NAME[app]).join(t("mcpPage.listSeparator"));

  const handleInstall = async (row: DiscoverRow) => {
    if (installing.has(row.key)) return;
    setInstalling((prev) => new Set(prev).add(row.key));
    setFailed((prev) => {
      const next = { ...prev };
      delete next[row.key];
      return next;
    });
    try {
      const { failures } = await installTo(async (firstApp) => [
        await installMutation.mutateAsync({
          skill: row.skill,
          currentApp: firstApp,
        }),
      ]);
      toast.success(
        t("skillsPage.toast.installed", {
          name: row.name,
          apps: names(installTargets),
        }),
        { closeButton: true },
      );
      if (failures.length) {
        onEnableFailures(failures);
        toast.warning(
          t("skillsPage.toast.enableFailed", {
            apps: names(failures.map((failure) => failure.app)),
          }),
          { closeButton: true },
        );
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      const { title, description } = formatSkillError(
        message,
        t,
        "skills.installFailed",
      );
      setFailed((prev) => ({ ...prev, [row.key]: description || title }));
    } finally {
      setInstalling((prev) => {
        const next = new Set(prev);
        next.delete(row.key);
        return next;
      });
    }
  };

  const reload = () => {
    void refetchDiscoverable();
    void refetchRepos();
  };

  // ─── 渲染 ───────────────────────────────────────────────────────────
  const installToLabel =
    installTargets.length <= 2
      ? names(installTargets)
      : t("skillsPage.installTo.many", {
          first: APP_DISPLAY_NAME[installTargets[0]],
          count: installTargets.length,
        });

  const loadingList = isSkillsSh
    ? (loadingSkillsSh || fetchingSkillsSh) && accumulated.length === 0
    : loadingDiscoverable || (fetchingDiscoverable && !discoverable?.length);

  const hasMoreSkillsSh =
    isSkillsSh &&
    skillsShResult &&
    accumulated.length < skillsShResult.totalCount;

  const renderEmpty = () => {
    let title = "";
    let body = "";
    let buttons: Array<{ label: string; onClick: () => void }> = [];
    if (isSkillsSh) {
      if (skillsShQuery.length < 2) {
        body = t("skillsPage.discover.skillsShHint");
      } else if (skillsShError) {
        title = t("skills.skillssh.error");
        buttons = [{ label: t("common.retry"), onClick: retrySkillsSh }];
      } else {
        title = t("skillsPage.discover.skillsShNone", { query: skillsShQuery });
      }
    } else if (discoverError) {
      title = t("skillsPage.discover.loadFailed");
      body = String(discoverErrorValue ?? "");
      buttons = [
        { label: t("common.retry"), onClick: reload },
        {
          label: t("skillsPage.discover.manageRepos"),
          onClick: onOpenRepoManager,
        },
      ];
    } else if (repoRows.length === 0) {
      // 后端逐仓库报告了失败：如实写哪些没读到、为什么；否则是读到了但里面没有 Skill
      title =
        repoFailures.length > 0
          ? t("skillsPage.discover.loadFailed")
          : t("skillsPage.discover.noneRead");
      body =
        repoFailures.length > 0
          ? t("skillsPage.repoFail.title", {
              count: repoFailures.length,
              repos: describeRepoFailures(repoFailures, t),
            })
          : t("skillsPage.discover.noneReadBody", {
              count: enabledRepos.length,
            });
      buttons = [
        { label: t("common.retry"), onClick: reload },
        {
          label: t("skillsPage.discover.manageRepos"),
          onClick: onOpenRepoManager,
        },
      ];
    } else if (statusFilter === "uninstalled" && !normalizedQuery) {
      title = t("skillsPage.discover.allInstalled");
      buttons = [
        {
          label: t("skillsPage.discover.showAll"),
          onClick: () => setStatusFilter("all"),
        },
      ];
    } else {
      title = t("skillsPage.noMatch", { query: query.trim() });
      buttons = [
        {
          label: t("mcpPage.clearSearch"),
          onClick: () => {
            setQuery("");
            setStatusFilter("all");
          },
        },
      ];
    }
    return (
      <div className="flex flex-col items-center gap-2 px-6 py-10 text-center">
        {title && <h2 className="m-0 text-section">{title}</h2>}
        {body && (
          <p className="m-0 max-w-[460px] text-body text-fg-2">{body}</p>
        )}
        {buttons.length > 0 && (
          <div className="flex flex-wrap justify-center gap-2 pt-1">
            {buttons.map((button) => (
              <Button
                key={button.label}
                type="button"
                variant="neutral"
                size="regular"
                onClick={button.onClick}
              >
                {button.label}
              </Button>
            ))}
          </div>
        )}
      </div>
    );
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* 页签行在外层（UnifiedSkillsPanel），这里只把右侧控件交出去 */}
      {renderViewTabs(
        <div className="flex min-w-0 items-center gap-2">
          <InstallToPopover
            label={installToLabel}
            fullLabel={t("skillsPage.installTo.button", {
              apps: names(installTargets),
            })}
            visibleAppIds={visibleAppIds}
            targets={installTargets}
            onChange={onInstallTargetsChange}
          />
          <MatrixSearch
            className="w-[240px] min-w-[160px] shrink"
            inputId="sk-discover-search"
            value={query}
            onValueChange={setQuery}
            onEnter={isSkillsSh ? submitSkillsSh : undefined}
            placeholder={
              isSkillsSh
                ? t("skillsPage.discover.skillsShPlaceholder")
                : t("skillsPage.discover.searchPlaceholder")
            }
            ariaLabel={
              isSkillsSh
                ? t("skillsPage.discover.skillsShAria")
                : t("skillsPage.discover.searchAria")
            }
            status={
              normalizedQuery && !isSkillsSh
                ? t("appMatrix.found", { count: visibleRows.length })
                : ""
            }
          />
        </div>,
      )}

      {/* 页签下面一行：来源是发现里的模式（分段控件），其余是页内筛选（更轻的样式） */}
      <div className="flex h-12 shrink-0 items-center gap-2 px-6">
        <SegmentedControl<SkillsPageSource>
          aria-label={t("skillsPage.discover.sourceAria")}
          size="sm"
          value={effectiveSource}
          onValueChange={setSource}
          items={[
            {
              value: "repos",
              label: t("skillsPage.discover.sourceRepos", {
                count: enabledRepos.length,
              }),
            },
            {
              value: "skillssh",
              label: "skills.sh",
            },
          ]}
        />
        {!isSkillsSh && (
          <>
            <RepoPopover
              repos={repos}
              discoverable={discoverable ?? []}
              failures={repoFailures}
              onToggle={setRepoEnabled}
              onManage={onOpenRepoManager}
            />
            <label className="flex h-7 shrink-0 cursor-pointer items-center gap-2 rounded-control px-2 text-body text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1">
              <input
                type="checkbox"
                className={CHECKBOX_CLASS}
                checked={statusFilter === "uninstalled"}
                onChange={(event) =>
                  setStatusFilter(event.target.checked ? "uninstalled" : "all")
                }
              />
              {t("skillsPage.discover.onlyUninstalled")}
            </label>
            <div className="flex-1" />
            <HoverTip content={t("skillsPage.discover.reload")}>
              <Button
                type="button"
                variant="quiet"
                size="icon-compact"
                aria-label={t("skillsPage.discover.reload")}
                disabled={fetchingDiscoverable}
                onClick={reload}
              >
                <RefreshCw
                  className={cn(
                    "h-4 w-4",
                    fetchingDiscoverable && "animate-spin",
                  )}
                  strokeWidth={1.5}
                />
              </Button>
            </HoverTip>
          </>
        )}
      </div>

      {/* 部分仓库没读到：列表照常显示读到的，横幅如实说哪些没读到。全部没读到时由空状态说明。 */}
      {!isSkillsSh && repoFailures.length > 0 && repoRows.length > 0 && (
        <div className="px-6 pb-3">
          <Notice
            tone="warning"
            title={t("skillsPage.repoFail.title", {
              count: repoFailures.length,
              repos: describeRepoFailures(repoFailures, t),
            })}
            actions={
              <Button
                type="button"
                variant="neutral"
                size="compact"
                disabled={fetchingDiscoverable}
                onClick={reload}
              >
                {t("common.retry")}
              </Button>
            }
          >
            {t("skillsPage.repoFail.discoverBody")}
          </Notice>
        </div>
      )}

      <div className="flex min-h-0 flex-1 flex-col px-6 pb-5">
        <div
          ref={listScrollRef}
          className="min-h-0 overflow-auto scroll-stable rounded-panel border border-border bg-surface"
        >
          {loadingList ? (
            <div className="flex items-center justify-center gap-2 py-16 text-body text-fg-2">
              <Loader2 className="h-4 w-4 animate-spin" />
              {isSkillsSh ? t("skills.skillssh.loading") : t("skills.loading")}
            </div>
          ) : visibleRows.length === 0 ? (
            renderEmpty()
          ) : (
            <ul
              aria-label={t("skillsPage.discover.listLabel")}
              className="relative m-0 min-w-[520px] list-none p-0"
              style={{ height: rowVirtualizer.getTotalSize() }}
            >
              {rowVirtualizer.getVirtualItems().map((item) => {
                const index = item.index;
                const row = visibleRows[index];
                const status = statusOf(row);
                const isInstalling = installing.has(row.key);
                const failure = failed[row.key];
                const line2Id = `sk-d-${index}-line2`;
                const repo = `${row.repoOwner}/${row.repoName}`;
                const showDir =
                  row.directory &&
                  row.directory.trim().toLowerCase() !==
                    row.name.trim().toLowerCase();
                return (
                  <li
                    key={item.key}
                    aria-setsize={visibleRows.length}
                    aria-posinset={index + 1}
                    className={cn(
                      "absolute inset-x-0 top-0 flex h-14 items-center gap-3 pe-3 ps-4",
                      index > 0 && "border-t border-border",
                    )}
                    style={{ transform: `translateY(${item.start}px)` }}
                  >
                    <div className="flex min-w-0 flex-1 flex-col">
                      <div className="flex min-w-0 items-center gap-1.5">
                        <span className="min-w-0 truncate text-body font-medium">
                          {row.name}
                        </span>
                        {showDir && (
                          <span
                            title={row.directory}
                            className="min-w-0 shrink truncate font-mono text-caption text-fg-3"
                          >
                            {row.directory}
                          </span>
                        )}
                        {!isSkillsSh && (
                          <NeutralBadge mono>{repo}</NeutralBadge>
                        )}
                        {typeof row.installs === "number" && (
                          <span
                            className="inline-flex shrink-0 items-center gap-0.5 text-caption text-fg-2"
                            title={t("skills.skillssh.installs", {
                              count: row.installs,
                            })}
                          >
                            <Download
                              aria-hidden="true"
                              className="h-3 w-3"
                              strokeWidth={2}
                            />
                            {formatInstalls(row.installs, i18n.language)}
                          </span>
                        )}
                      </div>
                      <span
                        id={line2Id}
                        className={cn(
                          "truncate text-caption",
                          failure ? "text-danger-text" : "text-fg-2",
                        )}
                        title={failure ?? row.description}
                      >
                        {failure
                          ? t("skillsPage.discover.installFailed", {
                              reason: failure,
                            })
                          : isSkillsSh
                            ? `skills.sh · ${repo}`
                            : row.description}
                      </span>
                    </div>
                    <div className="flex w-[180px] shrink-0 justify-end">
                      {status.kind === "installed" ? (
                        <button
                          type="button"
                          onClick={() => onShowInstalled(status.skill.id)}
                          aria-label={t("skillsPage.discover.installedAria", {
                            name: row.name,
                            count: enabledAppCount(status.skill),
                          })}
                          className="inline-flex h-7 items-center gap-0.5 rounded-control px-2 text-body text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1"
                        >
                          {t("skillsPage.discover.installed", {
                            count: enabledAppCount(status.skill),
                          })}
                          <ChevronRight
                            className="h-3.5 w-3.5"
                            strokeWidth={2}
                          />
                        </button>
                      ) : status.kind === "conflict" ? (
                        <button
                          type="button"
                          onClick={() => onShowInstalled(status.skill.id)}
                          className="inline-flex min-w-0 items-center gap-0.5 rounded-control px-1 text-right text-caption text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1"
                        >
                          <span className="min-w-0 truncate">
                            {t("skillsPage.discover.dirTaken", {
                              name: status.skill.name,
                              source: sourceLabel(status.skill),
                            })}
                          </span>
                          <ChevronRight
                            className="h-3.5 w-3.5 shrink-0"
                            strokeWidth={2}
                          />
                        </button>
                      ) : isInstalling ? (
                        <Button
                          type="button"
                          variant="neutral"
                          size="compact"
                          aria-disabled="true"
                          aria-label={t("skillsPage.discover.installingAria", {
                            name: row.name,
                          })}
                          className="cursor-not-allowed ps-2.5"
                        >
                          <Loader2 className="h-3.5 w-3.5 animate-spin" />
                          {t("skillsPage.discover.installing")}
                        </Button>
                      ) : (
                        <Button
                          type="button"
                          variant="neutral"
                          size="compact"
                          disabled={!row.repoOwner}
                          aria-label={
                            failure
                              ? t("skillsPage.discover.retryAria", {
                                  name: row.name,
                                })
                              : t("skillsPage.discover.installAria", {
                                  name: row.name,
                                })
                          }
                          aria-describedby={failure ? line2Id : undefined}
                          onClick={() => void handleInstall(row)}
                        >
                          {failure ? t("common.retry") : t("skills.install")}
                        </Button>
                      )}
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
        {isSkillsSh && accumulated.length > 0 && (
          <div className="mt-3 flex shrink-0 flex-col items-center gap-2">
            {hasMoreSkillsSh && (
              <Button
                type="button"
                variant="neutral"
                size="compact"
                disabled={fetchingSkillsSh}
                onClick={() =>
                  setSkillsShOffset((prev) => prev + SKILLSSH_PAGE_SIZE)
                }
              >
                {fetchingSkillsSh && (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                )}
                {t("skills.skillssh.loadMore")}
              </Button>
            )}
            <span className="text-caption text-fg-3">
              {t("skills.skillssh.poweredBy")}
            </span>
          </div>
        )}
      </div>
    </div>
  );
}

// ─── 「安装到」弹层 ──────────────────────────────────────────────────────
function InstallToPopover({
  label,
  fullLabel,
  visibleAppIds,
  targets,
  onChange,
}: {
  label: string;
  fullLabel: string;
  visibleAppIds: AppId[];
  targets: AppId[];
  onChange: (apps: AppId[]) => void;
}) {
  const { t } = useTranslation();
  const apps = SKILLS_APP_IDS.filter((app) => visibleAppIds.includes(app));
  return (
    <PopoverPrimitive.Root>
      <PopoverPrimitive.Trigger asChild>
        <Button
          type="button"
          variant="neutral"
          size="regular"
          title={fullLabel}
          className="min-w-0 shrink pe-2.5"
        >
          <span className="min-w-0 truncate">
            {t("skillsPage.installTo.prefix")}
            {label}
          </span>
          <ChevronDown className="h-3.5 w-3.5 shrink-0" strokeWidth={2} />
        </Button>
      </PopoverPrimitive.Trigger>
      <PopoverPrimitive.Portal>
        <PopoverPrimitive.Content
          align="end"
          sideOffset={6}
          collisionPadding={8}
          className={cn(POPOVER_CLASS, "flex w-[280px] flex-col gap-2.5 p-3")}
        >
          <div className="flex items-center gap-0.5">
            <h3 className="m-0 text-caption font-semibold text-fg-2">
              {t("skillsPage.installTo.title")}
            </h3>
            <HelpTip title={t("skillsPage.installTo.helpTitle")}>
              {t("skillsPage.installTo.help")}
            </HelpTip>
          </div>
          <div className="grid grid-cols-2 gap-x-3 gap-y-2">
            {apps.map((app) => {
              const checked = targets.includes(app);
              return (
                <label
                  key={app}
                  className="flex h-6 cursor-pointer items-center gap-2 text-body"
                >
                  <input
                    type="checkbox"
                    className={CHECKBOX_CLASS}
                    checked={checked}
                    onChange={(event) => {
                      const next = event.target.checked
                        ? [...targets, app]
                        : targets.filter((item) => item !== app);
                      // 至少留一个：后端安装必须落到某个应用
                      if (next.length) onChange(next);
                    }}
                  />
                  <AppGlyph app={app} size={16} badgeClassName="bg-surface" />
                  <span className="truncate">{APP_DISPLAY_NAME[app]}</span>
                </label>
              );
            })}
          </div>
        </PopoverPrimitive.Content>
      </PopoverPrimitive.Portal>
    </PopoverPrimitive.Root>
  );
}

// ─── 「仓库」弹层：勾掉即停用 ────────────────────────────────────────────
function RepoPopover({
  repos,
  discoverable,
  failures,
  onToggle,
  onManage,
}: {
  repos: SkillRepo[];
  discoverable: DiscoverableSkill[];
  failures: SkillRepoFailure[];
  onToggle: (repo: SkillRepo, enabled: boolean) => Promise<void>;
  onManage: () => void;
}) {
  const { t } = useTranslation();
  const failureByRepo = new Map(
    failures.map((failure) => [repoFailureKey(failure), failure]),
  );
  const [open, setOpen] = useState(false);
  const enabled = repos.filter((repo) => repo.enabled).length;
  const label =
    enabled === repos.length
      ? t("skillsPage.repos.buttonAll")
      : t("skillsPage.repos.buttonSome", {
          count: enabled,
          total: repos.length,
        });
  return (
    <PopoverPrimitive.Root open={open} onOpenChange={setOpen}>
      <PopoverPrimitive.Trigger asChild>
        <Button
          type="button"
          variant="quiet"
          size="compact"
          className="pe-1.5 ps-2.5 text-fg-2"
        >
          {label}
          <ChevronDown className="h-3.5 w-3.5" strokeWidth={2} />
        </Button>
      </PopoverPrimitive.Trigger>
      <PopoverPrimitive.Portal>
        <PopoverPrimitive.Content
          align="start"
          sideOffset={6}
          collisionPadding={8}
          aria-label={t("skillsPage.repos.popoverAria")}
          className={cn(POPOVER_CLASS, "flex w-[300px] flex-col")}
        >
          <div className="flex max-h-[280px] flex-col overflow-y-auto">
            {repos.map((repo) => {
              const id = `${repo.owner}/${repo.name}`;
              const failure = repo.enabled
                ? failureByRepo.get(repoFailureKey(repo))
                : undefined;
              return (
                <label
                  key={id}
                  className="flex min-h-8 cursor-pointer items-center gap-2 rounded-control px-2 py-1 transition-colors hover:bg-subtle"
                >
                  <input
                    type="checkbox"
                    className={CHECKBOX_CLASS}
                    checked={repo.enabled}
                    aria-label={t("skillsPage.repos.enableAria", { repo: id })}
                    onChange={(event) =>
                      void onToggle(repo, event.target.checked)
                    }
                  />
                  <span className="min-w-0 flex-1 truncate font-mono text-caption">
                    {id}
                  </span>
                  <span className="shrink-0 text-caption text-fg-3">
                    {repo.branch || "main"}
                  </span>
                  {failure ? (
                    <span
                      className="shrink-0 text-right text-caption text-warning-text"
                      title={skillErrorReason(failure.error, t)}
                    >
                      {t("skillsPage.repos.readFailed")}
                    </span>
                  ) : (
                    <span className="w-12 shrink-0 text-right text-caption tabular-nums text-fg-2">
                      {repo.enabled
                        ? countRepoSkills(discoverable, repo)
                        : t("skillsPage.repos.disabledShort")}
                    </span>
                  )}
                </label>
              );
            })}
          </div>
          <div aria-hidden="true" className="mx-1.5 my-1 h-px bg-border" />
          <button
            type="button"
            onClick={() => {
              setOpen(false);
              onManage();
            }}
            className="flex h-8 items-center rounded-control px-2.5 text-left text-body transition-colors hover:bg-subtle"
          >
            {t("skillsPage.repos.manage")}
          </button>
        </PopoverPrimitive.Content>
      </PopoverPrimitive.Portal>
    </PopoverPrimitive.Root>
  );
}
