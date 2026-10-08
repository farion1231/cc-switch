import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ExternalLink, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetBody,
  SheetContent,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { HoverTip } from "@/components/ui/hover-tip";
import { settingsApi } from "@/lib/api";
import type { DiscoverableSkill, SkillRepo } from "@/lib/api/skills";
import { cn } from "@/lib/utils";
import {
  CHECKBOX_CLASS,
  FieldError,
  LABEL_CLASS,
  MONO_FIELD_CLASS,
} from "@/components/mcp/formBits";

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

/** 仓库管理（右侧抽屉，宽 420）：添加、停用、删除 Skill 仓库。 */
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
  const [error, setError] = useState("");
  const [adding, setAdding] = useState(false);

  const handleAdd = async () => {
    setError("");
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
      });
      setRepoUrl("");
      setBranch("");
    } catch (e) {
      setError(e instanceof Error ? e.message : t("skills.repo.addFailed"));
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
                onChange={(event) => setRepoUrl(event.target.value)}
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
            {error && <FieldError id="sk-repo-error">{error}</FieldError>}
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
