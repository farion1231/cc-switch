import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ExternalLink, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
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
  useDiscoverAndAddSkillRepo,
  useDiscoverSkillRepo,
  useInstalledSkills,
  useInstallRepoSkills,
  useSkillRepos,
} from "@/hooks/useSkills";
import { settingsApi } from "@/lib/api";
import type { DiscoverableSkill } from "@/lib/api/skills";
import type { AppId } from "@/lib/api/types";
import { skillErrorReason } from "@/lib/errors/skillErrorParser";
import {
  GITHUB_HOST,
  isInsecureRepoUrl,
  parseRepoUrl,
  repoWebUrl,
} from "./repoUrl";
import type { useSkillInstallTargets } from "./useSkillInstallTargets";

// Imports may use a basename ID. Compare source documents independently of the
// branch revision, retaining the full path to distinguish same-name Skills.
function sourceDocumentPath(skill: {
  repoHost?: string;
  repoOwner?: string;
  repoName?: string;
  repoBranch?: string;
  readmeUrl?: string;
}): string | null {
  if (!skill.repoOwner || !skill.repoName || !skill.readmeUrl) return null;
  try {
    const url = new URL(skill.readmeUrl);
    const host = (skill.repoHost || GITHUB_HOST).toLowerCase();
    if (url.protocol !== "https:" || url.host.toLowerCase() !== host)
      return null;
    const prefix = `/${skill.repoOwner}/${skill.repoName}/`;
    const pathname = decodeURIComponent(url.pathname);
    if (!pathname.toLowerCase().startsWith(prefix.toLowerCase())) return null;
    const relative = pathname.slice(prefix.length);
    const markers =
      host === GITHUB_HOST
        ? ["blob/", "tree/"]
        : ["-/blob/", "-/tree/", "src/branch/", "src/tag/", "src/commit/"];
    const marker = markers.find((value) => relative.startsWith(value));
    if (!marker) return null;
    const source = relative.slice(marker.length);
    const branch = `${skill.repoBranch || "HEAD"}/`;
    if (!source.startsWith(branch)) return null;
    const path = source.slice(branch.length).split("/");
    return path.length > 0 && path.at(-1) === "SKILL.md"
      ? path.join("/")
      : null;
  } catch {
    return null;
  }
}

interface GitSkillsDialogProps {
  appIds: AppId[];
  targets: AppId[];
  setTargets: (apps: AppId[]) => void;
  installTo: ReturnType<typeof useSkillInstallTargets>["installTo"];
  onClose: () => void;
}

export function GitSkillsDialog({
  appIds,
  targets,
  setTargets,
  installTo,
  onClose,
}: GitSkillsDialogProps) {
  const { t } = useTranslation();
  const addRepo = useDiscoverAndAddSkillRepo();
  const discoverRepo = useDiscoverSkillRepo();
  const install = useInstallRepoSkills();
  const { data: installed = [] } = useInstalledSkills();
  const { data: repos = [], refetch: refreshRepos } = useSkillRepos();
  const [url, setUrl] = useState("");
  const [branch, setBranch] = useState("main");
  const [skills, setSkills] = useState<DiscoverableSkill[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [query, setQuery] = useState("");
  const [error, setError] = useState("");
  const [branchNotice, setBranchNotice] = useState("");
  const [failures, setFailures] = useState<Record<string, string>>({});
  const [completed, setCompleted] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const lock = useRef(false);
  const installedByDirectory = new Map(
    installed.map((skill) => [skill.directory.toLowerCase(), skill]),
  );
  const destinationFor = (skill: DiscoverableSkill) =>
    skill.directory.split(/[/\\]/).pop()!.trim().toLowerCase();
  const existingFor = (skill: DiscoverableSkill) =>
    installedByDirectory.get(destinationFor(skill));
  const isInstalled = (skill: DiscoverableSkill) => {
    const existing = existingFor(skill);
    if (completed.has(skill.key)) return true;
    if (
      (existing?.repoHost || GITHUB_HOST).toLowerCase() !==
      (skill.repoHost || GITHUB_HOST).toLowerCase()
    )
      return false;
    if (existing?.id === skill.key) return true;
    if (
      existing?.repoOwner?.toLowerCase() !== skill.repoOwner.toLowerCase() ||
      existing?.repoName?.toLowerCase() !== skill.repoName.toLowerCase()
    )
      return false;
    const source = sourceDocumentPath(skill);
    return (
      source !== null &&
      (existing.readmeUrl === skill.readmeUrl ||
        source === sourceDocumentPath(existing))
    );
  };
  const unavailable = (skill: DiscoverableSkill) =>
    completed.has(skill.key) || !!existingFor(skill);
  const selectedRepo = repos.find(
    (repo) => url === repoWebUrl(repo) && branch === (repo.branch || "main"),
  );
  const visible = (skills ?? []).filter((skill) =>
    [skill.name, skill.description, skill.directory].some((value) =>
      value.toLowerCase().includes(query.trim().toLowerCase()),
    ),
  );
  const explain = (e: unknown) =>
    skillErrorReason(e instanceof Error ? e.message : String(e), t);

  const reset = () => {
    setSkills(null);
    setBranchNotice("");
    setSelected(new Set());
    setFailures({});
    setCompleted(new Set());
    setError("");
    setQuery("");
  };

  const fetchSkills = async () => {
    if (lock.current) return;
    if (isInsecureRepoUrl(url)) {
      setError(t("skills.repo.httpsOnly"));
      return;
    }
    const repo = parseRepoUrl(url);
    if (!repo) {
      setError(t("skills.repo.invalidUrl"));
      return;
    }
    lock.current = true;
    setBusy(true);
    reset();
    try {
      const requestedBranch = branch.trim() || "main";
      // Read the current registry before deciding whether to add a new source.
      // Re-adding an existing source would overwrite its branch and enabled state.
      const { data: registry } = await refreshRepos({ throwOnError: true });
      const existingRepo = registry?.find(
        (item) =>
          item.owner.toLowerCase() === repo.owner.toLowerCase() &&
          item.name.toLowerCase() === repo.name.toLowerCase(),
      );
      if (
        existingRepo &&
        (existingRepo.host || GITHUB_HOST).toLowerCase() !==
          (repo.host || GITHUB_HOST).toLowerCase()
      ) {
        throw new Error(t("skills.github.hostConflict"));
      }
      let result;
      let resolvedBranch;
      if (existingRepo) {
        ({ discovery: result } = await discoverRepo.mutateAsync({
          repo: existingRepo,
          branch: requestedBranch,
        }));
        if (result.failures.length) throw new Error(result.failures[0].error);
        resolvedBranch = result.skills[0]?.repoBranch;
      } else {
        [resolvedBranch, result] = await addRepo.mutateAsync({
          ...repo,
          branch: requestedBranch,
          enabled: true,
        });
      }
      if (resolvedBranch && resolvedBranch !== requestedBranch) {
        if (!existingRepo) setBranch(resolvedBranch);
        setBranchNotice(
          t("skills.github.branchFallback", {
            requested: requestedBranch,
            resolved: resolvedBranch,
          }),
        );
      }
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
    const destinations = new Set(installedByDirectory.keys());
    try {
      const pending: DiscoverableSkill[] = [];
      const errors: Record<string, string> = {};
      for (const skill of skills ?? []) {
        if (!selected.has(skill.key) || unavailable(skill)) continue;
        const destination = destinationFor(skill);
        if (destinations.has(destination)) {
          errors[skill.key] = t("skills.github.occupied");
          continue;
        }
        destinations.add(destination);
        pending.push(skill);
      }
      const installedKeys = new Map<string, string>();
      const completedKeys = new Set<string>();
      const result = await installTo(async (currentApp) => {
        const outcomes = await install.mutateAsync({
          skills: pending,
          currentApp,
        });
        return outcomes.flatMap((outcome, index) => {
          const key = pending[index].key;
          if ("Err" in outcome) {
            errors[key] = explain(outcome.Err);
            return [];
          }
          completedKeys.add(key);
          installedKeys.set(outcome.Ok.id, key);
          return [outcome.Ok];
        });
      }, apps);
      for (const failure of result.failures) {
        const message =
          APP_DISPLAY_NAME[failure.app] + ": " + explain(failure.error);
        const key = installedKeys.get(failure.id)!;
        errors[key] = [errors[key], message].filter(Boolean).join("\n");
      }
      setFailures((previous) => {
        const next = { ...previous };
        for (const skill of pending) delete next[skill.key];
        return { ...next, ...errors };
      });
      setCompleted((previous) => new Set([...previous, ...completedKeys]));
      setSelected(
        (previous) =>
          new Set([...previous].filter((key) => !completedKeys.has(key))),
      );
    } catch (error) {
      setError(explain(error));
    } finally {
      lock.current = false;
      setBusy(false);
    }
  };

  const openDocs = async (skill: DiscoverableSkill) => {
    try {
      await settingsApi.openExternal(
        skill.readmeUrl ||
          repoWebUrl({
            host: skill.repoHost,
            owner: skill.repoOwner,
            name: skill.repoName,
          }),
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
        {repos.length > 0 && (
          <div className="flex flex-col gap-1.5">
            <label htmlFor="git-saved-repository" className={LABEL_CLASS}>
              {t("skills.github.chooseRepo")}
            </label>
            <Select
              disabled={busy}
              value={selectedRepo ? url : ""}
              onValueChange={(value) => {
                const repo = repos.find((item) => value === repoWebUrl(item));
                if (!repo) return;
                setUrl(value);
                setBranch(repo.branch || "main");
                reset();
              }}
            >
              <SelectTrigger
                id="git-saved-repository"
                className="w-full gap-3 [&>span:first-child]:min-w-0 [&>span:first-child]:flex-1"
              >
                <SelectValue
                  placeholder={t("skills.github.chooseRepoPlaceholder")}
                />
              </SelectTrigger>
              <SelectContent>
                {repos.map((repo) => {
                  const value = repoWebUrl(repo);
                  const host = repo.host || GITHUB_HOST;
                  const platform =
                    (
                      {
                        "github.com": "GitHub",
                        "gitlab.com": "GitLab",
                        "gitea.com": "Gitea",
                        "codeberg.org": "Forgejo",
                      } as Record<string, string>
                    )[host.toLowerCase()] || host;
                  return (
                    <SelectItem
                      key={value}
                      value={value}
                      textValue={`${repo.owner}/${repo.name} ${platform} ${host} ${repo.branch || "main"}`}
                      className="[&>span:last-child]:min-w-0 [&>span:last-child]:w-full"
                    >
                      <span className="grid w-full grid-cols-[5rem_minmax(0,1fr)_auto] items-center gap-3 text-start">
                        <span
                          className="truncate text-caption text-fg-3"
                          title={host}
                        >
                          {platform}
                        </span>
                        <span
                          className="truncate font-mono text-caption"
                          title={value}
                        >
                          {repo.owner}/{repo.name}
                        </span>
                        <span
                          className="max-w-[10rem] truncate font-mono text-caption text-fg-3"
                          title={repo.branch || "main"}
                        >
                          {repo.branch || "main"}
                          {!repo.enabled &&
                            ` · ${t("skillsPage.repos.disabledShort")}`}
                        </span>
                      </span>
                    </SelectItem>
                  );
                })}
              </SelectContent>
            </Select>
          </div>
        )}
        <div
          className="grid shrink-0 grid-cols-1 gap-3 sm:grid-cols-[1fr_140px]"
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.nativeEvent.isComposing) {
              event.preventDefault();
              void fetchSkills();
            }
          }}
        >
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
        <p className="text-caption text-fg-3">{t("skills.github.saveHint")}</p>
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
        {branchNotice && (
          <p role="status" className="text-caption text-warning-text">
            {branchNotice}
          </p>
        )}
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
                    (previous) =>
                      new Set([
                        ...previous,
                        ...visible
                          .filter((skill) => !unavailable(skill))
                          .map((skill) => skill.key),
                      ]),
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
                          className="block whitespace-pre-wrap text-caption text-danger-text"
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
        <p role="alert" className="text-caption text-danger-text">
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
