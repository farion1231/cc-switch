import {
  useState,
  useEffect,
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
import { RefreshCw, Search } from "lucide-react";
import { toast } from "sonner";
import { SkillCard } from "./SkillCard";
import { RepoManagerPanel } from "./RepoManagerPanel";
import { skillsApi, type Skill, type SkillRepo } from "@/lib/api/skills";
import { formatSkillError } from "@/lib/errors/skillErrorParser";

interface SkillsPageProps {
  onClose?: () => void;
}

export interface SkillsPageHandle {
  refresh: () => void;
  openRepoManager: () => void;
}

export const SkillsPage = forwardRef<SkillsPageHandle, SkillsPageProps>(
  ({ onClose: _onClose }, ref) => {
    const { t } = useTranslation();
    const [skills, setSkills] = useState<Skill[]>([]);
    const [repos, setRepos] = useState<SkillRepo[]>([]);
    const [loading, setLoading] = useState(true);
    const [repoManagerOpen, setRepoManagerOpen] = useState(false);
    const [searchQuery, setSearchQuery] = useState("");
    const [filterStatus, setFilterStatus] = useState<
      "all" | "installed" | "uninstalled"
    >("all");

    const loadSkills = async (afterLoad?: (data: Skill[]) => void) => {
      try {
        setLoading(true);
        const data = await skillsApi.getAll();
        setSkills(data);
        if (afterLoad) {
          afterLoad(data);
        }
      } catch (error) {
        const errorMessage =
          error instanceof Error ? error.message : String(error);

        // 传入 "skills.loadFailed" 作为标题
        const { title, description } = formatSkillError(
          errorMessage,
          t,
          "skills.loadFailed",
        );

        toast.error(title, {
          description,
          duration: 8000,
        });

        console.error("Load skills failed:", error);
      } finally {
        setLoading(false);
      }
    };

    const loadRepos = async () => {
      try {
        const data = await skillsApi.getRepos();
        setRepos(data);
      } catch (error) {
        console.error("Failed to load repos:", error);
      }
    };

    useEffect(() => {
      Promise.all([loadSkills(), loadRepos()]);
    }, []);

    useImperativeHandle(ref, () => ({
      refresh: () => loadSkills(),
      openRepoManager: () => setRepoManagerOpen(true),
    }));

    const handleInstall = async (directory: string) => {
      try {
        await skillsApi.install(directory);
        toast.success(t("skills.installSuccess", { name: directory }));
        await loadSkills();
      } catch (error) {
        const errorMessage =
          error instanceof Error ? error.message : String(error);

        // 使用错误解析器格式化错误，传入 "skills.installFailed"
        const { title, description } = formatSkillError(
          errorMessage,
          t,
          "skills.installFailed",
        );

        toast.error(title, {
          description,
          duration: 10000, // 延长显示时间让用户看清
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
        await skillsApi.uninstall(directory);
        toast.success(t("skills.uninstallSuccess", { name: directory }));
        await loadSkills();
      } catch (error) {
        const errorMessage =
          error instanceof Error ? error.message : String(error);

        // 使用错误解析器格式化错误，传入 "skills.uninstallFailed"
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

      let repoSkillCount = 0;
      await Promise.all([
        loadRepos(),
        loadSkills((data) => {
          repoSkillCount = data.filter(
            (skill) =>
              skill.repoOwner === repo.owner &&
              skill.repoName === repo.name &&
              (skill.repoBranch || "main") === (repo.branch || "main"),
          ).length;
        }),
      ]);

      toast.success(
        t("skills.repo.addSuccess", {
          owner: repo.owner,
          name: repo.name,
          count: repoSkillCount,
        }),
      );
    };

    const handleRemoveRepo = async (owner: string, name: string) => {
      await skillsApi.removeRepo(owner, name);
      toast.success(t("skills.repo.removeSuccess", { owner, name }));
      await Promise.all([loadRepos(), loadSkills()]);
    };

    // 过滤技能列表
    const filteredSkills = useMemo(() => {
      const byStatus = skills.filter((skill) => {
        if (filterStatus === "installed") return skill.installed;
        if (filterStatus === "uninstalled") return !skill.installed;
        return true;
      });

      if (!searchQuery.trim()) return byStatus;

      const query = searchQuery.toLowerCase();
      return byStatus.filter((skill) => {
        const name = skill.name?.toLowerCase() || "";
        const description = skill.description?.toLowerCase() || "";
        const directory = skill.directory?.toLowerCase() || "";

        return (
          name.includes(query) ||
          description.includes(query) ||
          directory.includes(query)
        );
      });
    }, [skills, searchQuery, filterStatus]);

    return (
      <div className="mx-auto max-w-[56rem] px-6 flex flex-col h-[calc(100vh-8rem)] overflow-hidden bg-background/50">
        {/* 技能网格（可滚动详情区域） */}
        <div className="flex-1 overflow-y-auto overflow-x-hidden animate-fade-in">
          <div className="py-4">
            {loading ? (
              <div className="flex items-center justify-center h-64">
                <RefreshCw className="h-8 w-8 animate-spin text-muted-foreground" />
              </div>
            ) : skills.length === 0 ? (
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
                {/* 搜索框 */}
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
                  <div className="w-full md:w-48">
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
                    <p className="text-lg font-medium text-gray-900 dark:text-gray-100">
                      {t("skills.noResults")}
                    </p>
                    <p className="mt-2 text-sm text-gray-500 dark:text-gray-400">
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
