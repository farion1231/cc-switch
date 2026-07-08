import { RefreshCw, Settings, type LucideIcon } from "lucide-react";

/**
 * SkillsPage 头部 actions 的独立模块。
 * App 外壳只需要这些轻量定义即可渲染头部按钮，
 * 拆出来后 SkillsPage 本体可以被懒加载（代码分割），
 * 避免把整个 Skills 页面打进首屏 bundle。
 */

export type SkillsPageSource = "repos" | "skillssh";

export interface SkillsPageHandle {
  refresh: () => void;
  openRepoManager: () => void;
}

type SkillsPageHeaderAction = {
  key: string;
  sources: readonly SkillsPageSource[];
  labelKey: string;
  Icon: LucideIcon;
  execute: (page: SkillsPageHandle | null) => void;
};

const SKILLS_PAGE_HEADER_ACTIONS: readonly SkillsPageHeaderAction[] = [
  {
    key: "refresh-repos",
    sources: ["repos"],
    labelKey: "skills.refresh",
    Icon: RefreshCw,
    execute: (page) => page?.refresh(),
  },
  {
    key: "manage-repos",
    sources: ["repos", "skillssh"],
    labelKey: "skills.repoManager",
    Icon: Settings,
    execute: (page) => page?.openRepoManager(),
  },
];

export const getSkillsPageHeaderActions = (source: SkillsPageSource) =>
  SKILLS_PAGE_HEADER_ACTIONS.filter((action) =>
    action.sources.includes(source),
  );
