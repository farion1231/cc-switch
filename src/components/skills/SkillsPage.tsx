import {
  useState,
  useMemo,
  forwardRef,
  useImperativeHandle,
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
import { RefreshCw, Search, AlertCircle, CheckCircle2 } from "lucide-react";
import { toast } from "sonner";
import { SkillCard } from "./SkillCard";
import { RepoManagerPanel } from "./RepoManagerPanel";
import { RepoFilter, LOCAL_REPO_KEY, isInstalledSkill } from "./RepoFilter";
import {
  skillsApi,
  type SkillRepo,
  type AppType,
} from "@/lib/api/skills";
import { formatSkillError } from "@/lib/errors/skillErrorParser";
import { useSkillsLoader, getRepoKey } from "@/hooks/useSkillsLoader";

interface SkillsPageProps {
  onClose?: () => void;
  initialApp?: AppType;
}

export interface SkillsPageHandle {
  refresh: () => void;
  openRepoManager: () => void;
}

/**
 * 仓库加载状态显示组件
 */
function RepoLoadingStatus({
  repos,
  repoStates,
}: {
  repos: SkillRepo[];
  repoStates: Map<string, import("@/lib/api/skills").RepoLoadingState>;
}) {
  const { t } = useTranslation();
  const enabledRepos = repos.filter((repo) => repo.enabled);

  if (enabledRepos.length === 0) {
    return null;
  }

  // 检查是否有任何仓库正在加载或有错误
  const hasLoadingOrError = enabledRepos.some((repo) => {
    const state = repoStates.get(getRepoKey(repo));
    return (
      state?.status === "pending" ||
      state?.status === "loading" ||
      state?.status === "error"
    );
  });

  // 如果所有仓库都成功加载，不显示状态区域
  if (!hasLoadingOrError) {
    return null;
  }

  return (
    <div className="mb-4 flex flex-wrap items-center gap-2 text-sm">
      <span className="text-muted-foreground">
        {t("skills.repoStatus.loading")}:
      </span>
      {enabledRepos.map((repo) => {
        const repoKey = getRepoKey(repo);
        const state = repoStates.get(repoKey);
        const status = state?.status || "pending";

        return (
          <div
            key={repoKey}
            className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-muted/50"
          >
            {status === "pending" || status === "loading" ? (
              <RefreshCw className="h-3 w-3 animate-spin text-muted-foreground" />
            ) : status === "error" ? (
              <AlertCircle className="h-3 w-3 text-destructive" />
            ) : (
              <CheckCircle2 className="h-3 w-3 text-green-500" />
            )}
            <span
              className={`text-xs ${status === "error" ? "text-destructive" : "text-muted-foreground"}`}
              title={state?.error}
            >
              {repo.owner}/{repo.name}
            </span>
          </div>
        );
      })}
    </div>
  );
}

export const SkillsPage = forwardRef<SkillsPageHandle, SkillsPageProps>(
  ({ onClose: _onClose, initialApp = "claude" }, ref) => {
    const { t } = useTranslation();
    const [repoManagerOpen, setRepoManagerOpen] = useState(false);
    const [searchQuery, setSearchQuery] = useState("");
    const [filterStatus, setFilterStatus] = useState<
      "all" | "installed" | "uninstalled"
    >("all");
    // 默认选中"本地"筛选
    const [selectedRepo, setSelectedRepo] = useState<string | "all">(LOCAL_REPO_KEY);

    // 使用 initialApp，不允许切换
    const selectedApp = initialApp;

    // 使用渐进式加载 Hook
    const {
      skills,
      repos,
      repoStates,
      isLoading,
      isLoadingRepos,
      refresh,
    } = useSkillsLoader(selectedApp);

    useImperativeHandle(ref, () => ({
      refresh: () => refresh(),
      openRepoManager: () => setRepoManagerOpen(true),
    }));

    const handleInstall = async (directory: string) => {
      try {
        await skillsApi.install(directory, selectedApp);
        toast.success(t("skills.installSuccess", { name: directory }), {
          closeButton: true,
        });
        refresh();
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

        console.error("Install skill failed:", {
          directory,
          error,
          message: errorMessage,
        });
      }
    };

    const handleUninstall = async (directory: string) => {
      try {
        await skillsApi.uninstall(directory, selectedApp);
        toast.success(t("skills.uninstallSuccess", { name: directory }), {
          closeButton: true,
        });
        refresh();
      } catch (error) {
        const errorMessage =
          error instanceof Error ? error.message : String(error);

        const { title, description } = formatSkillError(
          errorMessage,
          t,
          "skills.uninstallFailed",
        );

        toast.error(title, {
          description,
          duration: 10000,
        });

        console.error("Uninstall skill failed:", {
          directory,
          error,
          message: errorMessage,
        });
      }
    };

    const handleAddRepo = async (repo: SkillRepo) => {
      await skillsApi.addRepo(repo);
      refresh();

      toast.success(
        t("skills.repo.addSuccess", {
          owner: repo.owner,
          name: repo.name,
          count: 0, // 技能数量会在刷新后更新
        }),
        { closeButton: true },
      );
    };

    const handleRemoveRepo = async (owner: string, name: string) => {
      await skillsApi.removeRepo(owner, name);
      toast.success(t("skills.repo.removeSuccess", { owner, name }), {
        closeButton: true,
      });
      // 如果当前选中的仓库被删除，重置为 "all"
      const removedRepoKey = `${owner}/${name}`;
      if (selectedRepo === removedRepoKey) {
        setSelectedRepo("all");
      }
      refresh();
    };

    const handleToggleRepoEnabled = async (owner: string, name: string, enabled: boolean) => {
      try {
        await skillsApi.toggleRepoEnabled(owner, name, enabled);
        refresh();
      } catch (error) {
        toast.error(t("skills.repo.toggleFailed"), {
          closeButton: true,
        });
        console.error("Toggle repo enabled failed:", error);
      }
    };

    // 过滤技能列表 - 支持仓库筛选 + 搜索 + 状态筛选组合
    const filteredSkills = useMemo(() => {
      // 1. 按仓库筛选
      let filtered = skills;
      if (selectedRepo === LOCAL_REPO_KEY) {
        // "本地" = 所有已安装的技能，按目录名去重
        // 因为安装时只用目录名最后一段，所以不同仓库的同名技能会安装到同一目录
        const installedSkills = skills.filter(isInstalledSkill);
        const seenDirectories = new Set<string>();
        filtered = installedSkills.filter((skill) => {
          // 使用目录名最后一段作为去重 key
          const dirName = skill.directory.split('/').pop()?.toLowerCase() || skill.directory.toLowerCase();
          if (seenDirectories.has(dirName)) {
            return false;
          }
          seenDirectories.add(dirName);
          return true;
        });
      } else if (selectedRepo !== "all") {
        // 筛选特定仓库的技能
        filtered = skills.filter((skill) => {
          const skillRepoKey = `${skill.repoOwner}/${skill.repoName}`;
          return skillRepoKey === selectedRepo;
        });
      }

      // 2. 按安装状态筛选（仅在非"本地"筛选时生效，因为"本地"已经是已安装的）
      if (selectedRepo !== LOCAL_REPO_KEY) {
        filtered = filtered.filter((skill) => {
          if (filterStatus === "installed") return skill.installed;
          if (filterStatus === "uninstalled") return !skill.installed;
          return true;
        });
      }

      // 3. 按搜索关键词筛选
      if (searchQuery.trim()) {
        const query = searchQuery.toLowerCase();
        filtered = filtered.filter((skill) => {
          const name = skill.name?.toLowerCase() || "";
          const description = skill.description?.toLowerCase() || "";
          const directory = skill.directory?.toLowerCase() || "";

          return (
            name.includes(query) ||
            description.includes(query) ||
            directory.includes(query)
          );
        });
      }

      return filtered;
    }, [skills, selectedRepo, filterStatus, searchQuery]);

    // 判断是否显示空状态（仓库列表加载完成且没有启用的仓库）
    const showEmptyState = !isLoadingRepos && repos.filter(r => r.enabled).length === 0;

    // 判断是否显示初始加载状态（仓库列表正在加载）
    const showInitialLoading = isLoadingRepos;

    return (
      <div className="mx-auto max-w-[56rem] px-6 flex flex-col h-[calc(100vh-8rem)] overflow-hidden bg-background/50">
        {/* 技能网格（可滚动详情区域） */}
        <div className="flex-1 overflow-y-auto overflow-x-hidden animate-fade-in">
          <div className="py-4 px-2">
            {showInitialLoading ? (
              <div className="flex items-center justify-center h-64">
                <RefreshCw className="h-8 w-8 animate-spin text-muted-foreground" />
              </div>
            ) : showEmptyState ? (
              <div className="flex flex-col items-center justify-center h-64 text-center">
                <p className="text-lg font-medium text-gray-900 dark:text-gray-100">
                  {t("skills.empty")}
                </p>
                <p className="mt-2 text-sm text-gray-500 dark:text-gray-400">
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
                {/* 仓库加载状态显示区域 */}
                <RepoLoadingStatus repos={repos} repoStates={repoStates} />

                {/* 搜索框和筛选器 */}
                <div className="mb-6 flex flex-col gap-3 md:flex-row md:items-center">
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
                  {/* 仓库筛选器 */}
                  <RepoFilter
                    repos={repos}
                    repoStates={repoStates}
                    selectedRepo={selectedRepo}
                    onSelect={setSelectedRepo}
                    skills={skills}
                  />
                  {/* 状态筛选器 */}
                  <div className="w-full md:w-24">
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
                  {searchQuery && (
                    <p className="mt-2 text-sm text-muted-foreground">
                      {t("skills.count", { count: filteredSkills.length })}
                    </p>
                  )}
                </div>

                {/* 技能列表或无结果提示 */}
                {filteredSkills.length === 0 ? (
                  <div className="flex flex-col items-center justify-center h-48 text-center">
                    {isLoading ? (
                      <>
                        <RefreshCw className="h-6 w-6 animate-spin text-muted-foreground mb-2" />
                        <p className="text-sm text-muted-foreground">
                          {t("skills.loading")}
                        </p>
                      </>
                    ) : (
                      <>
                        <p className="text-lg font-medium text-gray-900 dark:text-gray-100">
                          {t("skills.noResults")}
                        </p>
                        <p className="mt-2 text-sm text-gray-500 dark:text-gray-400">
                          {t("skills.emptyDescription")}
                        </p>
                      </>
                    )}
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

        {/* 仓库管理面板 */}
        {repoManagerOpen && (
          <RepoManagerPanel
            repos={repos}
            skills={skills}
            onAdd={handleAddRepo}
            onRemove={handleRemoveRepo}
            onToggleEnabled={handleToggleRepoEnabled}
            onClose={() => setRepoManagerOpen(false)}
          />
        )}
      </div>
    );
  },
);

SkillsPage.displayName = "SkillsPage";
