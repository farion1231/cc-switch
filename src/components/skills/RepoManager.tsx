import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Trash2, ExternalLink, Plus, Eye, EyeOff } from "lucide-react";
import { settingsApi } from "@/lib/api";
import type { DiscoverableSkill, SkillRepo, RepoPlatform } from "@/lib/api/skills";

interface RepoManagerProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  repos: SkillRepo[];
  skills: DiscoverableSkill[];
  onAdd: (repo: SkillRepo) => Promise<void>;
  onRemove: (owner: string, name: string) => Promise<void>;
}

export function RepoManager({
  open: isOpen,
  onOpenChange,
  repos,
  skills,
  onAdd,
  onRemove,
}: RepoManagerProps) {
  const { t } = useTranslation();
  const [repoUrl, setRepoUrl] = useState("");
  const [branch, setBranch] = useState("");
  const [authToken, setAuthToken] = useState("");
  const [showToken, setShowToken] = useState(false);
  const [error, setError] = useState("");

  const getSkillCount = (repo: SkillRepo) =>
    skills.filter(
      (skill) =>
        skill.repoOwner === repo.owner &&
        skill.repoName === repo.name &&
        (skill.repoBranch || "main") === (repo.branch || "main"),
    ).length;

  /**
   * 解析仓库 URL，支持多种格式：
   * - GitHub: https://github.com/owner/name
   * - GitLab: https://gitlab.com/owner/name 或 http://x.x.x.x/group/project
   * - Gitea: https://gitea.com/owner/name 或私有部署
   * - 简写: owner/name
   */
  const parseRepoUrl = (
    url: string,
  ): { owner: string; name: string; platform: RepoPlatform; baseUrl?: string } | null => {
    let cleaned = url.trim();

    // 移除 .git 后缀
    cleaned = cleaned.replace(/\.git$/, "");

    // 简写格式: owner/name
    if (/^[^/]+\/[^/]+$/.test(cleaned)) {
      const [owner, name] = cleaned.split("/");
      if (owner && name) {
        return { owner, name, platform: "github" };
      }
      return null;
    }

    // 解析完整 URL
    try {
      const urlObj = new URL(cleaned);
      const host = urlObj.hostname.toLowerCase();
      const pathParts = urlObj.pathname.split("/").filter(Boolean);

      if (pathParts.length < 2) {
        return null;
      }

      // 对于私有 GitLab，可能有多层路径
      // 例如: http://192.168.0.1/application-component/ai-tools
      // 我们取最后两个部分作为 owner/name
      let owner: string;
      let name: string;
      
      if (pathParts.length >= 2) {
        // 取最后两个部分
        owner = pathParts[pathParts.length - 2];
        name = pathParts[pathParts.length - 1];
      } else {
        return null;
      }

      // 判断平台类型
      let platform: RepoPlatform = "generic";
      let baseUrl: string | undefined;

      if (host === "github.com") {
        platform = "github";
      } else if (host === "gitlab.com") {
        platform = "gitlab";
      } else if (host === "gitea.com") {
        platform = "gitea";
      } else {
        // 私有部署：根据 URL 特征判断平台
        if (host.includes("gitlab") || urlObj.pathname.includes("/-/")) {
          platform = "gitlab";
        } else if (host.includes("gitea") || host.includes("forgejo")) {
          platform = "gitea";
        } else {
          // 默认使用 GitLab API 格式（最常见的企业私有仓库）
          platform = "gitlab";
        }
        baseUrl = `${urlObj.protocol}//${urlObj.host}`;
      }

      return { owner, name, platform, baseUrl };
    } catch {
      // URL 解析失败，尝试简写格式
      const parts = cleaned.split("/");
      if (parts.length === 2 && parts[0] && parts[1]) {
        return { owner: parts[0], name: parts[1], platform: "github" };
      }
      return null;
    }
  };

  const handleAdd = async () => {
    setError("");

    const parsed = parseRepoUrl(repoUrl);
    if (!parsed) {
      setError(t("skills.repo.invalidUrl"));
      return;
    }

    // 调试日志
    console.log("准备添加仓库:", {
      owner: parsed.owner,
      name: parsed.name,
      branch: branch || "main",
      platform: parsed.platform,
      baseUrl: parsed.baseUrl,
      authToken: authToken || "未提供"
    });

    try {
      await onAdd({
        owner: parsed.owner,
        name: parsed.name,
        branch: branch || "main",
        enabled: true,
        platform: parsed.platform,
        baseUrl: parsed.baseUrl,
        authToken: authToken || undefined,
      });

      setRepoUrl("");
      setBranch("");
      setAuthToken("");
    } catch (e) {
      setError(e instanceof Error ? e.message : t("skills.repo.addFailed"));
    }
  };

  const handleOpenRepo = async (repo: SkillRepo) => {
    try {
      let url: string;
      if (repo.platform === "github" || !repo.baseUrl) {
        url = `https://github.com/${repo.owner}/${repo.name}`;
      } else {
        url = `${repo.baseUrl}/${repo.owner}/${repo.name}`;
      }
      await settingsApi.openExternal(url);
    } catch (error) {
      console.error("Failed to open URL:", error);
    }
  };

  return (
    <Dialog open={isOpen} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[80vh] flex flex-col p-0">
        {/* 固定头部 */}
        <DialogHeader className="flex-shrink-0 border-b border-border-default px-6 py-4">
          <DialogTitle>{t("skills.repo.title")}</DialogTitle>
          <DialogDescription>{t("skills.repo.description")}</DialogDescription>
        </DialogHeader>

        {/* 可滚动内容区域 */}
        <div className="flex-1 min-h-0 overflow-y-auto px-6 py-4">
          {/* 添加仓库表单 */}
          <div className="space-y-5">
            <div className="space-y-2">
              <Label htmlFor="repo-url">{t("skills.repo.url")}</Label>
              <Input
                id="repo-url"
                placeholder={t("skills.repo.urlPlaceholder")}
                value={repoUrl}
                onChange={(e) => setRepoUrl(e.target.value)}
                className="flex-1"
              />
            </div>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <div className="space-y-2">
                <Label htmlFor="branch">{t("skills.repo.branch")}</Label>
                <Input
                  id="branch"
                  placeholder={t("skills.repo.branchPlaceholder")}
                  value={branch}
                  onChange={(e) => setBranch(e.target.value)}
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="auth-token">{t("skills.repo.authToken")}</Label>
                <div className="relative">
                  <Input
                    id="auth-token"
                    type={showToken ? "text" : "password"}
                    placeholder={t("skills.repo.authTokenPlaceholder")}
                    value={authToken}
                    onChange={(e) => setAuthToken(e.target.value)}
                    className="pr-10"
                  />
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="absolute right-0 top-0 h-full px-3 hover:bg-transparent"
                    onClick={() => setShowToken(!showToken)}
                  >
                    {showToken ? (
                      <EyeOff className="h-4 w-4 text-muted-foreground" />
                    ) : (
                      <Eye className="h-4 w-4 text-muted-foreground" />
                    )}
                  </Button>
                </div>
              </div>
            </div>
            <Button
              onClick={handleAdd}
              className="w-full sm:w-auto sm:px-4"
              variant="mcp"
              type="button"
            >
              <Plus className="h-4 w-4 mr-2" />
              {t("skills.repo.add")}
            </Button>
            {error && <p className="text-xs text-destructive">{error}</p>}

            {/* 仓库列表 */}
            <div className="space-y-3">
              <h4 className="text-sm font-medium">{t("skills.repo.list")}</h4>
              {repos.length === 0 ? (
                <p className="text-sm text-muted-foreground">
                  {t("skills.repo.empty")}
                </p>
              ) : (
                <div className="space-y-3">
                  {repos.map((repo) => (
                    <div
                      key={`${repo.owner}/${repo.name}`}
                      className="flex items-center justify-between rounded-xl border border-border-default bg-card px-4 py-3"
                    >
                      <div>
                        <div className="text-sm font-medium text-foreground">
                          {repo.owner}/{repo.name}
                          {repo.platform && repo.platform !== "github" && (
                            <span className="ml-2 text-xs text-muted-foreground">
                              ({repo.platform})
                            </span>
                          )}
                        </div>
                        <div className="mt-1 text-xs text-muted-foreground">
                          {t("skills.repo.branch")}: {repo.branch || "main"}
                          <span className="ml-3 inline-flex items-center rounded-full border border-border-default px-2 py-0.5 text-[11px]">
                            {t("skills.repo.skillCount", {
                              count: getSkillCount(repo),
                            })}
                          </span>
                          {repo.baseUrl && (
                            <span className="ml-3 text-xs">
                              {repo.baseUrl}
                            </span>
                          )}
                        </div>
                      </div>
                      <div className="flex gap-2">
                        <Button
                          variant="ghost"
                          size="icon"
                          type="button"
                          onClick={() => handleOpenRepo(repo)}
                          title={t("common.view", { defaultValue: "查看" })}
                        >
                          <ExternalLink className="h-4 w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          type="button"
                          onClick={() => onRemove(repo.owner, repo.name)}
                          title={t("common.delete")}
                          className="hover:text-red-500 hover:bg-red-100 dark:hover:text-red-400 dark:hover:bg-red-500/10"
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
