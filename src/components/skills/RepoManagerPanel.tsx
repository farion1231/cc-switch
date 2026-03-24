import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Trash2, ExternalLink, Plus } from "lucide-react";
import { settingsApi } from "@/lib/api";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import type { DiscoverableSkill, SkillRepo } from "@/lib/api/skills";

type SourceType = "github" | "gitee" | "gitlab" | "zipUrl";

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
  const [sourceType, setSourceType] = useState<SourceType>("github");

  // GitHub/Gitee/GitLab 模式状态
  const [repoUrl, setRepoUrl] = useState("");
  const [branch, setBranch] = useState("main");

  // 自定义 ZIP 模式状态
  const [zipUrl, setZipUrl] = useState("");
  const [customOwner, setCustomOwner] = useState("");
  const [customName, setCustomName] = useState("");
  const [websiteUrl, setWebsiteUrl] = useState("");

  const [error, setError] = useState("");

  const getSkillCount = (repo: SkillRepo) =>
    skills.filter(
      (skill) =>
        skill.repoOwner === repo.owner && skill.repoName === repo.name,
    ).length;

  // 根据 source 生成仓库官网链接
  const getRepoWebsiteUrl = (source: string | undefined, owner: string, name: string): string => {
    if (source === "gitee") {
      return `https://gitee.com/${owner}/${name}`;
    }
    if (source === "gitlab") {
      return `https://gitlab.com/${owner}/${name}`;
    }
    // 默认 GitHub
    return `https://github.com/${owner}/${name}`;
  };

  // 解析 GitHub/Gitee/GitLab URL，提取 owner/name
  const parseRepoUrl = (url: string): { owner: string; name: string } | null => {
    let cleaned = url.trim();
    // 支持 https://github.com/owner/name 或 https://gitee.com/owner/name 或 https://gitlab.com/owner/name 格式
    cleaned = cleaned.replace(/^https?:\/\//, "");
    cleaned = cleaned.replace(/\.git$/, "");
    cleaned = cleaned.replace(/\/$/, "");

    // 移除域名部分
    cleaned = cleaned.replace(/^(github\.com|gitee\.com|gitlab\.com)\//, "");

    const parts = cleaned.split("/");
    if (parts.length >= 2 && parts[0] && parts[1]) {
      return { owner: parts[0], name: parts[1] };
    }
    return null;
  };

  const validateRepoInput = (): boolean => {
    setError("");

    if (!repoUrl.trim()) {
      setError(t("skills.repo.urlRequired"));
      return false;
    }

    const parsed = parseRepoUrl(repoUrl);
    if (!parsed) {
      setError(t("skills.repo.invalidUrl"));
      return false;
    }

    if (repos.some((r) => r.owner === parsed.owner && r.name === parsed.name)) {
      setError(t("skills.repo.exists"));
      return false;
    }

    return true;
  };

  const validateCustomInput = (): boolean => {
    setError("");

    if (!zipUrl.trim()) {
      setError(t("skills.repo.zipUrlRequired"));
      return false;
    }

    try {
      const url = new URL(zipUrl);
      if (!url.protocol.startsWith("http")) {
        setError(t("skills.repo.invalidZipUrl"));
        return false;
      }
    } catch {
      setError(t("skills.repo.invalidZipUrl"));
      return false;
    }

    if (!customOwner.trim() || !customName.trim()) {
      setError(t("skills.repo.idRequired"));
      return false;
    }

    if (repos.some((r) => r.owner === customOwner.trim() && r.name === customName.trim())) {
      setError(t("skills.repo.exists"));
      return false;
    }

    return true;
  };

  const handleAdd = async () => {
    if (sourceType === "github" || sourceType === "gitee" || sourceType === "gitlab") {
      if (!validateRepoInput()) return;

      const parsed = parseRepoUrl(repoUrl)!;
      try {
        await onAdd({
          owner: parsed.owner,
          name: parsed.name,
          branch: branch.trim() || "main",
          enabled: true,
          zipUrl: null,
          websiteUrl: null,
          source: sourceType,
        });

        setRepoUrl("");
        setBranch("main");
      } catch (e) {
        setError(e instanceof Error ? e.message : t("skills.repo.addFailed"));
      }
    } else {
      if (!validateCustomInput()) return;

      try {
        await onAdd({
          owner: customOwner.trim(),
          name: customName.trim(),
          branch: "main",
          zipUrl: zipUrl.trim(),
          websiteUrl: websiteUrl.trim() || null,
          enabled: true,
          source: sourceType,
        });

        setZipUrl("");
        setCustomOwner("");
        setCustomName("");
        setWebsiteUrl("");
      } catch (e) {
        setError(e instanceof Error ? e.message : t("skills.repo.addFailed"));
      }
    }
  };

  const handleOpenWebsite = async (url: string) => {
    try {
      await settingsApi.openExternal(url);
    } catch (error) {
      console.error("Failed to open URL:", error);
    }
  };

  const handleSourceTypeChange = (value: string) => {
    setSourceType(value as SourceType);
    setError("");
  };

  return (
    <FullScreenPanel
      isOpen={true}
      title={t("skills.repo.title")}
      onClose={onClose}
    >
      {/* 添加技能源表单 */}
      <div className="space-y-4 glass-card rounded-xl p-6">
        <h3 className="text-base font-semibold text-foreground">
          {t("skills.addRepo")}
        </h3>
        <div className="space-y-4">
          {/* 来源类型选择 */}
          <div>
            <Label className="text-foreground">{t("skills.repo.sourceType")}</Label>
            <Select value={sourceType} onValueChange={handleSourceTypeChange}>
              <SelectTrigger className="mt-2">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="github">GitHub</SelectItem>
                <SelectItem value="gitee">Gitee</SelectItem>
                <SelectItem value="gitlab">GitLab</SelectItem>
                <SelectItem value="zipUrl">{t("skills.repo.source.zipUrl")}</SelectItem>
              </SelectContent>
            </Select>
          </div>

          {/* GitHub / Gitee / GitLab 模式表单 */}
          {(sourceType === "github" || sourceType === "gitee" || sourceType === "gitlab") && (
            <>
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
            </>
          )}

          {/* 自定义 ZIP 模式表单 */}
          {sourceType === "zipUrl" && (
            <>
              <div>
                <Label htmlFor="zip-url" className="text-foreground">
                  {t("skills.repo.zipUrl")}
                </Label>
                <Input
                  id="zip-url"
                  placeholder={t("skills.repo.zipUrlPlaceholder")}
                  value={zipUrl}
                  onChange={(e) => setZipUrl(e.target.value)}
                  className="mt-2"
                />
              </div>
              <div>
                <Label htmlFor="repo-id" className="text-foreground">
                  {t("skills.repo.id")}
                </Label>
                <div className="flex gap-2 mt-2">
                  <Input
                    id="repo-owner"
                    placeholder={t("skills.repo.ownerPlaceholder")}
                    value={customOwner}
                    onChange={(e) => setCustomOwner(e.target.value)}
                    className="flex-1"
                  />
                  <span className="flex items-center text-muted-foreground">/</span>
                  <Input
                    id="repo-name"
                    placeholder={t("skills.repo.namePlaceholder")}
                    value={customName}
                    onChange={(e) => setCustomName(e.target.value)}
                    className="flex-1"
                  />
                </div>
              </div>
              <div>
                <Label htmlFor="website-url" className="text-foreground">
                  {t("skills.repo.websiteUrl")}
                </Label>
                <Input
                  id="website-url"
                  placeholder={t("skills.repo.websiteUrlPlaceholder")}
                  value={websiteUrl}
                  onChange={(e) => setWebsiteUrl(e.target.value)}
                  className="mt-2"
                />
              </div>
            </>
          )}

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

      {/* 技能源列表 */}
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
                <div className="min-w-0 flex-1">
                  <div className="text-sm font-medium text-foreground">
                    {repo.owner}/{repo.name}
                  </div>
                  <div className="mt-1 text-xs text-muted-foreground truncate max-w-[400px]">
                    {repo.zipUrl || getRepoWebsiteUrl(repo.source, repo.owner, repo.name)}
                  </div>
                </div>
                <div className="flex gap-2 flex-shrink-0 ml-2">
                  <span className="inline-flex items-center rounded-full border border-border-default px-2 py-0.5 text-[11px]">
                    {t("skills.repo.skillCount", {
                      count: getSkillCount(repo),
                    })}
                  </span>
                  {(repo.websiteUrl || !repo.zipUrl) && (
                    <Button
                      variant="ghost"
                      size="icon"
                      type="button"
                      onClick={() =>
                        handleOpenWebsite(
                          repo.websiteUrl ||
                            getRepoWebsiteUrl(repo.source, repo.owner, repo.name),
                        )
                      }
                      title={t("common.view", { defaultValue: "查看" })}
                      className="hover:bg-black/5 dark:hover:bg-white/5"
                    >
                      <ExternalLink className="h-4 w-4" />
                    </Button>
                  )}
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
    </FullScreenPanel>
  );
}
