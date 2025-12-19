import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { Trash2, ExternalLink, Plus, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { settingsApi } from "@/lib/api";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import {
  useTemplateRepos,
  useAddTemplateRepo,
  useRemoveTemplateRepo,
  useToggleTemplateRepo,
} from "@/lib/query/template";

interface RepoManagerProps {
  onClose: () => void;
}

export function RepoManager({ onClose }: RepoManagerProps) {
  const { t } = useTranslation();
  const [repoUrl, setRepoUrl] = useState("");
  const [branch, setBranch] = useState("");
  const [error, setError] = useState("");

  const { data: repos = [], isLoading } = useTemplateRepos();
  const addRepoMutation = useAddTemplateRepo();
  const removeRepoMutation = useRemoveTemplateRepo();
  const toggleRepoMutation = useToggleTemplateRepo();

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
      setError(
        t("templates.repo.invalidUrl", {
          defaultValue: "仓库地址格式不正确",
        }),
      );
      return;
    }

    try {
      await addRepoMutation.mutateAsync({
        owner: parsed.owner,
        name: parsed.name,
        branch: branch || "main",
      });

      toast.success(
        t("templates.repo.addSuccess", {
          owner: parsed.owner,
          name: parsed.name,
          defaultValue: `已添加仓库 ${parsed.owner}/${parsed.name}`,
        }),
      );

      setRepoUrl("");
      setBranch("");
    } catch (e) {
      const errorMessage = e instanceof Error ? e.message : String(e);
      setError(
        t("templates.repo.addFailed", {
          defaultValue: "添加仓库失败",
        }),
      );
      toast.error(
        t("templates.repo.addFailed", { defaultValue: "添加仓库失败" }),
        {
          description: errorMessage,
          duration: 8000,
        },
      );
    }
  };

  const handleRemove = async (id: number, owner: string, name: string) => {
    try {
      await removeRepoMutation.mutateAsync(id);
      toast.success(
        t("templates.repo.removeSuccess", {
          owner,
          name,
          defaultValue: `已移除仓库 ${owner}/${name}`,
        }),
      );
    } catch (e) {
      const errorMessage = e instanceof Error ? e.message : String(e);
      toast.error(
        t("templates.repo.removeFailed", { defaultValue: "移除仓库失败" }),
        {
          description: errorMessage,
          duration: 8000,
        },
      );
    }
  };

  const handleToggle = async (id: number, enabled: boolean) => {
    try {
      await toggleRepoMutation.mutateAsync({ id, enabled });
      toast.success(
        enabled
          ? t("templates.repo.enableSuccess", { defaultValue: "已启用仓库" })
          : t("templates.repo.disableSuccess", { defaultValue: "已禁用仓库" }),
      );
    } catch (e) {
      const errorMessage = e instanceof Error ? e.message : String(e);
      toast.error(
        t("templates.repo.toggleFailed", {
          defaultValue: "切换仓库状态失败",
        }),
        {
          description: errorMessage,
          duration: 8000,
        },
      );
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
      title={t("templates.repo.title", { defaultValue: "模板仓库管理" })}
      onClose={onClose}
    >
      {/* 添加仓库表单 */}
      <div className="space-y-4 glass-card rounded-xl p-6">
        <h3 className="text-base font-semibold text-foreground">
          {t("templates.repo.addTitle", { defaultValue: "添加模板仓库" })}
        </h3>
        <div className="space-y-4">
          <div>
            <Label htmlFor="repo-url" className="text-foreground">
              {t("templates.repo.url", { defaultValue: "仓库地址" })}
            </Label>
            <Input
              id="repo-url"
              placeholder={t("templates.repo.urlPlaceholder", {
                defaultValue: "owner/repo 或 https://github.com/owner/repo",
              })}
              value={repoUrl}
              onChange={(e) => setRepoUrl(e.target.value)}
              className="mt-2"
            />
          </div>
          <div>
            <Label htmlFor="branch" className="text-foreground">
              {t("templates.repo.branch", { defaultValue: "分支" })}
            </Label>
            <Input
              id="branch"
              placeholder={t("templates.repo.branchPlaceholder", {
                defaultValue: "main",
              })}
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
            disabled={addRepoMutation.isPending}
            className="bg-primary text-primary-foreground hover:bg-primary/90"
            type="button"
          >
            {addRepoMutation.isPending ? (
              <Loader2 className="h-4 w-4 mr-2 animate-spin" />
            ) : (
              <Plus className="h-4 w-4 mr-2" />
            )}
            {t("templates.repo.add", { defaultValue: "添加仓库" })}
          </Button>
        </div>
      </div>

      {/* 仓库列表 */}
      <div className="space-y-4">
        <h3 className="text-base font-semibold text-foreground">
          {t("templates.repo.list", { defaultValue: "仓库列表" })}
        </h3>
        {isLoading ? (
          <div className="flex items-center justify-center py-12 glass-card rounded-xl">
            <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
          </div>
        ) : repos.length === 0 ? (
          <div className="text-center py-12 glass-card rounded-xl">
            <p className="text-sm text-muted-foreground">
              {t("templates.repo.empty", { defaultValue: "暂无仓库" })}
            </p>
          </div>
        ) : (
          <div className="space-y-3">
            {repos.map((repo) => (
              <div
                key={repo.id ?? `${repo.owner}-${repo.name}`}
                className="flex items-center justify-between glass-card rounded-xl px-4 py-3"
              >
                <div className="flex items-center gap-4 flex-1 min-w-0">
                  <Switch
                    checked={repo.enabled}
                    onCheckedChange={(checked) =>
                      repo.id !== null && handleToggle(repo.id, checked)
                    }
                    disabled={toggleRepoMutation.isPending || repo.id === null}
                  />
                  <div className="flex-1 min-w-0">
                    <div className="text-sm font-medium text-foreground">
                      {repo.owner}/{repo.name}
                    </div>
                    <div className="mt-1 flex items-center gap-2">
                      <span className="text-xs text-muted-foreground">
                        {t("templates.repo.branch", { defaultValue: "分支" })}:{" "}
                        {repo.branch}
                      </span>
                      <Badge
                        variant={repo.enabled ? "default" : "secondary"}
                        className="text-[10px] px-1.5 py-0 h-4"
                      >
                        {repo.enabled
                          ? t("templates.repo.enabled", {
                              defaultValue: "已启用",
                            })
                          : t("templates.repo.disabled", {
                              defaultValue: "已禁用",
                            })}
                      </Badge>
                    </div>
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
                    onClick={() =>
                      repo.id !== null &&
                      handleRemove(repo.id, repo.owner, repo.name)
                    }
                    disabled={removeRepoMutation.isPending || repo.id === null}
                    title={t("common.delete", { defaultValue: "删除" })}
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
