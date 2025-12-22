import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Badge } from "@/components/ui/badge";
import { Loader2, HardDrive } from "lucide-react";
import type { SkillRepo, RepoLoadingState, Skill } from "@/lib/api/skills";
import { getRepoKey, LOCAL_REPO_KEY } from "@/hooks/useSkillsLoader";

// 重新导出 LOCAL_REPO_KEY 以便其他组件使用
export { LOCAL_REPO_KEY } from "@/hooks/useSkillsLoader";

interface RepoFilterProps {
  /** 仓库列表 */
  repos: SkillRepo[];
  /** 每个仓库的加载状态，key 为 "{owner}/{name}" */
  repoStates: Map<string, RepoLoadingState>;
  /** 当前选中的仓库 key，或 "all" 表示全部，或 "__local__" 表示本地 */
  selectedRepo: string | "all";
  /** 选择仓库时的回调 */
  onSelect: (repoKey: string | "all") => void;
  /** 所有技能列表（用于计算本地技能数量） */
  skills?: Skill[];
}

/**
 * 判断技能是否为本地独有技能（不属于任何仓库）
 */
export function isLocalOnlySkill(skill: Skill): boolean {
  return !skill.repoOwner || !skill.repoName;
}

/**
 * 判断技能是否已安装（本地存在）
 */
export function isInstalledSkill(skill: Skill): boolean {
  return skill.installed;
}

/**
 * 仓库筛选组件
 *
 * 功能：
 * - 提供 "全部" 选项显示所有仓库的技能
 * - 提供 "本地" 选项显示本地技能（不属于任何仓库）
 * - 列出每个启用的仓库，格式为 "{owner}/{name}"
 * - 显示分支作为单独的标签
 * - 显示每个仓库的技能数量
 * - 加载中的仓库显示 loading 图标
 */
export function RepoFilter({
  repos,
  repoStates,
  selectedRepo,
  onSelect,
  skills = [],
}: RepoFilterProps) {
  const { t } = useTranslation();

  // 只显示启用的仓库
  const enabledRepos = repos.filter((repo) => repo.enabled);

  // 计算本地已安装的技能数量（所有 installed = true 的技能，按目录名去重）
  // 因为安装时只用目录名最后一段，所以不同仓库的同名技能会安装到同一目录
  const localSkillCount = useMemo(() => {
    const installedSkills = skills.filter(isInstalledSkill);
    const seenDirectories = new Set<string>();
    let count = 0;
    for (const skill of installedSkills) {
      const dirName = skill.directory.split('/').pop()?.toLowerCase() || skill.directory.toLowerCase();
      if (!seenDirectories.has(dirName)) {
        seenDirectories.add(dirName);
        count++;
      }
    }
    return count;
  }, [skills]);

  // 计算所有仓库的技能总数（包括本地技能）
  const repoSkillCount = Array.from(repoStates.values()).reduce(
    (sum, state) => sum + (state.skillCount || 0),
    0
  );
  const totalSkillCount = repoSkillCount + localSkillCount;

  // 检查是否有任何仓库正在加载
  const hasLoadingRepos = Array.from(repoStates.values()).some(
    (state) => state.status === "pending" || state.status === "loading"
  );

  /**
   * 获取选中项的显示文本
   */
  const getSelectedDisplayText = () => {
    if (selectedRepo === "all") {
      return t("skills.repoFilter.all");
    }
    if (selectedRepo === LOCAL_REPO_KEY) {
      return t("skills.repoFilter.local");
    }
    const repo = enabledRepos.find((r) => getRepoKey(r) === selectedRepo);
    if (repo) {
      return `${repo.owner}/${repo.name}`;
    }
    return t("skills.repoFilter.all");
  };

  return (
    <Select value={selectedRepo} onValueChange={onSelect}>
      <SelectTrigger className="bg-card border shadow-sm text-foreground w-full md:w-56">
        <SelectValue placeholder={t("skills.repoFilter.placeholder")}>
          <span className="flex items-center gap-2 overflow-hidden">
            <span className="truncate">{getSelectedDisplayText()}</span>
            {selectedRepo === "all" && hasLoadingRepos && (
              <Loader2 className="h-3 w-3 animate-spin text-muted-foreground shrink-0" />
            )}
          </span>
        </SelectValue>
      </SelectTrigger>
      <SelectContent className="bg-card text-foreground shadow-lg max-w-[300px]">
        {/* "全部" 选项 */}
        <SelectItem
          value="all"
          className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
        >
          <div className="flex items-center justify-between w-full gap-2">
            <span>{t("skills.repoFilter.all")}</span>
            <div className="flex items-center gap-1 shrink-0">
              {hasLoadingRepos ? (
                <Loader2 className="h-3 w-3 animate-spin text-muted-foreground" />
              ) : (
                <span className="text-xs text-muted-foreground">
                  ({totalSkillCount})
                </span>
              )}
            </div>
          </div>
        </SelectItem>

        {/* "本地" 选项 - 只有当存在本地技能时才显示 */}
        {localSkillCount > 0 && (
          <SelectItem
            value={LOCAL_REPO_KEY}
            className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
          >
            <div className="flex items-center justify-between w-full gap-2">
              <div className="flex items-center gap-2 min-w-0 flex-1">
                <HardDrive className="h-3 w-3 text-muted-foreground shrink-0" />
                <span>{t("skills.repoFilter.local")}</span>
              </div>
              <span className="text-xs text-muted-foreground shrink-0">
                ({localSkillCount})
              </span>
            </div>
          </SelectItem>
        )}

        {/* 仓库列表 */}
        {enabledRepos.map((repo) => {
          const repoKey = getRepoKey(repo);
          const state = repoStates.get(repoKey);
          const isLoading =
            state?.status === "pending" || state?.status === "loading";
          const hasError = state?.status === "error";
          const skillCount = state?.skillCount;

          return (
            <SelectItem
              key={repoKey}
              value={repoKey}
              className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
            >
              <div className="flex items-center justify-between w-full gap-2">
                <div className="flex items-center gap-2 min-w-0 flex-1">
                  <span className="truncate">{`${repo.owner}/${repo.name}`}</span>
                  {/* 分支标签 */}
                  <Badge
                    variant="secondary"
                    className="text-[10px] px-1.5 py-0 h-4 shrink-0"
                  >
                    {repo.branch || "main"}
                  </Badge>
                </div>
                <div className="flex items-center gap-1 shrink-0">
                  {isLoading ? (
                    <Loader2 className="h-3 w-3 animate-spin text-muted-foreground" />
                  ) : hasError ? (
                    <span className="text-xs text-destructive">!</span>
                  ) : skillCount !== undefined ? (
                    <span className="text-xs text-muted-foreground">
                      ({skillCount})
                    </span>
                  ) : null}
                </div>
              </div>
            </SelectItem>
          );
        })}
      </SelectContent>
    </Select>
  );
}
