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
import { Trash2, ExternalLink, Plus } from "lucide-react";
import { settingsApi } from "@/lib/api";
import type { DiscoverableSkill, SkillRepo } from "@/lib/api/skills";

const SUPPORTED_GITLAB_HOSTS = new Set(["gitlab.com", "gitlabwh.uniontech.com"]);

function parseSkillRepoUrl(
  url: string,
): { owner: string; name: string; provider: "github" | "gitlab"; repoUrl?: string } | null {
  const trimmed = url.trim();
  if (!trimmed) return null;

  const githubShorthand = trimmed.replace(/^https?:\/\/github\.com\//, "").replace(/\.git$/, "");
  const githubParts = githubShorthand.split("/");
  if (githubParts.length === 2 && githubParts[0] && githubParts[1]) {
    const isGithubUrl = /^https?:\/\/github\.com\//.test(trimmed) || !trimmed.includes("://");
    if (isGithubUrl) {
      return { owner: githubParts[0], name: githubParts[1], provider: "github" };
    }
  }

  try {
    const parsed = new URL(trimmed);
    if (parsed.protocol !== "https:") return null;
    const isGitLab = SUPPORTED_GITLAB_HOSTS.has(parsed.hostname);
    const isGitHub = parsed.hostname === "github.com";
    if (!isGitLab && !isGitHub) return null;

    const parts = parsed.pathname.split("/").filter(Boolean);
    if (parts.length < 2) return null;
    if (parts.includes("tree") || parts.includes("blob") || parts.includes("-")) {
      return null;
    }
    const name = parts.at(-1)?.replace(/\.git$/, "") || "";
    const ownerPath = parts.slice(0, -1).join("/");
    if (!ownerPath || !name) return null;
    return {
      owner: isGitLab ? `${parsed.hostname}/${ownerPath}` : ownerPath,
      name,
      provider: isGitLab ? "gitlab" : "github",
      repoUrl: isGitLab ? `${parsed.origin}/${ownerPath}/${name}.git` : undefined,
    };
  } catch {
    return null;
  }
}

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
  const [error, setError] = useState("");

  const getSkillCount = (repo: SkillRepo) =>
    skills.filter(
      (skill) =>
        skill.repoOwner === repo.owner &&
        skill.repoName === repo.name &&
        (skill.repoBranch || "main") === (repo.branch || "main"),
    ).length;


  const handleAdd = async () => {
    setError("");

    const parsed = parseSkillRepoUrl(repoUrl);
    if (!parsed) {
      setError(t("skills.repo.invalidUrl"));
      return;
    }

    try {
      await onAdd({
        owner: parsed.owner,
        name: parsed.name,
        branch: branch || "main",
        enabled: true,
        provider: parsed.provider,
        repoUrl: parsed.repoUrl,
      });

      setRepoUrl("");
      setBranch("");
    } catch (e) {
      setError(e instanceof Error ? e.message : t("skills.repo.addFailed"));
    }
  };

  const handleOpenRepo = async (repo: SkillRepo) => {
    try {
      await settingsApi.openExternal(
        repo.repoUrl?.replace(/\.git$/, "") || `https://github.com/${repo.owner}/${repo.name}`,
      );
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
              <div className="flex flex-col gap-3">
                <Input
                  id="repo-url"
                  placeholder={t("skills.repo.urlPlaceholder")}
                  value={repoUrl}
                  onChange={(e) => setRepoUrl(e.target.value)}
                  className="flex-1"
                />
                <div className="flex flex-col gap-3 sm:flex-row">
                  <Input
                    id="branch"
                    placeholder={t("skills.repo.branchPlaceholder")}
                    value={branch}
                    onChange={(e) => setBranch(e.target.value)}
                    className="flex-1"
                  />
                  <Button
                    onClick={handleAdd}
                    className="w-full sm:w-auto sm:px-4"
                    variant="mcp"
                    type="button"
                  >
                    <Plus className="h-4 w-4 mr-2" />
                    {t("skills.repo.add")}
                  </Button>
                </div>
              </div>
              {error && <p className="text-xs text-destructive">{error}</p>}
            </div>

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
                          {repo.provider === "gitlab" && (
                            <span className="ml-2 text-[11px] text-muted-foreground">GitLab</span>
                          )}
                        </div>
                        <div className="mt-1 text-xs text-muted-foreground">
                          {t("skills.repo.branch")}: {repo.branch || "main"}
                          <span className="ml-3 inline-flex items-center rounded-full border border-border-default px-2 py-0.5 text-[11px]">
                            {t("skills.repo.skillCount", {
                              count: getSkillCount(repo),
                            })}
                          </span>
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
