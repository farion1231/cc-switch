import { useState, useMemo, forwardRef, useImperativeHandle } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { RefreshCw, Search, LayoutGrid, List, ExternalLink, Download, Trash2, Loader2 } from "lucide-react";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
  TooltipProvider,
} from "@/components/ui/tooltip";
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
  useRefreshDiscoverableCache,
} from "@/hooks/useSkills";
import type { AppId } from "@/lib/api/types";
import type { DiscoverableSkill, SkillRepo } from "@/lib/api/skills";
import { formatSkillError } from "@/lib/errors/skillErrorParser";
import { settingsApi } from "@/lib/api";

interface SkillsPageProps {
  initialApp?: AppId;
}

export interface SkillsPageHandle {
  refresh: () => void;
  openRepoManager: () => void;
}

/**
 * Skills 发现面板
 * 用于浏览和安装来自仓库的 Skills
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
    const [viewMode, setViewMode] = useState<"grid" | "list">("grid");

    // currentApp 用于安装时的默认应用
    const currentApp = initialApp;

    // Queries
    const {
      data: discoverableSkills,
      isLoading: loadingDiscoverable,
      isFetching: fetchingDiscoverable,
      refetch: refetchDiscoverable,
    } = useDiscoverableSkills();
    const { data: installedSkills } = useInstalledSkills();
    const { data: repos = [], refetch: refetchRepos } = useSkillRepos();

    // Mutations
    const installMutation = useInstallSkill();
    const addRepoMutation = useAddSkillRepo();
    const removeRepoMutation = useRemoveSkillRepo();
    const refreshDiscoverableCacheMutation = useRefreshDiscoverableCache();

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

    // 从可发现技能中提取所有仓库选项
    const repoOptions = useMemo(() => {
      if (!discoverableSkills) return [];
      const repoSet = new Set<string>();
      discoverableSkills.forEach((s) => {
        if (s.repoOwner && s.repoName) {
          repoSet.add(`${s.repoOwner}/${s.repoName}`);
        }
      });
      return Array.from(repoSet).sort();
    }, [discoverableSkills]);

    // 为发现列表补齐 installed 状态，供 SkillCard 使用
    const skills: DiscoverableSkillItem[] = useMemo(() => {
      if (!discoverableSkills) return [];
      return discoverableSkills.map((d) => {
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
    }, [discoverableSkills, installedKeys]);

    const loading = loadingDiscoverable || fetchingDiscoverable;

    useImperativeHandle(ref, () => ({
      refresh: () => {
        refetchDiscoverable();
        refetchRepos();
      },
      openRepoManager: () => setRepoManagerOpen(true),
    }));

    const handleInstall = async (directory: string) => {
      // 找到对应的 DiscoverableSkill
      const skill = discoverableSkills?.find(
        (s) =>
          s.directory === directory ||
          s.directory.split("/").pop() === directory,
      );
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
        // Await discovery so we can report the real count
        const { data: freshSkills } = await refetchDiscoverable();
        const count =
          freshSkills?.filter(
            (s) =>
              s.repoOwner === repo.owner &&
              s.repoName === repo.name &&
              (s.repoBranch || "main") === (repo.branch || "main"),
          ).length ?? 0;
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

    const handleRefresh = () => {
      refetchDiscoverable();
      refetchRepos();
    };

    const handleForceRefresh = async () => {
      try {
        const refreshed = await refreshDiscoverableCacheMutation.mutateAsync();
        toast.success(t("skills.forceRefreshSuccess", { count: refreshed.length }), {
          closeButton: true,
        });
      } catch (error) {
        toast.error(t("skills.forceRefreshFailed"), { description: String(error) });
      }
    };

    // 过滤技能列表 - 合并多次 filter 为单次迭代优化性能
    const filteredSkills = useMemo(() => {
      const query = searchQuery.trim().toLowerCase();
      const isRepoFiltered = filterRepo !== "all";
      const isStatusFiltered = filterStatus !== "all";

      return skills.filter((skill) => {
        // 仓库筛选
        if (isRepoFiltered) {
          const skillRepo = `${skill.repoOwner}/${skill.repoName}`;
          if (skillRepo !== filterRepo) return false;
        }

        // 状态筛选
        if (isStatusFiltered) {
          if (filterStatus === "installed" && !skill.installed) return false;
          if (filterStatus === "uninstalled" && skill.installed) return false;
        }

        // 搜索关键词筛选
        if (query) {
          const name = skill.name?.toLowerCase() || "";
          const repo =
            skill.repoOwner && skill.repoName
              ? `${skill.repoOwner}/${skill.repoName}`.toLowerCase()
              : "";
          if (!name.includes(query) && !repo.includes(query)) return false;
        }

        return true;
      });
    }, [skills, searchQuery, filterRepo, filterStatus]);

    return (
      <TooltipProvider delayDuration={300}>
        <div className="px-6 flex flex-col flex-1 min-h-0 overflow-hidden bg-background/50">
          <div className="flex-1 overflow-y-auto overflow-x-hidden animate-fade-in">
            <div className="py-4">
              {loading ? (
                <div className="flex items-center justify-center h-64">
                  <RefreshCw className="h-8 w-8 animate-spin text-muted-foreground" />
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
              ) : (
                <>
                  <div className="mb-6 flex flex-col gap-3 md:flex-row md:items-center sticky top-0 z-10 bg-card border shadow-sm p-3">
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
                    <div className="w-full md:w-56">
                      <Select value={filterRepo} onValueChange={setFilterRepo}>
                        <SelectTrigger className="bg-card border shadow-sm text-foreground">
                          <SelectValue
                            placeholder={t("skills.filter.repo")}
                            className="text-left truncate"
                          />
                        </SelectTrigger>
                        <SelectContent className="bg-card text-foreground shadow-lg max-h-64 min-w-[var(--radix-select-trigger-width)]">
                          <SelectItem
                            value="all"
                            className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                          >
                            {t("skills.filter.allRepos")}
                          </SelectItem>
                          {repoOptions.map((repo) => (
                            <SelectItem
                              key={repo}
                              value={repo}
                              className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                              title={repo}
                            >
                              <span className="truncate block max-w-[200px]">
                                {repo}
                              </span>
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>
                    <div className="w-full md:w-36">
                      <Select
                        value={filterStatus}
                        onValueChange={(val) =>
                          setFilterStatus(
                            val as "all" | "installed" | "uninstalled",
                          )
                        }
                      >
                        <SelectTrigger className="bg-card border shadow-sm text-foreground">
                          <SelectValue
                            placeholder={t("skills.filter.placeholder")}
                            className="text-left"
                          />
                        </SelectTrigger>
                        <SelectContent className="bg-card text-foreground shadow-lg">
                          <SelectItem
                            value="all"
                            className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                          >
                            {t("skills.filter.all")}
                          </SelectItem>
                          <SelectItem
                            value="installed"
                            className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                          >
                            {t("skills.filter.installed")}
                          </SelectItem>
                          <SelectItem
                            value="uninstalled"
                            className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                          >
                            {t("skills.filter.uninstalled")}
                          </SelectItem>
                        </SelectContent>
                      </Select>
                    </div>
                    <div className="flex items-center gap-1 border rounded-md p-0.5 shrink-0">
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button
                            variant={viewMode === "grid" ? "secondary" : "ghost"}
                            size="icon"
                            className="size-7"
                            onClick={() => setViewMode("grid")}
                          >
                            <LayoutGrid className="size-3.5" />
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent>{t("skills.viewGrid")}</TooltipContent>
                      </Tooltip>
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button
                            variant={viewMode === "list" ? "secondary" : "ghost"}
                            size="icon"
                            className="size-7"
                            onClick={() => setViewMode("list")}
                          >
                            <List className="size-3.5" />
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent>{t("skills.viewList")}</TooltipContent>
                      </Tooltip>
                    </div>
                    <div className="flex items-center gap-1.5 shrink-0">
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button
                            type="button"
                            variant="outline"
                            size="sm"
                            onClick={handleRefresh}
                            disabled={loading || refreshDiscoverableCacheMutation.isPending}
                          >
                            <RefreshCw
                              className={`size-3.5 mr-1.5 ${loading ? "animate-spin" : ""}`}
                            />
                            {loading ? t("skills.refreshing") : t("skills.refresh")}
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent>{t("skills.refreshHint")}</TooltipContent>
                      </Tooltip>
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button
                            type="button"
                            variant="secondary"
                            size="sm"
                            onClick={handleForceRefresh}
                            disabled={refreshDiscoverableCacheMutation.isPending}
                          >
                            {refreshDiscoverableCacheMutation.isPending ? (
                              <Loader2 className="size-3.5 mr-1.5 animate-spin" />
                            ) : (
                              <Download className="size-3.5 mr-1.5" />
                            )}
                            {refreshDiscoverableCacheMutation.isPending
                              ? t("skills.forceRefreshing")
                              : t("skills.forceRefresh")}
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent>{t("skills.forceRefreshHint")}</TooltipContent>
                      </Tooltip>
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        onClick={() => setRepoManagerOpen(true)}
                      >
                        {t("skills.repoManager")}
                      </Button>
                    </div>
                    {searchQuery && (
                      <p className="mt-2 text-sm text-muted-foreground">
                        {t("skills.count", { count: filteredSkills.length })}
                      </p>
                    )}
                  </div>

                  {filteredSkills.length === 0 ? (
                    <div className="flex flex-col items-center justify-center h-48 text-center">
                      <p className="text-lg font-medium text-foreground">
                        {t("skills.noResults")}
                      </p>
                      <p className="mt-2 text-sm text-muted-foreground">
                        {t("skills.emptyDescription")}
                      </p>
                    </div>
                  ) : viewMode === "list" ? (
                    <div className="space-y-2">
                      {filteredSkills.map((skill) => (
                        <div
                          key={skill.key}
                          className="flex items-center gap-4 p-3 border rounded-lg bg-card hover:bg-muted/50 transition-colors"
                        >
                          <div className="flex-1 min-w-0">
                            <div className="flex items-center gap-2 flex-wrap">
                              <span className="font-medium truncate">{skill.name}</span>
                              {skill.installed && (
                                <Badge variant="default" className="shrink-0 bg-green-600/90 text-white text-[10px] px-1.5 py-0 h-4">
                                  {t("skills.installed")}
                                </Badge>
                              )}
                              {skill.repoOwner && skill.repoName && (
                                <Badge variant="outline" className="shrink-0 text-[10px] px-1.5 py-0 h-4">
                                  {skill.repoOwner}/{skill.repoName}
                                </Badge>
                              )}
                            </div>
                            <p className="text-sm text-muted-foreground truncate mt-0.5">
                              {skill.description || t("skills.noDescription")}
                            </p>
                          </div>
                          <div className="flex items-center gap-2 shrink-0">
                            {skill.readmeUrl && (
                              <Button
                                variant="ghost"
                                size="icon"
                                className="size-8"
                                onClick={() => settingsApi.openExternal(skill.readmeUrl!)}
                              >
                                <ExternalLink className="size-4" />
                              </Button>
                            )}
                            {skill.installed ? (
                              <Button
                                variant="outline"
                                size="sm"
                                className="text-red-600 border-red-200 hover:bg-red-50 hover:text-red-700 dark:border-red-900/50 dark:text-red-400 dark:hover:bg-red-950/50"
                                onClick={() => handleUninstall(skill.directory)}
                              >
                                <Trash2 className="size-3.5 mr-1.5" />
                                {t("skills.uninstall")}
                              </Button>
                            ) : (
                              <Button
                                variant="mcp"
                                size="sm"
                                disabled={!skill.repoOwner || installMutation.isPending}
                                onClick={() => handleInstall(skill.directory)}
                              >
                                {installMutation.isPending ? (
                                  <Loader2 className="size-3.5 mr-1.5 animate-spin" />
                                ) : (
                                  <Download className="size-3.5 mr-1.5" />
                                )}
                                {installMutation.isPending ? t("skills.installing") : t("skills.install")}
                              </Button>
                            )}
                          </div>
                        </div>
                      ))}
                    </div>
                  ) : (
                    <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                      {filteredSkills.map((skill) => (
                        <SkillCard
                          key={skill.key}
                          skill={skill}
                          onInstall={handleInstall}
                          onUninstall={handleUninstall}
                        />
                      ))}
                    </div>
                  )}
                </>
              )}
            </div>
          </div>

          {repoManagerOpen && (
            <RepoManagerPanel
              repos={repos}
              skills={skills}
              onAdd={handleAddRepo}
              onRemove={handleRemoveRepo}
              onClose={() => setRepoManagerOpen(false)}
            />
          )}
        </div>
      </TooltipProvider>
    );
  },
);

SkillsPage.displayName = "SkillsPage";
