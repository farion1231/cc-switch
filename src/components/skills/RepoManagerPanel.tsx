import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Trash2,
  ExternalLink,
  Plus,
  Lock,
  Pencil,
  Loader2,
  CheckCircle2,
} from "lucide-react";
import { settingsApi, skillsApi } from "@/lib/api";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import {
  parsePrivateRepoUrl,
  maskToken,
  isPrivateRepo,
} from "./repoUrlParser";
import type { Skill, SkillRepo } from "@/lib/api/skills";

interface RepoManagerPanelProps {
  repos: SkillRepo[];
  skills: Skill[];
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

  // 公共仓库表单状态
  const [publicRepoUrl, setPublicRepoUrl] = useState("");
  const [publicBranch, setPublicBranch] = useState("");
  const [publicError, setPublicError] = useState("");

  // 私有仓库表单状态
  const [privateUrl, setPrivateUrl] = useState("");
  const [privateToken, setPrivateToken] = useState("");
  const [privateBranch, setPrivateBranch] = useState("");
  const [privateError, setPrivateError] = useState("");
  const [isTesting, setIsTesting] = useState(false);
  const [testSuccess, setTestSuccess] = useState(false);
  const [testedAuthHeader, setTestedAuthHeader] = useState<string | null>(null);

  // 编辑私有仓库状态
  const [editingRepo, setEditingRepo] = useState<SkillRepo | null>(null);
  const [editToken, setEditToken] = useState("");
  const [editError, setEditError] = useState("");
  const [isSavingEdit, setIsSavingEdit] = useState(false);

  const getSkillCount = (repo: SkillRepo) =>
    skills.filter(
      (skill) =>
        skill.repoOwner === repo.owner &&
        skill.repoName === repo.name &&
        (skill.repoBranch || "main") === (repo.branch || "main")
    ).length;

  // 公共仓库 URL 解析
  const parsePublicRepoUrl = (
    url: string
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

  // 添加公共仓库
  const handleAddPublic = async () => {
    setPublicError("");

    const parsed = parsePublicRepoUrl(publicRepoUrl);
    if (!parsed) {
      setPublicError(t("skills.repo.invalidUrl"));
      return;
    }

    try {
      await onAdd({
        owner: parsed.owner,
        name: parsed.name,
        branch: publicBranch || "main",
        enabled: true,
      });

      setPublicRepoUrl("");
      setPublicBranch("");
    } catch (e) {
      setPublicError(
        e instanceof Error ? e.message : t("skills.repo.addFailed")
      );
    }
  };

  // 测试私有仓库连接
  const handleTestConnection = async () => {
    setPrivateError("");
    setTestSuccess(false);
    setTestedAuthHeader(null);

    const parsed = parsePrivateRepoUrl(privateUrl);
    if (!parsed) {
      setPrivateError(t("skills.repo.private.invalidUrl"));
      return;
    }

    if (!privateToken.trim()) {
      setPrivateError(t("skills.repo.private.tokenRequired"));
      return;
    }

    setIsTesting(true);

    try {
      const authHeader = await skillsApi.testRepoConnection(
        privateUrl,
        privateToken
      );
      setTestedAuthHeader(authHeader);
      setTestSuccess(true);
    } catch (e) {
      setPrivateError(
        e instanceof Error
          ? e.message
          : t("skills.repo.private.connectionFailed")
      );
    } finally {
      setIsTesting(false);
    }
  };

  // 添加私有仓库
  const handleAddPrivate = async () => {
    setPrivateError("");

    const parsed = parsePrivateRepoUrl(privateUrl);
    if (!parsed) {
      setPrivateError(t("skills.repo.private.invalidUrl"));
      return;
    }

    if (!testedAuthHeader) {
      setPrivateError(t("skills.repo.private.testFirst"));
      return;
    }

    try {
      await onAdd({
        owner: parsed.owner,
        name: parsed.name,
        branch: privateBranch || "main",
        enabled: true,
        base_url: parsed.baseUrl,
        access_token: privateToken,
        auth_header: testedAuthHeader,
      });

      // 重置表单
      setPrivateUrl("");
      setPrivateToken("");
      setPrivateBranch("");
      setTestSuccess(false);
      setTestedAuthHeader(null);
    } catch (e) {
      setPrivateError(
        e instanceof Error ? e.message : t("skills.repo.addFailed")
      );
    }
  };

  // 打开仓库链接
  const handleOpenRepo = async (repo: SkillRepo) => {
    try {
      const baseUrl = repo.base_url || "https://github.com";
      await settingsApi.openExternal(`${baseUrl}/${repo.owner}/${repo.name}`);
    } catch (error) {
      console.error("Failed to open URL:", error);
    }
  };

  // 开始编辑私有仓库
  const handleStartEdit = (repo: SkillRepo) => {
    setEditingRepo(repo);
    setEditToken("");
    setEditError("");
  };

  // 保存编辑的 token
  const handleSaveEdit = async () => {
    if (!editingRepo) return;

    if (!editToken.trim()) {
      setEditError(t("skills.repo.private.tokenRequired"));
      return;
    }

    setIsSavingEdit(true);
    setEditError("");

    try {
      // 先测试新 token 的连接
      const baseUrl = editingRepo.base_url || "https://github.com";
      const testUrl = `${baseUrl}/${editingRepo.owner}/${editingRepo.name}`;
      const authHeader = await skillsApi.testRepoConnection(testUrl, editToken);

      // 删除旧仓库并添加新配置
      await onRemove(editingRepo.owner, editingRepo.name);
      await onAdd({
        ...editingRepo,
        access_token: editToken,
        auth_header: authHeader,
      });

      setEditingRepo(null);
      setEditToken("");
    } catch (e) {
      setEditError(
        e instanceof Error
          ? e.message
          : t("skills.repo.private.updateFailed")
      );
    } finally {
      setIsSavingEdit(false);
    }
  };

  // 当 URL 或 Token 变化时重置测试状态
  const handlePrivateUrlChange = (value: string) => {
    setPrivateUrl(value);
    setTestSuccess(false);
    setTestedAuthHeader(null);
  };

  const handlePrivateTokenChange = (value: string) => {
    setPrivateToken(value);
    setTestSuccess(false);
    setTestedAuthHeader(null);
  };

  return (
    <FullScreenPanel
      isOpen={true}
      title={t("skills.repo.title")}
      onClose={onClose}
    >
      {/* Tab 布局 */}
      <Tabs defaultValue="public" className="w-full">
        <TabsList className="mb-4">
          <TabsTrigger value="public">
            {t("skills.repo.tabs.public")}
          </TabsTrigger>
          <TabsTrigger value="private">
            {t("skills.repo.tabs.private")}
          </TabsTrigger>
        </TabsList>

        {/* 公共仓库 Tab */}
        <TabsContent value="public">
          <div className="space-y-4 glass-card rounded-xl p-6">
            <h3 className="text-base font-semibold text-foreground">
              {t("skills.repo.addPublic")}
            </h3>
            <div className="space-y-4">
              <div>
                <Label htmlFor="public-repo-url" className="text-foreground">
                  {t("skills.repo.url")}
                </Label>
                <Input
                  id="public-repo-url"
                  placeholder={t("skills.repo.urlPlaceholder")}
                  value={publicRepoUrl}
                  onChange={(e) => setPublicRepoUrl(e.target.value)}
                  className="mt-2"
                />
              </div>
              <div>
                <Label htmlFor="public-branch" className="text-foreground">
                  {t("skills.repo.branch")}
                </Label>
                <Input
                  id="public-branch"
                  placeholder={t("skills.repo.branchPlaceholder")}
                  value={publicBranch}
                  onChange={(e) => setPublicBranch(e.target.value)}
                  className="mt-2"
                />
              </div>
              {publicError && (
                <p className="text-sm text-red-600 dark:text-red-400">
                  {publicError}
                </p>
              )}
              <Button
                onClick={handleAddPublic}
                className="bg-primary text-primary-foreground hover:bg-primary/90"
                type="button"
              >
                <Plus className="h-4 w-4 mr-2" />
                {t("skills.repo.add")}
              </Button>
            </div>
          </div>
        </TabsContent>

        {/* 私有仓库 Tab */}
        <TabsContent value="private">
          <div className="space-y-4 glass-card rounded-xl p-6">
            <h3 className="text-base font-semibold text-foreground">
              {t("skills.repo.addPrivate")}
            </h3>
            <div className="space-y-4">
              <div>
                <Label htmlFor="private-url" className="text-foreground">
                  {t("skills.repo.private.url")} *
                </Label>
                <Input
                  id="private-url"
                  placeholder={t("skills.repo.private.urlPlaceholder")}
                  value={privateUrl}
                  onChange={(e) => handlePrivateUrlChange(e.target.value)}
                  className="mt-2"
                />
              </div>
              <div>
                <Label htmlFor="private-token" className="text-foreground">
                  {t("skills.repo.private.token")} *
                </Label>
                <Input
                  id="private-token"
                  type="password"
                  placeholder={t("skills.repo.private.tokenPlaceholder")}
                  value={privateToken}
                  onChange={(e) => handlePrivateTokenChange(e.target.value)}
                  className="mt-2"
                />
              </div>
              <div>
                <Label htmlFor="private-branch" className="text-foreground">
                  {t("skills.repo.branch")}
                </Label>
                <Input
                  id="private-branch"
                  placeholder={t("skills.repo.branchPlaceholder")}
                  value={privateBranch}
                  onChange={(e) => setPrivateBranch(e.target.value)}
                  className="mt-2"
                />
              </div>
              {privateError && (
                <p className="text-sm text-red-600 dark:text-red-400">
                  {privateError}
                </p>
              )}
              {testSuccess && (
                <p className="text-sm text-green-600 dark:text-green-400 flex items-center gap-1">
                  <CheckCircle2 className="h-4 w-4" />
                  {t("skills.repo.private.testSuccess")}
                </p>
              )}
              <div className="flex gap-2">
                <Button
                  onClick={handleTestConnection}
                  variant="outline"
                  type="button"
                  disabled={isTesting || !privateUrl || !privateToken}
                >
                  {isTesting && (
                    <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                  )}
                  {t("skills.repo.private.testConnection")}
                </Button>
                <Button
                  onClick={handleAddPrivate}
                  className="bg-primary text-primary-foreground hover:bg-primary/90"
                  type="button"
                  disabled={!testSuccess}
                >
                  <Plus className="h-4 w-4 mr-2" />
                  {t("skills.repo.add")}
                </Button>
              </div>
            </div>
          </div>
        </TabsContent>
      </Tabs>

      {/* 仓库列表 */}
      <div className="space-y-4 mt-6">
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
                  <div className="text-sm font-medium text-foreground flex items-center gap-2">
                    {repo.owner}/{repo.name}
                    {isPrivateRepo(repo) && (
                      <span className="inline-flex items-center gap-1 rounded-full bg-amber-100 dark:bg-amber-900/30 px-2 py-0.5 text-[11px] text-amber-700 dark:text-amber-400">
                        <Lock className="h-3 w-3" />
                        {t("skills.repo.private.label")}
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
                  {isPrivateRepo(repo) && (
                    <Button
                      variant="ghost"
                      size="icon"
                      type="button"
                      onClick={() => handleStartEdit(repo)}
                      title={t("common.edit")}
                      className="hover:bg-black/5 dark:hover:bg-white/5"
                    >
                      <Pencil className="h-4 w-4" />
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

      {/* 编辑私有仓库弹窗 */}
      {editingRepo && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="bg-background rounded-xl p-6 w-full max-w-md mx-4 space-y-4">
            <h3 className="text-lg font-semibold">
              {t("skills.repo.private.editTitle")}
            </h3>
            <div className="space-y-4">
              <div>
                <Label className="text-foreground">
                  {t("skills.repo.private.repo")}
                </Label>
                <p className="mt-1 text-sm text-muted-foreground">
                  {editingRepo.owner}/{editingRepo.name}
                </p>
              </div>
              <div>
                <Label className="text-foreground">
                  {t("skills.repo.private.currentToken")}
                </Label>
                <p className="mt-1 text-sm text-muted-foreground font-mono">
                  {maskToken(editingRepo.access_token || "")}
                </p>
              </div>
              <div>
                <Label htmlFor="edit-token" className="text-foreground">
                  {t("skills.repo.private.newToken")} *
                </Label>
                <Input
                  id="edit-token"
                  type="password"
                  placeholder={t("skills.repo.private.tokenPlaceholder")}
                  value={editToken}
                  onChange={(e) => setEditToken(e.target.value)}
                  className="mt-2"
                />
              </div>
              {editError && (
                <p className="text-sm text-red-600 dark:text-red-400">
                  {editError}
                </p>
              )}
            </div>
            <div className="flex justify-end gap-2 pt-2">
              <Button
                variant="outline"
                onClick={() => setEditingRepo(null)}
                disabled={isSavingEdit}
              >
                {t("common.cancel")}
              </Button>
              <Button
                onClick={handleSaveEdit}
                disabled={isSavingEdit || !editToken.trim()}
              >
                {isSavingEdit && (
                  <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                )}
                {t("common.save")}
              </Button>
            </div>
          </div>
        </div>
      )}
    </FullScreenPanel>
  );
}
