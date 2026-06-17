import {
  useMemo,
  useEffect,
  forwardRef,
  useImperativeHandle,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Check,
  Loader2,
  MoreHorizontal,
  RefreshCw,
  Search,
  X,
} from "lucide-react";
import { toast } from "sonner";
import { SkillCard } from "./SkillCard";
import { RepoManagerPanel } from "./RepoManagerPanel";
import {
  useDiscoverableSkills,
  useInstalledSkills,
  useInstallSkill,
  useSkillRepos,
  useAddSkillRepo,
  useRemoveSkillRepo,
  useRetrySkillRepo,
  useSearchSkillsSh,
} from "@/hooks/useSkills";
import type { AppId } from "@/lib/api/types";
import type {
  DiscoverableSkill,
  SkillRepo,
  SkillsShDiscoverableSkill,
  SkillRepoFetchFailure,
} from "@/lib/api/skills";
import {
  formatSkillError,
  formatSkillRepoFailure,
  getSkillRepoFailureReasonKey,
} from "@/lib/errors/skillErrorParser";
import {
  beginSkillDiscovery,
  failSkillDiscovery,
  getSkillDiscoveryTaskSnapshot,
  useSkillDiscoveryTask,
} from "@/stores/skillDiscoveryTask";

interface SkillsPageProps {
  initialApp?: AppId;
}

export interface SkillsPageHandle {
  refresh: (force?: boolean) => Promise<void>;
  openRepoManager: () => void;
}

type SearchSource = "repos" | "skillssh";

const SKILLSSH_PAGE_SIZE = 20;
const REPO_SKILL_BATCH_SIZE = 48;

/**
 * Skills 发现面板
 * 用于浏览和安装来自仓库或 skills.sh 的 Skills
 */
export const SkillsPage = forwardRef<SkillsPageHandle, SkillsPageProps>(
  ({ initialApp = "claude" }, ref) => {
    const { t } = useTranslation();
    const [repoManagerOpen, setRepoManagerOpen] = useState(false);
    const [searchQuery, setSearchQuery] = useState("");
    const [filterRepo, setFilterRepo] = useState<string>("all");
    const [filterStatus, setFilterStatus] = useState<
      "all" | "installed" | "uninstalled"
    >("all");
    const [repoSkillLimit, setRepoSkillLimit] = useState(REPO_SKILL_BATCH_SIZE);

    // skills.sh 搜索状态
    const [searchSource, setSearchSource] = useState<SearchSource>("repos");
    const [skillsShInput, setSkillsShInput] = useState("");
    const [skillsShQuery, setSkillsShQuery] = useState("");
    const [skillsShOffset, setSkillsShOffset] = useState(0);
    const [accumulatedResults, setAccumulatedResults] = useState<
      SkillsShDiscoverableSkill[]
    >([]);

    // currentApp 用于安装时的默认应用
    const currentApp = initialApp;

    // Queries
    const {
      data: discoveryResult,
      isLoading: loadingDiscoverable,
      isFetching: fetchingDiscoverable,
      isError: discoveryFailed,
      error: discoveryError,
      refetch: refetchDiscoverable,
      forceRefetch: forceRefetchDiscoverable,
    } = useDiscoverableSkills();
    const discoverableSkills = discoveryResult?.skills ?? [];
    const { data: installedSkills } = useInstalledSkills();
    const { data: repos = [], isLoading: loadingRepos } = useSkillRepos();

    // skills.sh 搜索
    const {
      data: skillsShResult,
      isLoading: loadingSkillsSh,
      isFetching: fetchingSkillsSh,
    } = useSearchSkillsSh(skillsShQuery, SKILLSSH_PAGE_SIZE, skillsShOffset);

    // 当搜索结果返回时累积
    useEffect(() => {
      if (skillsShResult) {
        if (skillsShOffset === 0) {
          setAccumulatedResults(skillsShResult.skills);
        } else {
          setAccumulatedResults((prev) => [...prev, ...skillsShResult.skills]);
        }
      }
    }, [skillsShResult, skillsShOffset]);

    // 手动提交搜索
    const handleSkillsShSearch = () => {
      const trimmed = skillsShInput.trim();
      if (trimmed.length < 2) return;
      setSkillsShOffset(0);
      setAccumulatedResults([]);
      setSkillsShQuery(trimmed);
    };

    // Mutations
    const installMutation = useInstallSkill();
    const addRepoMutation = useAddSkillRepo();
    const removeRepoMutation = useRemoveSkillRepo();
    const retryRepoMutation = useRetrySkillRepo();
    const [retryingRepos, setRetryingRepos] = useState<Set<string>>(
      () => new Set(),
    );
    const discoveryTask = useSkillDiscoveryTask();
    const initialConnectionStartedAt = useRef(Date.now());
    const [initialContentReady, setInitialContentReady] = useState(
      () =>
        discoverableSkills.length > 0 ||
        Object.values(discoveryTask.repositories).some(
          (progress) => (progress.skills?.length ?? 0) > 0,
        ),
    );

    // 已安装的 skill key 集合（使用 directory + repoOwner + repoName 组合判断）
    const installedKeys = useMemo(() => {
      if (!installedSkills) return new Set<string>();
      return new Set(
        installedSkills.map((s) => {
          // 构建唯一 key：directory + repoOwner + repoName
          const owner = s.repoOwner?.toLowerCase() || "";
          const name = s.repoName?.toLowerCase() || "";
          return `${s.directory.toLowerCase()}:${owner}:${name}`;
        }),
      );
    }, [installedSkills]);

    type DiscoverableSkillItem = DiscoverableSkill & { installed: boolean };

    // 从已添加仓库中提取所有仓库选项，避免网络失败时仓库从筛选项消失
    const repoOptions = useMemo(() => {
      return repos
        .filter((repo) => repo.enabled)
        .map((repo) => `${repo.owner}/${repo.name}`)
        .sort();
    }, [repos]);
    const repoOptionSet = useMemo(() => new Set(repoOptions), [repoOptions]);

    const visibleDiscoverableSkills = useMemo(() => {
      const merged = new Map(
        discoverableSkills
          .filter(
            (skill) =>
              loadingRepos ||
              repoOptionSet.has(`${skill.repoOwner}/${skill.repoName}`),
          )
          .map((skill) => [skill.key, skill] as const),
      );
      Object.entries(discoveryTask.repositories)
        .filter(([repo]) => loadingRepos || repoOptionSet.has(repo))
        .map(([, progress]) => progress)
        .flatMap((progress) => progress.skills ?? [])
        .forEach((skill) => merged.set(skill.key, skill));
      return Array.from(merged.values());
    }, [
      discoverableSkills,
      discoveryTask.repositories,
      loadingRepos,
      repoOptionSet,
    ]);

    useEffect(() => {
      if (filterRepo !== "all" && !repoOptionSet.has(filterRepo)) {
        setFilterRepo("all");
      }
    }, [filterRepo, repoOptionSet]);

    // 为发现列表补齐 installed 状态，供 SkillCard 使用
    const skills: DiscoverableSkillItem[] = useMemo(() => {
      return visibleDiscoverableSkills.map((d) => {
        // 同时处理 / 和 \ 路径分隔符（兼容 Windows 和 Unix）
        const installName =
          d.directory.split(/[/\\]/).pop()?.toLowerCase() ||
          d.directory.toLowerCase();
        // 使用 directory + repoOwner + repoName 组合判断是否已安装
        const key = `${installName}:${d.repoOwner.toLowerCase()}:${d.repoName.toLowerCase()}`;
        return {
          ...d,
          installed: installedKeys.has(key),
        };
      });
    }, [installedKeys, visibleDiscoverableSkills]);

    // 检查 skills.sh 结果的安装状态
    const isSkillsShInstalled = (skill: SkillsShDiscoverableSkill): boolean => {
      const key = `${skill.directory.toLowerCase()}:${skill.repoOwner.toLowerCase()}:${skill.repoName.toLowerCase()}`;
      return installedKeys.has(key);
    };

    const loading =
      searchSource === "repos"
        ? loadingDiscoverable || fetchingDiscoverable || discoveryTask.active
        : false;
    const discoveryRepoProgress = discoveryTask.repositories;

    useEffect(() => {
      if (initialContentReady) return;

      if (skills.length > 0) {
        const elapsed = Date.now() - initialConnectionStartedAt.current;
        const timeoutId = window.setTimeout(
          () => setInitialContentReady(true),
          Math.max(0, 800 - elapsed),
        );
        return () => window.clearTimeout(timeoutId);
      }

      if (discoveryFailed) {
        setInitialContentReady(true);
        return;
      }

      if (
        discoveryResult !== undefined &&
        !loadingRepos &&
        !loading &&
        !discoveryTask.active
      ) {
        setInitialContentReady(true);
      }
    }, [
      discoveryResult,
      discoveryFailed,
      discoveryTask.active,
      initialContentReady,
      loading,
      loadingRepos,
      skills.length,
    ]);

    const renderFailures = (failures: SkillRepoFetchFailure[]) => (
      <div className="space-y-1">
        {failures.map((failure) => (
          <div
            data-repo-failure
            key={`${failure.owner}/${failure.name}@${failure.branch}`}
          >
            {formatSkillRepoFailure(failure, t)}
          </div>
        ))}
      </div>
    );

    const getDiscoveryStatus = (repo: string) => {
      const progress = discoveryRepoProgress[repo];
      if (!progress) {
        return {
          icon: <MoreHorizontal className="h-4 w-4 text-muted-foreground" />,
          title: t("skills.discoveryStatus.waiting"),
        };
      }
      switch (progress.phase) {
        case "loading":
        case "scanning":
          return {
            icon: <Loader2 className="h-4 w-4 animate-spin text-blue-500" />,
            title: t(
              progress.phase === "loading"
                ? "skills.discoveryStatus.loading"
                : "skills.discoveryStatus.scanning",
            ),
          };
        case "completed":
          return {
            icon: <Check className="h-4 w-4 text-emerald-500" />,
            title: t("skills.discoveryStatus.completed", {
              count: progress.skillCount ?? 0,
            }),
          };
        case "failed":
          return {
            icon: <X className="h-4 w-4 text-red-500" />,
            title: t(getSkillRepoFailureReasonKey(progress.error ?? "")),
          };
      }
    };

    const handleRefresh = async (force = false) => {
      if (getSkillDiscoveryTaskSnapshot().active) return;
      setRepoSkillLimit(REPO_SKILL_BATCH_SIZE);
      beginSkillDiscovery();
      try {
        const discoverResult = force
          ? await forceRefetchDiscoverable()
          : await refetchDiscoverable();
        if ("error" in discoverResult && discoverResult.error) {
          throw discoverResult.error;
        }
        const failures = discoverResult.data?.failures ?? [];
        if (failures.length > 0) {
          toast.error(t("skills.discoveryPartialFailed"), {
            description: renderFailures(failures),
            closeButton: true,
            duration: Infinity,
          });
        }
      } catch (error) {
        failSkillDiscovery();
        const errorMessage =
          error instanceof Error ? error.message : String(error);
        const { title, description } = formatSkillError(
          errorMessage,
          t,
          "skills.loadFailed",
        );
        toast.error(title, {
          description,
          closeButton: true,
          duration: Infinity,
        });
      }
    };

    useImperativeHandle(ref, () => ({
      refresh: handleRefresh,
      openRepoManager: () => setRepoManagerOpen(true),
    }));

    // skills.sh 结果转为 DiscoverableSkill（复用现有安装流程）
    const toDiscoverableSkill = (
      s: SkillsShDiscoverableSkill,
    ): DiscoverableSkill => ({
      key: s.key,
      name: s.name,
      description: "",
      directory: s.directory,
      repoOwner: s.repoOwner,
      repoName: s.repoName,
      repoBranch: s.repoBranch,
      readmeUrl: s.readmeUrl,
    });

    const handleInstall = async (key: string) => {
      let skill: DiscoverableSkill | undefined;

      if (searchSource === "skillssh") {
        const found = accumulatedResults.find((s) => s.key === key);
        if (found) {
          skill = toDiscoverableSkill(found);
        }
      } else {
        skill = visibleDiscoverableSkills.find((s) => s.key === key);
      }

      if (!skill) {
        toast.error(t("skills.notFound"));
        return;
      }

      try {
        await installMutation.mutateAsync({
          skill,
          currentApp,
        });
        toast.success(t("skills.installSuccess", { name: skill.name }), {
          closeButton: true,
        });
      } catch (error) {
        const errorMessage =
          error instanceof Error ? error.message : String(error);
        const { title, description } = formatSkillError(
          errorMessage,
          t,
          "skills.installFailed",
        );
        toast.error(title, {
          description,
          duration: 10000,
        });
        console.error("Install skill failed:", error);
      }
    };

    const handleUninstall = async (_directory: string) => {
      // 在发现面板中，不支持卸载，需要在主面板中操作
      toast.info(t("skills.uninstallInMainPanel"));
    };

    const handleAddRepo = async (repo: SkillRepo) => {
      try {
        await addRepoMutation.mutateAsync(repo);
        const freshDiscovery = await retryRepoMutation.mutateAsync(repo);
        if (freshDiscovery.failures.length > 0) {
          toast.error(
            t("skills.repo.addDiscoveryFailed", {
              owner: repo.owner,
              name: repo.name,
            }),
            {
              description: renderFailures(freshDiscovery.failures),
              closeButton: true,
              duration: Infinity,
            },
          );
          return;
        }
        const count = freshDiscovery.skills.length;
        toast.success(
          t("skills.repo.addSuccess", {
            owner: repo.owner,
            name: repo.name,
            count,
          }),
          { closeButton: true },
        );
      } catch (error) {
        toast.error(t("common.error"), {
          description: String(error),
        });
      }
    };

    const handleRemoveRepo = async (owner: string, name: string) => {
      try {
        await removeRepoMutation.mutateAsync({ owner, name });
        toast.success(t("skills.repo.removeSuccess", { owner, name }), {
          closeButton: true,
        });
      } catch (error) {
        toast.error(t("common.error"), {
          description: String(error),
        });
      }
    };

    const handleRetryRepo = async (repo: SkillRepo) => {
      const repoKey = `${repo.owner}/${repo.name}@${repo.branch || "main"}`;
      setRetryingRepos((current) => new Set(current).add(repoKey));
      try {
        const result = await retryRepoMutation.mutateAsync(repo);
        if (result.failures.length > 0) {
          toast.error(t("skills.repo.retryFailed"), {
            description: renderFailures(result.failures),
            closeButton: true,
            duration: Infinity,
          });
          return;
        }

        toast.success(
          t("skills.repo.retrySuccess", {
            owner: repo.owner,
            name: repo.name,
            count: result.skills.length,
          }),
          { closeButton: true },
        );
      } catch {
        toast.error(t("skills.repo.retryFailed"), {
          description: t("skills.repo.failureReason.unknown"),
          closeButton: true,
          duration: Infinity,
        });
      } finally {
        setRetryingRepos((current) => {
          const next = new Set(current);
          next.delete(repoKey);
          return next;
        });
      }
    };

    // 过滤技能列表（仓库模式）
    const filteredSkills = useMemo(() => {
      // 按仓库筛选
      const byRepo = skills.filter((skill) => {
        if (filterRepo === "all") return true;
        const skillRepo = `${skill.repoOwner}/${skill.repoName}`;
        return skillRepo === filterRepo;
      });

      // 按安装状态筛选
      const byStatus = byRepo.filter((skill) => {
        if (filterStatus === "installed") return skill.installed;
        if (filterStatus === "uninstalled") return !skill.installed;
        return true;
      });

      // 按搜索关键词筛选
      if (!searchQuery.trim()) return byStatus;

      const query = searchQuery.toLowerCase();
      return byStatus.filter((skill) => {
        const name = skill.name?.toLowerCase() || "";
        const repo =
          skill.repoOwner && skill.repoName
            ? `${skill.repoOwner}/${skill.repoName}`.toLowerCase()
            : "";

        return name.includes(query) || repo.includes(query);
      });
    }, [skills, searchQuery, filterRepo, filterStatus]);
    const displayedRepoSkills = useMemo(
      () => filteredSkills.slice(0, repoSkillLimit),
      [filteredSkills, repoSkillLimit],
    );

    // 是否有更多 skills.sh 结果
    const hasMoreSkillsSh =
      skillsShResult && accumulatedResults.length < skillsShResult.totalCount;

    // 无已添加仓库时默认切换到 skills.sh；有仓库但加载失败/识别为空时仍留在仓库模式
    const effectiveSource =
      searchSource === "repos" &&
      repos.length === 0 &&
      !loadingRepos &&
      discoveryResult !== undefined &&
      !loading
        ? "skillssh"
        : searchSource;

    const showInitialConnection =
      effectiveSource === "repos" &&
      !initialContentReady &&
      !discoveryFailed &&
      (loadingRepos ||
        discoveryResult === undefined ||
        loading ||
        discoveryTask.active ||
        skills.length > 0);

    return (
      <div className="px-6 flex flex-col flex-1 min-h-0 overflow-hidden bg-background/50">
        {/* 技能网格（可滚动详情区域） */}
        <div className="flex-1 overflow-y-auto overflow-x-hidden animate-fade-in">
          <div className="py-4">
            {/* 搜索来源切换 + 搜索框 */}
            <div className="mb-6 flex flex-col gap-3 md:flex-row md:items-center">
              {/* 来源切换 */}
              <div className="inline-flex gap-1 rounded-md border border-border-default bg-background p-1 shrink-0">
                <Button
                  type="button"
                  size="sm"
                  variant={effectiveSource === "repos" ? "default" : "ghost"}
                  className={
                    effectiveSource === "repos"
                      ? "shadow-sm min-w-[64px]"
                      : "text-muted-foreground hover:text-foreground hover:bg-muted min-w-[64px]"
                  }
                  onClick={() => setSearchSource("repos")}
                >
                  {t("skills.searchSource.repos")}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant={effectiveSource === "skillssh" ? "default" : "ghost"}
                  className={
                    effectiveSource === "skillssh"
                      ? "shadow-sm min-w-[80px]"
                      : "text-muted-foreground hover:text-foreground hover:bg-muted min-w-[80px]"
                  }
                  onClick={() => setSearchSource("skillssh")}
                >
                  skills.sh
                </Button>
              </div>

              {effectiveSource === "repos" ? (
                <>
                  {/* 仓库模式搜索框 */}
                  <div className="relative flex-1 min-w-0">
                    <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                    <Input
                      type="text"
                      placeholder={t("skills.searchPlaceholder")}
                      value={searchQuery}
                      onChange={(e) => {
                        setSearchQuery(e.target.value);
                        setRepoSkillLimit(REPO_SKILL_BATCH_SIZE);
                      }}
                      className="pl-9 pr-3"
                    />
                  </div>
                  {/* 仓库筛选 */}
                  <div className="w-full md:w-56">
                    <Select
                      value={filterRepo}
                      onValueChange={(value) => {
                        setFilterRepo(value);
                        setRepoSkillLimit(REPO_SKILL_BATCH_SIZE);
                      }}
                    >
                      <SelectTrigger className="bg-card border shadow-sm text-foreground">
                        <SelectValue
                          placeholder={t("skills.filter.repo")}
                          className="text-left truncate"
                        >
                          {filterRepo === "all"
                            ? t("skills.filter.allRepos")
                            : filterRepo}
                        </SelectValue>
                      </SelectTrigger>
                      <SelectContent className="bg-card text-foreground shadow-lg max-h-64 min-w-[var(--radix-select-trigger-width)]">
                        <SelectItem value="all" className="text-left pr-3">
                          {t("skills.filter.allRepos")}
                        </SelectItem>
                        {repoOptions.map((repo) => (
                          <SelectItem
                            key={repo}
                            value={repo}
                            className="text-left pr-3"
                            title={repo}
                          >
                            <span className="flex w-full items-center justify-between gap-3">
                              <span className="truncate block max-w-[180px]">
                                {repo}
                              </span>
                              <span
                                className="shrink-0"
                                title={getDiscoveryStatus(repo).title}
                              >
                                {getDiscoveryStatus(repo).icon}
                              </span>
                            </span>
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                  {/* 安装状态筛选 */}
                  <div className="w-full md:w-36">
                    <Select
                      value={filterStatus}
                      onValueChange={(val) => {
                        setFilterStatus(
                          val as "all" | "installed" | "uninstalled",
                        );
                        setRepoSkillLimit(REPO_SKILL_BATCH_SIZE);
                      }}
                    >
                      <SelectTrigger className="bg-card border shadow-sm text-foreground">
                        <SelectValue
                          placeholder={t("skills.filter.placeholder")}
                          className="text-left"
                        />
                      </SelectTrigger>
                      <SelectContent className="bg-card text-foreground shadow-lg">
                        <SelectItem value="all" className="text-left pr-3">
                          {t("skills.filter.all")}
                        </SelectItem>
                        <SelectItem
                          value="installed"
                          className="text-left pr-3"
                        >
                          {t("skills.filter.installed")}
                        </SelectItem>
                        <SelectItem
                          value="uninstalled"
                          className="text-left pr-3"
                        >
                          {t("skills.filter.uninstalled")}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  {searchQuery && (
                    <p className="mt-2 text-sm text-muted-foreground">
                      {t("skills.count", { count: filteredSkills.length })}
                    </p>
                  )}
                </>
              ) : (
                <>
                  {/* skills.sh 搜索框 */}
                  <div className="relative flex-1 min-w-0">
                    <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                    <Input
                      type="text"
                      placeholder={t("skills.skillssh.searchPlaceholder")}
                      value={skillsShInput}
                      onChange={(e) => setSkillsShInput(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") handleSkillsShSearch();
                      }}
                      className="pl-9 pr-3"
                    />
                  </div>
                  <Button
                    size="sm"
                    onClick={handleSkillsShSearch}
                    disabled={
                      skillsShInput.trim().length < 2 || fetchingSkillsSh
                    }
                    className="shrink-0"
                  >
                    {fetchingSkillsSh ? (
                      <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
                    ) : (
                      <Search className="h-3.5 w-3.5 mr-1.5" />
                    )}
                    {t("skills.search")}
                  </Button>
                </>
              )}
            </div>

            {/* 内容区域 */}
            {effectiveSource === "repos" ? (
              /* ===== 仓库模式 ===== */
              showInitialConnection ? (
                <div
                  className="flex flex-col items-center justify-center h-64 text-center"
                  role="status"
                >
                  <RefreshCw className="h-8 w-8 animate-spin text-muted-foreground" />
                  <p className="mt-4 text-sm font-medium text-foreground">
                    {repos.length > 0
                      ? t("skills.discoveryInitialConnectingCount", {
                          count: repos.filter((repo) => repo.enabled).length,
                        })
                      : t("skills.discoveryInitialConnecting")}
                  </p>
                  {repoOptions.length > 0 && (
                    <div className="mt-3 space-y-1 text-xs text-muted-foreground">
                      {repoOptions.slice(0, 3).map((repo) => (
                        <div key={repo}>{repo}</div>
                      ))}
                      {repoOptions.length > 3 && (
                        <div>
                          {t("skills.discoveryMoreRepositories", {
                            count: repoOptions.length - 3,
                          })}
                        </div>
                      )}
                    </div>
                  )}
                </div>
              ) : discoveryFailed && skills.length === 0 ? (
                <div className="flex flex-col items-center justify-center h-64 text-center">
                  <p className="text-lg font-medium text-foreground">
                    {t("skills.loadFailed")}
                  </p>
                  {discoveryError && (
                    <p className="mt-2 max-w-xl text-sm text-muted-foreground">
                      {
                        formatSkillError(
                          discoveryError instanceof Error
                            ? discoveryError.message
                            : String(discoveryError),
                          t,
                          "skills.loadFailed",
                        ).description
                      }
                    </p>
                  )}
                </div>
              ) : skills.length === 0 ? (
                <div className="flex flex-col items-center justify-center h-64 text-center">
                  <p className="text-lg font-medium text-foreground">
                    {t("skills.empty")}
                  </p>
                  <p className="mt-2 text-sm text-muted-foreground">
                    {t("skills.emptyDescription")}
                  </p>
                  <Button
                    variant="link"
                    onClick={() => setRepoManagerOpen(true)}
                    className="mt-3 text-sm font-normal"
                  >
                    {t("skills.addRepo")}
                  </Button>
                </div>
              ) : filteredSkills.length === 0 ? (
                <div className="flex flex-col items-center justify-center h-48 text-center">
                  <p className="text-lg font-medium text-foreground">
                    {t("skills.noResults")}
                  </p>
                  <p className="mt-2 text-sm text-muted-foreground">
                    {t("skills.emptyDescription")}
                  </p>
                </div>
              ) : (
                <>
                  <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                    {displayedRepoSkills.map((skill) => (
                      <SkillCard
                        key={skill.key}
                        skill={skill}
                        onInstall={handleInstall}
                        onUninstall={handleUninstall}
                      />
                    ))}
                  </div>
                  {displayedRepoSkills.length < filteredSkills.length && (
                    <div className="mt-6 flex justify-center">
                      <Button
                        type="button"
                        variant="outline"
                        onClick={() =>
                          setRepoSkillLimit(
                            (current) => current + REPO_SKILL_BATCH_SIZE,
                          )
                        }
                      >
                        {t("skills.discoveryLoadMore", {
                          shown: displayedRepoSkills.length,
                          total: filteredSkills.length,
                        })}
                      </Button>
                    </div>
                  )}
                </>
              )
            ) : (
              /* ===== skills.sh 模式 ===== */
              <>
                {loadingSkillsSh && accumulatedResults.length === 0 ? (
                  <div className="flex items-center justify-center h-64">
                    <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
                    <span className="ml-3 text-sm text-muted-foreground">
                      {t("skills.skillssh.loading")}
                    </span>
                  </div>
                ) : skillsShQuery.length < 2 ? (
                  <div className="flex flex-col items-center justify-center h-64 text-center">
                    <Search className="h-12 w-12 text-muted-foreground/30 mb-4" />
                    <p className="text-sm text-muted-foreground">
                      {t("skills.skillssh.searchPlaceholder")}
                    </p>
                  </div>
                ) : accumulatedResults.length === 0 && !loadingSkillsSh ? (
                  <div className="flex flex-col items-center justify-center h-48 text-center">
                    <p className="text-lg font-medium text-foreground">
                      {t("skills.skillssh.noResults", {
                        query: skillsShQuery,
                      })}
                    </p>
                  </div>
                ) : (
                  <>
                    <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                      {accumulatedResults.map((skill) => {
                        const installed = isSkillsShInstalled(skill);
                        return (
                          <SkillCard
                            key={skill.key}
                            skill={{
                              ...toDiscoverableSkill(skill),
                              installed,
                            }}
                            installs={skill.installs}
                            onInstall={handleInstall}
                            onUninstall={handleUninstall}
                          />
                        );
                      })}
                    </div>

                    {/* 加载更多 + 底部信息 */}
                    <div className="mt-6 flex flex-col items-center gap-2">
                      {hasMoreSkillsSh && (
                        <Button
                          variant="outline"
                          size="sm"
                          disabled={fetchingSkillsSh}
                          onClick={() =>
                            setSkillsShOffset(
                              (prev) => prev + SKILLSSH_PAGE_SIZE,
                            )
                          }
                        >
                          {fetchingSkillsSh ? (
                            <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
                          ) : null}
                          {t("skills.skillssh.loadMore")}
                        </Button>
                      )}
                      <p className="text-xs text-muted-foreground">
                        {t("skills.skillssh.poweredBy")}
                      </p>
                    </div>
                  </>
                )}
              </>
            )}
          </div>
        </div>

        {/* 仓库管理面板 */}
        {repoManagerOpen && (
          <RepoManagerPanel
            repos={repos}
            skills={skills}
            failures={discoveryResult?.failures ?? []}
            retryingRepos={retryingRepos}
            onRetry={handleRetryRepo}
            onAdd={handleAddRepo}
            onRemove={handleRemoveRepo}
            onClose={() => setRepoManagerOpen(false)}
          />
        )}
      </div>
    );
  },
);

SkillsPage.displayName = "SkillsPage";
