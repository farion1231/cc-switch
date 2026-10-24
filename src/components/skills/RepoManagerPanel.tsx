import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ExternalLink, Eye, EyeOff, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetBody,
  SheetContent,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { HoverTip } from "@/components/ui/hover-tip";
import { DialogTitle } from "@/components/ui/dialog";
import { HelpTip } from "@/components/ui/help-tip";
import { settingsApi } from "@/lib/api";
import type {
  DiscoverableSkill,
  GitCredential,
  SkillRepo,
} from "@/lib/api/skills";
import {
  useCreateGitCredential,
  useDeleteGitCredential,
  useGitCredentials,
} from "@/hooks/useSkills";
import {
  formatSkillError,
  parseSkillError,
} from "@/lib/errors/skillErrorParser";

/** 这几种失败都意味着"令牌那一栏需要动"，其余失败不该让令牌下拉框背锅 */
const CREDENTIAL_ERROR_CODES = new Set([
  "AUTH_REQUIRED",
  "TOKEN_REJECTED",
  "CREDENTIAL_INVALID",
  "KEYRING_UNAVAILABLE",
]);
import { cn } from "@/lib/utils";
import {
  CHECKBOX_CLASS,
  FieldError,
  LABEL_CLASS,
  MONO_FIELD_CLASS,
  V7ConfirmDialog,
  V7Dialog,
} from "@/components/mcp/formBits";

/**
 * 下拉框里的「添加令牌」项。
 *
 * 用哨兵值而不是另开一个按钮：下拉框既要能选已有令牌、也要能新增，选中它只负责
 * 打开对话框，不改变当前选择。
 */
const ADD_TOKEN_VALUE = "__add_token__";

interface RepoManagerPanelProps {
  repos: SkillRepo[];
  skills: DiscoverableSkill[];
  onAdd: (repo: SkillRepo) => Promise<void>;
  onRemove: (owner: string, name: string) => Promise<void>;
  /** 勾掉 = 停用（不参与发现），不删除 */
  onToggle?: (repo: SkillRepo, enabled: boolean) => Promise<void>;
  onClose: () => void;
}

export function parseRepoUrl(
  url: string,
): { owner: string; name: string } | null {
  let cleaned = url.trim();
  cleaned = cleaned.replace(/^https?:\/\/github\.com\//, "");
  cleaned = cleaned.replace(/\.git$/, "").replace(/\/+$/, "");
  const parts = cleaned.split("/");
  if (parts.length === 2 && parts[0] && parts[1]) {
    return { owner: parts[0], name: parts[1] };
  }
  return null;
}

export function countRepoSkills(skills: DiscoverableSkill[], repo: SkillRepo) {
  return skills.filter(
    (skill) =>
      skill.repoOwner === repo.owner &&
      skill.repoName === repo.name &&
      (skill.repoBranch || "main") === (repo.branch || "main"),
  ).length;
}

/**
 * 令牌输入框：默认打码，右侧按钮可切明文。
 *
 * 没有复用 provider 表单的 `ApiKeyInput`——那个组件的标签样式属于那边的一套，
 * 拿进来会让这个对话框里「名称」和「访问令牌」两个标签字号不一致。这里沿用它的
 * 明文切换写法，外观与本面板其余字段保持一致。
 */
function TokenField({
  id,
  value,
  onChange,
  invalid,
  describedBy,
}: {
  id: string;
  value: string;
  onChange: (value: string) => void;
  invalid?: boolean;
  describedBy?: string;
}) {
  const { t } = useTranslation();
  const [revealed, setRevealed] = useState(false);

  return (
    <div className="relative">
      <input
        id={id}
        type={revealed ? "text" : "password"}
        className={cn(MONO_FIELD_CLASS, "pe-10")}
        placeholder={t("skills.credential.tokenPlaceholder")}
        value={value}
        aria-invalid={invalid || undefined}
        aria-describedby={describedBy}
        autoComplete="off"
        spellCheck={false}
        onChange={(event) => onChange(event.target.value)}
      />
      {value && (
        <button
          type="button"
          onClick={() => setRevealed((previous) => !previous)}
          className="absolute inset-y-0 end-0 flex items-center pe-3 text-fg-3 transition-colors hover:text-fg-1"
          aria-label={
            revealed ? t("skills.credential.hide") : t("skills.credential.show")
          }
        >
          {revealed ? <EyeOff size={16} /> : <Eye size={16} />}
        </button>
      )}
    </div>
  );
}

/** 新建令牌的小对话框：名称 + 令牌。 */
function AddTokenDialog({
  open,
  onOpenChange,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreated: (credential: GitCredential) => void;
}) {
  const { t } = useTranslation();
  const createCredential = useCreateGitCredential();
  const [name, setName] = useState("");
  const [token, setToken] = useState("");
  const [error, setError] = useState("");

  const close = () => {
    setName("");
    setToken("");
    setError("");
    onOpenChange(false);
  };

  const handleSave = async () => {
    setError("");
    try {
      const created = await createCredential.mutateAsync({ name, token });
      onCreated(created);
      close();
    } catch (e) {
      setError(
        formatSkillError(String(e), t, "skills.credential.saveFailed")
          .description,
      );
    }
  };

  return (
    <V7Dialog
      open={open}
      onOpenChange={(next) => (next ? onOpenChange(true) : close())}
      describedBy={undefined}
      width={420}
    >
      <DialogTitle className="text-section">
        {t("skills.credential.addTitle")}
      </DialogTitle>

      <div className="flex flex-col gap-1.5">
        <label htmlFor="sk-cred-name" className={LABEL_CLASS}>
          {t("skills.credential.name")}
        </label>
        <input
          id="sk-cred-name"
          className={MONO_FIELD_CLASS}
          placeholder={t("skills.credential.namePlaceholder")}
          maxLength={60}
          value={name}
          aria-invalid={Boolean(error)}
          aria-describedby={error ? "sk-cred-dialog-error" : undefined}
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => setName(event.target.value)}
        />
      </div>

      <div className="flex flex-col gap-1.5">
        <label htmlFor="sk-cred-token" className={LABEL_CLASS}>
          {t("skills.credential.token")}
        </label>
        <TokenField
          id="sk-cred-token"
          value={token}
          onChange={setToken}
          invalid={Boolean(error)}
          describedBy={error ? "sk-cred-dialog-error" : undefined}
        />
      </div>

      {error && (
        <FieldError id="sk-cred-dialog-error" role="alert">
          {error}
        </FieldError>
      )}

      <div className="flex justify-end gap-2">
        <Button type="button" variant="quiet" size="regular" onClick={close}>
          {t("skills.credential.cancel")}
        </Button>
        <Button
          type="button"
          variant="neutral"
          size="regular"
          disabled={createCredential.isPending || !name.trim() || !token.trim()}
          onClick={() => void handleSave()}
        >
          {t("skills.credential.save")}
        </Button>
      </div>
    </V7Dialog>
  );
}

/** 访问令牌：下拉框选已有令牌，或选「添加令牌」开对话框。勾了私有仓库才出现。 */
function CredentialPicker({
  value,
  onChange,
  invalid,
  describedBy,
}: {
  /** 选中的凭据 id；空串表示还没选 */
  value: string;
  onChange: (credentialId: string) => void;
  /** 添加失败时把错误也关联到下拉框上：私有仓库的预检错误通常出在凭据这边 */
  invalid?: boolean;
  describedBy?: string;
}) {
  const { t } = useTranslation();
  const {
    data: credentials = [],
    isPending,
    isError,
    refetch,
  } = useGitCredentials();
  const deleteCredential = useDeleteGitCredential();
  const [addOpen, setAddOpen] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<GitCredential | null>(
    null,
  );
  const [error, setError] = useState("");

  const selected = credentials.find((credential) => credential.id === value);

  const handleValueChange = (next: string) => {
    if (next === ADD_TOKEN_VALUE) {
      // 只开对话框，不动当前选择
      setAddOpen(true);
      return;
    }
    // 换了一枚令牌，上一次删除失败的提示就不再有意义
    setError("");
    onChange(next);
  };

  const handleDelete = async () => {
    if (!pendingDelete) return;
    setError("");
    try {
      await deleteCredential.mutateAsync(pendingDelete.id);
      // 删掉的正是当前选中的那枚，选择跟着清空
      if (pendingDelete.id === value) onChange("");
      setPendingDelete(null);
    } catch (e) {
      setError(
        formatSkillError(String(e), t, "skills.credential.removeFailed")
          .description,
      );
      setPendingDelete(null);
    }
  };

  return (
    <section aria-labelledby="sk-cred-title" className="flex flex-col gap-2">
      <div className="flex items-center gap-1.5">
        <span id="sk-cred-title" className={LABEL_CLASS}>
          {t("skills.credential.label")}
        </span>
        <HelpTip title={t("skills.credential.helpTitle")}>
          {t("skills.credential.helpBody")}
        </HelpTip>
      </div>

      <div className="flex items-center gap-2">
        <Select value={value} onValueChange={handleValueChange}>
          <SelectTrigger
            id="sk-cred-select"
            aria-label={t("skills.credential.label")}
            aria-invalid={invalid || Boolean(error)}
            aria-describedby={
              error ? "sk-cred-picker-error" : invalid ? describedBy : undefined
            }
            className="flex-1"
          >
            <SelectValue placeholder={t("skills.credential.placeholder")} />
          </SelectTrigger>
          <SelectContent>
            {credentials.map((credential) => (
              <SelectItem key={credential.id} value={credential.id}>
                {credential.name}
              </SelectItem>
            ))}
            <SelectItem value={ADD_TOKEN_VALUE}>
              ＋ {t("skills.credential.add")}
            </SelectItem>
          </SelectContent>
        </Select>

        {selected && (
          <HoverTip content={t("skills.credential.remove")}>
            <Button
              type="button"
              variant="quiet"
              size="icon-compact"
              aria-label={t("skills.credential.remove")}
              className="hover:text-danger-text"
              onClick={() => setPendingDelete(selected)}
            >
              <Trash2 className="h-4 w-4" strokeWidth={1.5} />
            </Button>
          </HoverTip>
        )}
      </div>

      {/* 首帧还在加载时不要说"还没有令牌"：这个组件是勾上私有仓库才挂载的，
          列表请求正是那一刻才发出去 */}
      {isError ? (
        <p className="m-0 text-caption text-danger-text">
          {t("skills.credential.loadFailed")}
          <button
            type="button"
            className="ms-1 underline"
            onClick={() => void refetch()}
          >
            {t("common.retry")}
          </button>
        </p>
      ) : (
        !isPending &&
        credentials.length === 0 && (
          <p className="m-0 text-caption text-fg-2">
            {t("skills.credential.empty")}
          </p>
        )
      )}
      {error && (
        <FieldError id="sk-cred-picker-error" role="alert">
          {error}
        </FieldError>
      )}

      <AddTokenDialog
        open={addOpen}
        onOpenChange={setAddOpen}
        onCreated={(credential) => onChange(credential.id)}
      />

      <V7ConfirmDialog
        open={pendingDelete !== null}
        title={t("skills.credential.removeTitle")}
        body={t("skills.credential.removeBody", {
          name: pendingDelete?.name ?? "",
        })}
        confirmLabel={t("skills.credential.removeConfirm")}
        pending={deleteCredential.isPending}
        onConfirm={() => void handleDelete()}
        onCancel={() => setPendingDelete(null)}
      />
    </section>
  );
}

/** 仓库管理（右侧抽屉，宽 420）：添加、停用、删除 Skill 仓库，配置私有仓库凭据。 */
export function RepoManagerPanel({
  repos,
  skills,
  onAdd,
  onRemove,
  onToggle,
  onClose,
}: RepoManagerPanelProps) {
  const { t } = useTranslation();
  const [repoUrl, setRepoUrl] = useState("");
  const [branch, setBranch] = useState("");
  const [isPrivate, setIsPrivate] = useState(false);
  const [credentialId, setCredentialId] = useState("");
  // 上一次回填来自哪个仓库。只在"匹配的仓库变了"时才动表单，
  // 用户自己勾的私有、自己选的令牌不会被一次次击键清掉。
  const [backfilledFrom, setBackfilledFrom] = useState("");
  const [error, setError] = useState("");
  // 只有失败真的落在凭据上时才让令牌下拉框背这个错：URL 写错或分支不存在，
  // 令牌本身是好的。错误码只能在 catch 里从原始错误取——存进 state 的是
  // 本地化后的整段文案，已经不是 JSON 了。
  const [credentialFailed, setCredentialFailed] = useState(false);
  const [adding, setAdding] = useState(false);

  const handleAdd = async () => {
    setError("");
    setCredentialFailed(false);
    const parsed = parseRepoUrl(repoUrl);
    if (!parsed) {
      setError(t("skills.repo.invalidUrl"));
      return;
    }
    setAdding(true);
    try {
      await onAdd({
        owner: parsed.owner,
        name: parsed.name,
        branch: branch.trim() || "main",
        enabled: true,
        private: isPrivate,
        credentialId: isPrivate && credentialId ? credentialId : null,
      });
      setRepoUrl("");
      setBranch("");
      setIsPrivate(false);
      setCredentialId("");
      setBackfilledFrom("");
    } catch (e) {
      // 私有仓库的预检失败（凭据缺失、无效、无权访问）是结构化错误，按错误码出文案
      const raw = String(e);
      const code = parseSkillError(raw)?.code;
      setCredentialFailed(
        code !== undefined && CREDENTIAL_ERROR_CODES.has(code),
      );
      setError(formatSkillError(raw, t, "skills.repo.addFailed").description);
    } finally {
      setAdding(false);
    }
  };

  const handleOpenRepo = async (owner: string, name: string) => {
    try {
      await settingsApi.openExternal(`https://github.com/${owner}/${name}`);
    } catch (err) {
      console.error("Failed to open URL:", err);
    }
  };

  return (
    <Sheet open onOpenChange={(open) => !open && onClose()}>
      <SheetContent
        width={420}
        closeLabel={t("common.close")}
        aria-describedby={undefined}
      >
        <SheetHeader className="flex h-[52px] items-center space-y-0 border-b border-border pe-12 ps-6 pt-0">
          <SheetTitle>{t("skills.repo.title")}</SheetTitle>
        </SheetHeader>
        <SheetBody className="flex flex-col gap-6 px-6 pb-6 pt-5">
          <section
            aria-labelledby="sk-repo-add-title"
            className="flex flex-col gap-3"
          >
            <h3
              id="sk-repo-add-title"
              className="m-0 text-body font-semibold text-fg-1"
            >
              {t("skills.addRepo")}
            </h3>
            <div className="flex flex-col gap-1.5">
              <label htmlFor="sk-repo-url" className={LABEL_CLASS}>
                {t("skills.repo.url")}
              </label>
              <input
                id="sk-repo-url"
                className={MONO_FIELD_CLASS}
                placeholder={t("skills.repo.urlPlaceholder")}
                value={repoUrl}
                aria-invalid={Boolean(error)}
                aria-describedby={error ? "sk-repo-error" : undefined}
                autoComplete="off"
                spellCheck={false}
                onChange={(event) => {
                  const value = event.target.value;
                  setRepoUrl(value);
                  // 重新录入一个已存在的仓库时，把它的现有配置回填进表单。
                  // 否则表单默认值（公开、无凭据、main）会把它默默降级成公开仓库。
                  const parsed = parseRepoUrl(value);
                  const existing = parsed
                    ? repos.find(
                        (repo) =>
                          repo.owner === parsed.owner &&
                          repo.name === parsed.name,
                      )
                    : undefined;
                  const key = existing
                    ? `${existing.owner}/${existing.name}`
                    : "";
                  if (key !== backfilledFrom) {
                    // 离开了一个回填过的仓库，就回到表单默认值——否则它的分支、
                    // 私有勾选和令牌会悄悄跟着新 URL 一起提交
                    setBranch(existing?.branch || "");
                    setIsPrivate(existing?.private ?? false);
                    setCredentialId(existing?.credentialId ?? "");
                    setBackfilledFrom(key);
                  }
                }}
                onKeyDown={(event) => {
                  if (event.key === "Enter") void handleAdd();
                }}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <label htmlFor="sk-repo-branch" className={LABEL_CLASS}>
                {t("skills.repo.branch")}
              </label>
              <input
                id="sk-repo-branch"
                className={MONO_FIELD_CLASS}
                placeholder={t("skills.repo.branchPlaceholder")}
                value={branch}
                autoComplete="off"
                spellCheck={false}
                onChange={(event) => setBranch(event.target.value)}
              />
            </div>
            <div className="flex items-start gap-2.5">
              <input
                id="sk-repo-private"
                type="checkbox"
                className={cn(CHECKBOX_CLASS, "mt-0.5")}
                checked={isPrivate}
                onChange={(event) => setIsPrivate(event.target.checked)}
              />
              <label htmlFor="sk-repo-private" className="text-body text-fg-1">
                {t("skills.repo.private")}
              </label>
            </div>
            {error && (
              <FieldError id="sk-repo-error" role="alert">
                {error}
              </FieldError>
            )}
            <Button
              type="button"
              variant="neutral"
              size="regular"
              className="self-start"
              disabled={adding}
              onClick={() => void handleAdd()}
            >
              <Plus className="h-4 w-4" strokeWidth={2} />
              {t("skills.repo.add")}
            </Button>
          </section>

          {isPrivate && (
            <CredentialPicker
              value={credentialId}
              onChange={(id) => {
                // 换令牌就是在修上一次预检失败的原因，仓库级错误跟着清掉
                setError("");
                setCredentialId(id);
              }}
              invalid={credentialFailed}
              describedBy="sk-repo-error"
            />
          )}

          <section
            aria-labelledby="sk-repo-list-title"
            className="flex flex-col gap-2"
          >
            <h3
              id="sk-repo-list-title"
              className="m-0 text-body font-semibold text-fg-1"
            >
              {t("skills.repo.list")}
            </h3>
            {repos.length === 0 ? (
              <p className="m-0 rounded-panel border border-border px-4 py-6 text-center text-body text-fg-2">
                {t("skills.repo.empty")}
              </p>
            ) : (
              <ul className="m-0 list-none rounded-panel border border-border p-0">
                {repos.map((repo, index) => {
                  const id = `${repo.owner}/${repo.name}`;
                  const checkboxId = `sk-repo-manage-${index}`;
                  return (
                    <li
                      key={id}
                      className={cn(
                        "flex min-h-12 items-center gap-2.5 py-1.5 pe-2 ps-3.5",
                        index > 0 && "border-t border-border",
                      )}
                    >
                      {onToggle && (
                        <input
                          id={checkboxId}
                          type="checkbox"
                          className={CHECKBOX_CLASS}
                          checked={repo.enabled}
                          aria-label={t("skillsPage.repos.enableAria", {
                            repo: id,
                          })}
                          onChange={(event) =>
                            void onToggle(repo, event.target.checked)
                          }
                        />
                      )}
                      <div className="flex min-w-0 flex-1 flex-col">
                        <span className="truncate font-mono text-caption text-fg-1">
                          {id}
                        </span>
                        <span className="text-caption text-fg-2">
                          {repo.branch || "main"}
                          {repo.private &&
                            ` · ${t("skills.repo.privateBadge")}`}
                          {" · "}
                          {repo.enabled
                            ? t("skills.repo.skillCount", {
                                count: countRepoSkills(skills, repo),
                              })
                            : t("skillsPage.repos.disabled")}
                        </span>
                      </div>
                      <HoverTip content={t("common.view")}>
                        <Button
                          type="button"
                          variant="quiet"
                          size="icon-compact"
                          aria-label={t("skillsPage.repos.openAria", {
                            repo: id,
                          })}
                          onClick={() =>
                            void handleOpenRepo(repo.owner, repo.name)
                          }
                        >
                          <ExternalLink className="h-4 w-4" strokeWidth={1.5} />
                        </Button>
                      </HoverTip>
                      <HoverTip content={t("common.delete")}>
                        <Button
                          type="button"
                          variant="quiet"
                          size="icon-compact"
                          aria-label={t("skillsPage.repos.removeAria", {
                            repo: id,
                          })}
                          className="hover:text-danger-text"
                          onClick={() => void onRemove(repo.owner, repo.name)}
                        >
                          <Trash2 className="h-4 w-4" strokeWidth={1.5} />
                        </Button>
                      </HoverTip>
                    </li>
                  );
                })}
              </ul>
            )}
          </section>
        </SheetBody>
      </SheetContent>
    </Sheet>
  );
}
