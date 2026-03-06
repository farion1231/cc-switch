import { useState, useMemo, forwardRef, useImperativeHandle } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { RefreshCw, Search } from "lucide-react";
import { toast } from "sonner";
import { SkillCard } from "./SkillCard";
import { RepoManagerPanel } from "./RepoManagerPanel";
import {
  useDiscoverableSkills,
  useInstalledSkills,
  useInstallSkill,
  useBatchInstallSkills,
  useSkillRepos,
  useAddSkillRepo,
  useRemoveSkillRepo,
} from "@/hooks/useSkills";
import type { AppId } from "@/lib/api/types";
import {
  skillsApi,
  type DiscoverableSkill,
  type SkillRepo,
} from "@/lib/api/skills";
import { formatSkillError } from "@/lib/errors/skillErrorParser";

interface SkillsPageProps {
  initialApp?: AppId;
}

export interface SkillsPageHandle {
  refresh: () => Promise<void>;
  openRepoManager: () => void;
}

/**
 * Skills 发现面板
 * 用于浏览和安装来自仓库的 Skills
 */
export const SkillsPage = forwardRef<SkillsPageHandle, SkillsPageProps>(
  ({ initialApp = "claude" }, ref) => {
    const { t } = useTranslation();
    const queryClient = useQueryClient();
    const [repoManagerOpen, setRepoManagerOpen] = useState(false);
    const [searchQuery, setSearchQuery] = useState("");
    const [filterRepo, setFilterRepo] = useState<string>("all");
    const [filterStatus, setFilterStatus] = useState<
      "all" | "installed" | "uninstalled"
    >("all");
    const [selected, setSelected] = useState<Set<string>>(new Set());

    // currentApp 用于安装时的默认应用
    const currentApp = initialApp;

    // Queries
    const {
      data: discoverableSkills,
      isLoading: loadingDiscoverable,
      isFetching: fetchingDiscoverable,
    } = useDiscoverableSkills();
    const { data: installedSkills } = useInstalledSkills();
    const { data: repos = [], refetch: refetchRepos } = useSkillRepos();

    // Mutations
    const installMutation = useInstallSkill();
    const batchInstallMutation = useBatchInstallSkills();
    const addRepoMutation = useAddSkillRepo();
    const removeRepoMutation = useRemoveSkillRepo();
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
      refresh: async () => {
        const latest = await skillsApi.discoverAvailable(true);
        queryClient.setQueryData(["skills", "discoverable"], latest);
        await refetchRepos();
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

    const toggleSelect = (key: string) => {
      setSelected((prev) => {
        const next = new Set(prev);
        if (next.has(key)) {
          next.delete(key);
        } else {
          next.add(key);
        }
        return next;
      });
    };

    const handleAddRepo = async (repo: SkillRepo) => {
      try {
        await addRepoMutation.mutateAsync(repo);
        const latest = await skillsApi.discoverAvailable(true);
        queryClient.setQueryData(["skills", "discoverable"], latest);
        const count = latest.filter(
          (skill) =>
            skill.repoOwner === repo.owner &&
            skill.repoName === repo.name &&
            (skill.repoBranch || "main") === (repo.branch || "main"),
        ).length;
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
        const latest = await skillsApi.discoverAvailable(true);
        queryClient.setQueryData(["skills", "discoverable"], latest);
        toast.success(t("skills.repo.removeSuccess", { owner, name }), {
          closeButton: true,
        });
      } catch (error) {
        toast.error(t("common.error"), {
          description: String(error),
        });
      }
    };

    // 过滤技能列表
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

    const selectedCount = useMemo(
      () =>
        filteredSkills.filter(
          (item) => selected.has(item.key) && !item.installed,
        ).length,
      [filteredSkills, selected],
    );

    const handleSelectAllVisible = () => {
      setSelected((prev) => {
        const next = new Set(prev);
        filteredSkills
          .filter((item) => !item.installed)
          .forEach((item) => next.add(item.key));
        return next;
      });
    };

    const handleClearSelection = () => setSelected(new Set());

    const handleBatchInstall = async () => {
      const target = filteredSkills.filter(
        (item) => selected.has(item.key) && !item.installed,
      );
      if (target.length === 0) {
        toast.info(t("skills.batchInstallNoop"));
        return;
      }

      try {
        const result = await batchInstallMutation.mutateAsync({
          skills: target,
          currentApp,
        });
        const success = result.installed.length;
        const failed = result.failed.length;
        if (failed === 0) {
          toast.success(t("skills.batchInstallSuccess", { count: success }), {
            closeButton: true,
          });
        } else {
          toast.warning(
            t("skills.batchInstallPartial", {
              success,
              failed,
            }),
            { description: result.failed[0]?.error },
          );
        }
        setSelected(new Set());
      } catch (error) {
        toast.error(t("skills.installFailed"), { description: String(error) });
      }
    };

    return (
      <div className="px-6 flex flex-col h-[calc(100vh-8rem)] overflow-hidden bg-background/50">
        {/* 技能网格（可滚动详情区域） */}
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
                  {/* 仓库筛选 */}
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
                  {/* 安装状态筛选 */}
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
                  {searchQuery && (
                    <p className="mt-2 text-sm text-muted-foreground">
                      {t("skills.count", { count: filteredSkills.length })}
                    </p>
                  )}
                </div>
                <div className="mb-4 flex flex-wrap items-center gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={handleSelectAllVisible}
                  >
                    {t("skills.selectAllVisible", {
                      defaultValue: "全选可见",
                    })}
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={handleClearSelection}
                  >
                    {t("skills.clearSelection")}
                  </Button>
                  <Button
                    variant="mcp"
                    size="sm"
                    onClick={handleBatchInstall}
                    disabled={
                      selectedCount === 0 || batchInstallMutation.isPending
                    }
                  >
                    {batchInstallMutation.isPending
                      ? t("skills.installing")
                      : t("skills.batchInstall", { count: selectedCount })}
                  </Button>
                </div>

                {/* 技能列表或无结果提示 */}
                {filteredSkills.length === 0 ? (
                  <div className="flex flex-col items-center justify-center h-48 text-center">
                    <p className="text-lg font-medium text-foreground">
                      {t("skills.noResults")}
                    </p>
                    <p className="mt-2 text-sm text-muted-foreground">
                      {t("skills.emptyDescription")}
                    </p>
                  </div>
                ) : (
                  <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                    {filteredSkills.map((skill) => (
                      <SkillCard
                        key={skill.key}
                        skill={skill}
                        onInstall={handleInstall}
                        onUninstall={handleUninstall}
                        selected={selected.has(skill.key)}
                        onToggleSelect={
                          skill.installed
                            ? undefined
                            : () => toggleSelect(skill.key)
                        }
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
            onClose={() => setRepoManagerOpen(false)}
          />
        )}
      </div>
    );
  },
);

SkillsPage.displayName = "SkillsPage";
