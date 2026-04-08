import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Trash2, ExternalLink, Plus } from "lucide-react";
import { settingsApi } from "@/lib/api";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import type { DiscoverableRule, RuleRepo } from "@/lib/api/rules";

interface RuleRepoManagerPanelProps {
  repos: RuleRepo[];
  rules: DiscoverableRule[];
  onAdd: (repo: RuleRepo) => Promise<void>;
  onRemove: (owner: string, name: string) => Promise<void>;
  onClose: () => void;
}

export function RuleRepoManagerPanel({
  repos,
  rules,
  onAdd,
  onRemove,
  onClose,
}: RuleRepoManagerPanelProps) {
  const { t } = useTranslation();
  const [repoUrl, setRepoUrl] = useState("");
  const [branch, setBranch] = useState("");
  const [error, setError] = useState("");

  const getRuleCount = (repo: RuleRepo) =>
    rules.filter(
      (rule) =>
        rule.repoOwner === repo.owner &&
        rule.repoName === repo.name &&
        (rule.repoBranch || "main") === (repo.branch || "main"),
    ).length;

  const parseRepoUrl = (
    url: string,
  ): { owner: string; name: string } | null => {
    let cleaned = url.trim();
    cleaned = cleaned.replace(/^https?:\/\/github\.com\//, "");
    cleaned = cleaned.replace(/\.git$/, "");

    const parts = cleaned.split("/");
    if (parts.length === 2 && parts[0] && parts[1]) {
      return { owner: parts[0], name: parts[1] };
    }

    return null;
  };

  const handleAdd = async () => {
    setError("");

    const parsed = parseRepoUrl(repoUrl);
    if (!parsed) {
      setError(t("rules.repo.invalidUrl"));
      return;
    }

    try {
      await onAdd({
        owner: parsed.owner,
        name: parsed.name,
        branch: branch || "main",
        enabled: true,
      });

      setRepoUrl("");
      setBranch("");
    } catch (e) {
      setError(e instanceof Error ? e.message : t("rules.repo.addFailed"));
    }
  };

  const handleOpenRepo = async (owner: string, name: string) => {
    try {
      await settingsApi.openExternal(`https://github.com/${owner}/${name}`);
    } catch (error) {
      console.error("Failed to open URL:", error);
    }
  };

  return (
    <FullScreenPanel
      isOpen={true}
      title={t("rules.repo.title")}
      onClose={onClose}
    >
      <div className="space-y-4 glass-card rounded-xl p-6">
        <h3 className="text-base font-semibold text-foreground">
          {t("rules.addRepo")}
        </h3>
        <div className="space-y-4">
          <div>
            <Label htmlFor="rule-repo-url" className="text-foreground">
              {t("rules.repo.url")}
            </Label>
            <Input
              id="rule-repo-url"
              placeholder={t("rules.repo.urlPlaceholder")}
              value={repoUrl}
              onChange={(e) => setRepoUrl(e.target.value)}
              className="mt-2"
            />
          </div>
          <div>
            <Label htmlFor="rule-branch" className="text-foreground">
              {t("rules.repo.branch")}
            </Label>
            <Input
              id="rule-branch"
              placeholder={t("rules.repo.branchPlaceholder")}
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
            {t("rules.repo.add")}
          </Button>
        </div>
      </div>

      <div className="space-y-4">
        <h3 className="text-base font-semibold text-foreground">
          {t("rules.repo.list")}
        </h3>
        {repos.length === 0 ? (
          <div className="text-center py-12 glass-card rounded-xl">
            <p className="text-sm text-muted-foreground">
              {t("rules.repo.empty")}
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
                    {repo.owner}/{repo.name}
                  </div>
                  <div className="mt-1 text-xs text-muted-foreground">
                    {t("rules.repo.branch")}: {repo.branch || "main"}
                    <span className="ml-3 inline-flex items-center rounded-full border border-border-default px-2 py-0.5 text-[11px]">
                      {t("rules.repo.ruleCount", {
                        count: getRuleCount(repo),
                      })}
                    </span>
                  </div>
                </div>
                <div className="flex gap-2">
                  <Button
                    variant="ghost"
                    size="icon"
                    type="button"
                    onClick={() => handleOpenRepo(repo.owner, repo.name)}
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
    </FullScreenPanel>
  );
}
