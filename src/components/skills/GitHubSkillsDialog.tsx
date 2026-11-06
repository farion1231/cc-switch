import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ExternalLink, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { DialogDescription, DialogTitle } from "@/components/ui/dialog";
import {
  CHECKBOX_CLASS,
  LABEL_CLASS,
  MONO_FIELD_CLASS,
  V7Dialog,
} from "@/components/mcp/formBits";
import { MatrixSearch, NeutralBadge } from "@/components/mcp/AppMatrix";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import {
  useAddSkillRepo,
  useInstalledSkills,
  useInstallSkill,
} from "@/hooks/useSkills";
import { settingsApi } from "@/lib/api";
import type { DiscoverableSkill } from "@/lib/api/skills";
import type { AppId } from "@/lib/api/types";
import { skillErrorReason } from "@/lib/errors/skillErrorParser";
import { parseRepoUrl } from "./RepoManagerPanel";
import type { useSkillInstallTargets } from "./useSkillInstallTargets";

interface GitHubSkillsDialogProps {
  appIds: AppId[];
  targets: AppId[];
  setTargets: (apps: AppId[]) => void;
  installTo: ReturnType<typeof useSkillInstallTargets>["installTo"];
  onClose: () => void;
}

export function GitHubSkillsDialog({
  appIds,
  targets,
  setTargets,
  installTo,
  onClose,
}: GitHubSkillsDialogProps) {
  const { t } = useTranslation();
  const addRepo = useAddSkillRepo();
  const install = useInstallSkill();
  const { data: installed = [] } = useInstalledSkills();
  const [url, setUrl] = useState("");
  const [branch, setBranch] = useState("main");
  const [skills, setSkills] = useState<DiscoverableSkill[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [query, setQuery] = useState("");
  const [error, setError] = useState("");
  const [failures, setFailures] = useState<Record<string, string>>({});
  const [completed, setCompleted] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const lock = useRef(false);
  const installedByDirectory = new Map(
    installed.map((skill) => [skill.directory.toLowerCase(), skill]),
  );
  const existingFor = (skill: DiscoverableSkill) =>
    installedByDirectory.get(skill.directory.split("/").pop()!.toLowerCase());
  const isInstalled = (skill: DiscoverableSkill) => {
    const existing = existingFor(skill);
    return (
      completed.has(skill.key) ||
      (existing?.repoOwner === skill.repoOwner &&
        existing?.repoName === skill.repoName)
    );
  };
  const unavailable = (skill: DiscoverableSkill) =>
    completed.has(skill.key) || !!existingFor(skill);
  const visible = (skills ?? []).filter((skill) =>
    [skill.name, skill.description, skill.directory].some((value) =>
      value.toLowerCase().includes(query.trim().toLowerCase()),
    ),
  );
  const explain = (e: unknown) =>
    skillErrorReason(e instanceof Error ? e.message : String(e), t);

  const reset = () => {
    setSkills(null);
    setSelected(new Set());
    setFailures({});
    setCompleted(new Set());
    setError("");
    setQuery("");
  };

  const fetchSkills = async () => {
    if (lock.current) return;
    const repo = parseRepoUrl(url);
    if (!repo) {
      setError(t("skills.repo.invalidUrl"));
      return;
    }
    lock.current = true;
    setBusy(true);
    reset();
    try {
      const result = await addRepo.mutateAsync({
        ...repo,
        branch: branch.trim() || "main",
        enabled: true,
      });
      if (!result) throw new Error(t("skills.repo.addFailed"));
      if (result.failures.length) throw new Error(result.failures[0].error);
      setSkills(result.skills);
    } catch (e) {
      setError(explain(e));
    } finally {
      lock.current = false;
      setBusy(false);
    }
  };

  const installSelected = async () => {
    if (lock.current || !targets.length) return;
    lock.current = true;
    setBusy(true);
    setError("");
    const apps = [...targets];
    try {
      for (const skill of skills ?? []) {
        if (!selected.has(skill.key) || unavailable(skill)) continue;
        setFailures((previous) => {
          const next = { ...previous };
          delete next[skill.key];
          return next;
        });
        try {
          const result = await installTo(
            async (currentApp) => [
              await install.mutateAsync({ skill, currentApp }),
            ],
            apps,
          );
          setCompleted((previous) => new Set(previous).add(skill.key));
          setSelected((previous) => {
            const next = new Set(previous);
            next.delete(skill.key);
            return next;
          });
          if (result.failures.length) {
            setFailures((previous) => ({
              ...previous,
              [skill.key]: result.failures
                .map(
                  (failure) =>
                    `${APP_DISPLAY_NAME[failure.app]}: ${explain(failure.error)}`,
                )
                .join("\n"),
            }));
          }
        } catch (e) {
          setFailures((previous) => ({ ...previous, [skill.key]: explain(e) }));
        }
      }
    } finally {
      lock.current = false;
      setBusy(false);
    }
  };

  const openDocs = async (skill: DiscoverableSkill) => {
    const path = skill.directory.split("/").map(encodeURIComponent).join("/");
    try {
      await settingsApi.openExternal(
        `https://github.com/${encodeURIComponent(skill.repoOwner)}/${encodeURIComponent(skill.repoName)}/tree/${encodeURIComponent(skill.repoBranch)}/${path}`,
      );
    } catch (e) {
      setError(explain(e));
    }
  };

  return (
    <V7Dialog
      open
      width={760}
      className="flex flex-col overflow-hidden"
      onOpenChange={(open) => {
        if (!open && !lock.current) onClose();
      }}
    >
      <div className="flex shrink-0 flex-col gap-1">
        <DialogTitle>{t("skills.github.title")}</DialogTitle>
        <DialogDescription className="text-caption text-fg-2">
          {t("skills.github.lead")}
        </DialogDescription>
      </div>
      <fieldset
        disabled={busy}
        className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto disabled:opacity-70"
      >
        <div className="grid shrink-0 grid-cols-1 gap-3 sm:grid-cols-[1fr_140px]">
          <label className={LABEL_CLASS}>
            {t("skills.repo.url")}
            <input
              className={`${MONO_FIELD_CLASS} mt-1.5 w-full`}
              value={url}
              placeholder={t("skills.repo.urlPlaceholder")}
              autoCapitalize="off"
              autoCorrect="off"
              spellCheck={false}
              onChange={(event) => {
                setUrl(event.target.value);
                reset();
              }}
            />
          </label>
          <label className={LABEL_CLASS}>
            {t("skills.repo.branch")}
            <input
              className={`${MONO_FIELD_CLASS} mt-1.5 w-full`}
              value={branch}
              autoCapitalize="off"
              autoCorrect="off"
              spellCheck={false}
              onChange={(event) => {
                setBranch(event.target.value);
                reset();
              }}
            />
          </label>
        </div>
        <div className="flex shrink-0 items-center gap-3">
          <Button
            variant="neutral"
            onClick={() => void fetchSkills()}
            className="self-start"
          >
            {busy && skills === null && (
              <Loader2 className="h-4 w-4 animate-spin" />
            )}
            {t("skills.github.fetch")}
          </Button>
          {skills !== null && (
            <p role="status" className="text-caption text-fg-2">
              {t("skills.github.found", { count: skills.length })}
            </p>
          )}
        </div>
        {skills !== null && (
          <>
            <div className="flex flex-wrap items-center gap-2">
              <MatrixSearch
                className="min-w-0 flex-1"
                value={query}
                onValueChange={setQuery}
                placeholder={t("skills.github.search")}
                ariaLabel={t("skills.github.search")}
              />
              <Button
                variant="neutral"
                size="compact"
                onClick={() =>
                  setSelected(
                    new Set(
                      skills
                        .filter((skill) => !unavailable(skill))
                        .map((skill) => skill.key),
                    ),
                  )
                }
              >
                {t("skills.github.selectAll")}
              </Button>
              <Button
                variant="neutral"
                size="compact"
                onClick={() => setSelected(new Set())}
              >
                {t("skills.github.selectNone")}
              </Button>
            </div>
            <p className="text-caption text-fg-2">
              {t("skills.github.count", {
                total: skills.length,
                selected: skills.filter(
                  (skill) => selected.has(skill.key) && !unavailable(skill),
                ).length,
              })}
            </p>
            <ul className="min-h-[96px] max-h-[320px] flex-1 overflow-y-auto rounded-panel border border-border divide-y divide-border">
              {visible.map((skill) => (
                <li key={skill.key} className="flex items-start gap-3 p-3">
                  <label className="flex min-w-0 flex-1 items-start gap-3">
                    <input
                      type="checkbox"
                      className={CHECKBOX_CLASS}
                      disabled={unavailable(skill)}
                      checked={selected.has(skill.key) && !unavailable(skill)}
                      onChange={(event) =>
                        setSelected((previous) => {
                          const next = new Set(previous);
                          if (event.target.checked) next.add(skill.key);
                          else next.delete(skill.key);
                          return next;
                        })
                      }
                    />
                    <span className="min-w-0">
                      <span className="flex flex-wrap items-center gap-2 text-body font-medium">
                        {skill.name}
                        {isInstalled(skill) && (
                          <NeutralBadge>
                            {t("skillsPage.discover.installed")}
                          </NeutralBadge>
                        )}
                      </span>
                      <span
                        className="line-clamp-2 break-words text-caption text-fg-2"
                        title={skill.description}
                      >
                        {skill.description}
                      </span>
                      {unavailable(skill) && !isInstalled(skill) && (
                        <span className="block text-caption text-fg-2">
                          {t("skills.github.occupied")}
                        </span>
                      )}
                      {failures[skill.key] && (
                        <span
                          className="block whitespace-pre-wrap text-caption text-destructive"
                          role="alert"
                        >
                          {failures[skill.key]}
                        </span>
                      )}
                    </span>
                  </label>
                  <Button
                    variant="quiet"
                    size="icon-compact"
                    aria-label={t("skills.github.open", { name: skill.name })}
                    onClick={() => void openDocs(skill)}
                  >
                    <ExternalLink className="h-4 w-4" />
                  </Button>
                </li>
              ))}
              {visible.length === 0 && (
                <li className="p-6 text-center text-fg-2">
                  {t("skills.github.empty")}
                </li>
              )}
            </ul>
            <div
              className="grid shrink-0 grid-cols-2 gap-x-4 gap-y-2 border-t border-border pt-3 sm:grid-cols-4"
              aria-label={t("skillsPage.installTo.title")}
            >
              <span className="col-span-full text-caption font-medium text-fg-2">
                {t("skillsPage.installTo.title")}
              </span>
              {appIds.map((app) => (
                <label key={app} className="flex items-center gap-2 text-body">
                  <input
                    className={CHECKBOX_CLASS}
                    type="checkbox"
                    checked={targets.includes(app)}
                    disabled={targets.length === 1 && targets.includes(app)}
                    onChange={(event) =>
                      setTargets(
                        event.target.checked
                          ? [...targets, app]
                          : targets.filter((target) => target !== app),
                      )
                    }
                  />
                  <AppGlyph app={app} size={16} badgeClassName="bg-surface" />
                  <span className="min-w-0 truncate">
                    {APP_DISPLAY_NAME[app]}
                  </span>
                </label>
              ))}
            </div>
          </>
        )}
      </fieldset>
      {error && (
        <p role="alert" className="text-caption text-destructive">
          {error}
        </p>
      )}
      <div className="flex shrink-0 justify-end gap-2">
        <Button variant="neutral" disabled={busy} onClick={onClose}>
          {t("common.close")}
        </Button>
        <Button
          variant="solid"
          disabled={
            busy ||
            !targets.length ||
            !(skills ?? []).some(
              (skill) => selected.has(skill.key) && !unavailable(skill),
            )
          }
          onClick={() => void installSelected()}
        >
          {busy && skills !== null && (
            <Loader2 className="h-4 w-4 animate-spin" />
          )}
          {t("skills.github.install")}
        </Button>
      </div>
    </V7Dialog>
  );
}
