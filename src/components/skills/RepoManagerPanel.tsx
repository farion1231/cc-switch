import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Trash2, ExternalLink, Plus } from "lucide-react";
import { settingsApi } from "@/lib/api";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import type { DiscoverableSkill, SkillRepo } from "@/lib/api/skills";
import { skillsApi } from "@/lib/api/skills";

interface RepoManagerPanelProps {
  repos: SkillRepo[];
  skills: DiscoverableSkill[];
  onAdd: (repo: SkillRepo) => Promise<void>;
  onRemove: (owner: string, name: string) => Promise<void>;
  onClose: () => void;
}

export function RepoManagerPanel({
  repos,
  skills,
  onAdd,
  onRemove,
  onClose,
}: RepoManagerPanelProps) {
  const { t } = useTranslation();
  const [repoUrl, setRepoUrl] = useState("");
  const [branch, setBranch] = useState("");
  const [error, setError] = useState("");
  const [autoUpdateFreq, setAutoUpdateFreq] = useState("daily");

  useEffect(() => {
    skillsApi.getAutoUpdateFrequency().then(setAutoUpdateFreq).catch(console.error);
  }, []);

  const getSkillCount = (repo: SkillRepo) =>
    skills.filter(
      (skill) =>
        skill.repoOwner === repo.owner &&
        skill.repoName === repo.name &&
        (skill.repoBranch || "main") === (repo.branch || "main"),
    ).length;

  const parseRepoUrl = (
    input: string,
  ): { owner: string; name: string; url: string } | null => {
    let cleaned = input.trim().replace(/\.git$/, "");

    const urlMatch = cleaned.match(/^https?:\/\/([^/]+)\/([^/]+)\/([^/]+?)$/);
    if (urlMatch) {
      const [, host, owner, name] = urlMatch;
      const isGitHub = host!.toLowerCase() === "github.com";
      return { owner: owner!, name: name!, url: isGitHub ? "" : cleaned };
    }

    const parts = cleaned.split("/");
    if (parts.length === 2 && parts[0] && parts[1]) {
      return { owner: parts[0], name: parts[1], url: "" };
    }

    return null;
  };

  const handleAdd = async () => {
    setError("");

    const parsed = parseRepoUrl(repoUrl);
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
        url: parsed.url,
      });

      setRepoUrl("");
      setBranch("");
    } catch (e) {
      setError(e instanceof Error ? e.message : t("skills.repo.addFailed"));
    }
  };

  const handleOpenRepo = async (repo: SkillRepo) => {
    try {
      const webUrl = repo.url || `https://github.com/${repo.owner}/${repo.name}`;
      await settingsApi.openExternal(webUrl);
    } catch (error) {
      console.error("Failed to open URL:", error);
    }
  };

  return (
    <FullScreenPanel
      isOpen={true}
      title={t("skills.repo.title")}
      onClose={onClose}
    >
      {/* 添加仓库表单 */}
      <div className="space-y-4 glass-card rounded-xl p-6">
        <h3 className="text-base font-semibold text-foreground">
          {t("skills.addRepo")}
        </h3>
        <div className="space-y-4">
          <div>
            <Label htmlFor="repo-url" className="text-foreground">
              {t("skills.repo.url")}
            </Label>
            <Input
              id="repo-url"
              placeholder={t("skills.repo.urlPlaceholder")}
              value={repoUrl}
              onChange={(e) => setRepoUrl(e.target.value)}
              className="mt-2"
            />
          </div>
          <div>
            <Label htmlFor="branch" className="text-foreground">
              {t("skills.repo.branch")}
            </Label>
            <Input
              id="branch"
              placeholder={t("skills.repo.branchPlaceholder")}
              value={branch}
              onChange={(e) => setBranch(e.target.value)}
              className="mt-2"
            />
          </div>
          {error && (
            <p className="text-sm text-red-600 dark:text-red-400">{error}</p>
          )}
          <Button
            onClick={handleAdd}
            className="bg-primary text-primary-foreground hover:bg-primary/90"
            type="button"
          >
            <Plus className="h-4 w-4 mr-2" />
            {t("skills.repo.add")}
          </Button>
        </div>
      </div>

      {/* 仓库列表 */}
      <div className="space-y-4">
        <h3 className="text-base font-semibold text-foreground">
          {t("skills.repo.list")}
        </h3>
        {repos.length === 0 ? (
          <div className="text-center py-12 glass-card rounded-xl">
            <p className="text-sm text-muted-foreground">
              {t("skills.repo.empty")}
            </p>
          </div>
        ) : (
          <div className="space-y-3">
            {repos.map((repo) => (
              <div
                key={`${repo.owner}/${repo.name}`}
                className="flex items-center justify-between glass-card rounded-xl px-4 py-3"
              >
                <div>
                  <div className="text-sm font-medium text-foreground">
                    {repo.url ? (
                      <>
                        <span className="text-muted-foreground">
                          {new URL(repo.url).host}/
                        </span>
                        {repo.owner}/{repo.name}
                      </>
                    ) : (
                      <>{repo.owner}/{repo.name}</>
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
                    className="hover:bg-black/5 dark:hover:bg-white/5"
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

      {/* 自动更新设置 */}
      <div className="space-y-4 glass-card rounded-xl p-6">
        <h3 className="text-base font-semibold text-foreground">
          {t("skills.autoUpdate.title")}
        </h3>
        <div className="flex items-center gap-3">
          <Label htmlFor="auto-update-freq" className="text-foreground whitespace-nowrap">
            {t("skills.autoUpdate.frequency")}
          </Label>
          <select
            id="auto-update-freq"
            value={autoUpdateFreq}
            onChange={async (e) => {
              const val = e.target.value;
              setAutoUpdateFreq(val);
              try {
                await skillsApi.setAutoUpdateFrequency(val);
              } catch (err) {
                console.error("Failed to set auto-update frequency:", err);
              }
            }}
            className="flex h-9 rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <option value="on_launch">{t("skills.autoUpdate.onLaunch")}</option>
            <option value="daily">{t("skills.autoUpdate.daily")}</option>
            <option value="weekly">{t("skills.autoUpdate.weekly")}</option>
            <option value="monthly">{t("skills.autoUpdate.monthly")}</option>
            <option value="off">{t("skills.autoUpdate.off")}</option>
          </select>
        </div>
      </div>
    </FullScreenPanel>
  );
}
