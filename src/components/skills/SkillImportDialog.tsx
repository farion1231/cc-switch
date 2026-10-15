import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import type { AppId } from "@/lib/api/types";
import type { ImportSkillSelection, UnmanagedSkill } from "@/lib/api/skills";
import { SKILLS_APP_IDS } from "@/config/appConfig";
import { cn } from "@/lib/utils";
import { MatrixSearch } from "@/components/mcp/AppMatrix";
import { CHECKBOX_CLASS, V7Dialog } from "@/components/mcp/formBits";

/** Pi 的启用状态由 ~/.pi/agent/skills 里有没有这个目录决定，导入时不勾选 */
const IMPORT_APP_IDS = SKILLS_APP_IDS.filter((app) => app !== "pi");

interface SkillImportDialogProps {
  skills: UnmanagedSkill[];
  visibleAppIds: AppId[];
  isImporting: boolean;
  onImport: (imports: ImportSkillSelection[]) => void;
  onClose: () => void;
}

const isAppId = (value: string): value is AppId =>
  (SKILLS_APP_IDS as string[]).includes(value);

/** 「导入本机已有…」（宽 560）：选目录 + 选导入后在哪些应用启用。 */
export function SkillImportDialog({
  skills,
  visibleAppIds,
  isImporting,
  onImport,
  onClose,
}: SkillImportDialogProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Set<string>>(
    () => new Set(skills.map((skill) => skill.directory)),
  );
  const appChoices = IMPORT_APP_IDS.filter((app) =>
    visibleAppIds.includes(app),
  );
  // 默认勾「发现于」里出现过的应用
  const defaultApps = useMemo(() => {
    const set = new Set<AppId>();
    for (const skill of skills) {
      for (const found of skill.foundIn) {
        if (isAppId(found) && found !== "pi") set.add(found);
      }
    }
    return set;
  }, [skills]);
  const [apps, setApps] = useState<Set<AppId>>(() => new Set(defaultApps));

  const foundLabel = (found: string) => {
    if (isAppId(found)) return APP_DISPLAY_NAME[found];
    if (found === "agents") return "~/.agents/skills";
    if (found === "cc-switch") return t("skillsPage.import.foundCcSwitch");
    return found;
  };

  const normalized = query.trim().toLowerCase();
  const visible = skills.filter(
    (skill) =>
      !normalized ||
      [skill.name, skill.directory, ...skill.foundIn.map(foundLabel)].some(
        (value) => value.toLowerCase().includes(normalized),
      ),
  );

  const toggle = (directory: string, checked: boolean) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (checked) next.add(directory);
      else next.delete(directory);
      return next;
    });

  const submit = () => {
    // 默认勾的应用只给在那里发现过的 Skill；另外勾的应用给全部
    const extra = [...apps].filter((app) => !defaultApps.has(app));
    onImport(
      skills
        .filter((skill) => selected.has(skill.directory))
        .map((skill) => {
          const chosen = new Set<AppId>(extra);
          for (const found of skill.foundIn) {
            if (isAppId(found) && apps.has(found)) chosen.add(found);
          }
          return {
            directory: skill.directory,
            apps: {
              claude: chosen.has("claude"),
              codex: chosen.has("codex"),
              gemini: chosen.has("gemini"),
              grokbuild: chosen.has("grokbuild"),
              opencode: chosen.has("opencode"),
              openclaw: false,
              hermes: chosen.has("hermes"),
              pi: false,
              mcode: chosen.has("mcode"),
            },
          };
        }),
    );
  };

  return (
    <V7Dialog
      open
      width={560}
      onOpenChange={(open) => {
        if (!open && !isImporting) onClose();
      }}
    >
      <div className="flex shrink-0 flex-col gap-1">
        <div className="flex items-center gap-0.5">
          <DialogTitle className="text-section">
            {t("skillsPage.import.title")}
          </DialogTitle>
          <HelpTip title={t("skillsPage.import.helpTitle")}>
            {t("skillsPage.import.help")}
          </HelpTip>
        </div>
        <DialogDescription className="text-caption text-fg-2">
          {t("skillsPage.import.lead")}
        </DialogDescription>
      </div>

      <div className="flex shrink-0 items-center gap-2">
        <MatrixSearch
          className="flex-1"
          value={query}
          onValueChange={setQuery}
          placeholder={t("skillsPage.import.searchPlaceholder")}
          ariaLabel={t("skillsPage.import.searchAria")}
        />
        <Button
          type="button"
          variant="quiet"
          size="compact"
          onClick={() =>
            setSelected(new Set(skills.map((skill) => skill.directory)))
          }
        >
          {t("skillsPage.import.selectAll")}
        </Button>
        <Button
          type="button"
          variant="quiet"
          size="compact"
          onClick={() => setSelected(new Set())}
        >
          {t("skillsPage.import.selectNone")}
        </Button>
      </div>

      <ul className="m-0 max-h-[212px] min-h-0 list-none overflow-y-auto rounded-panel border border-border p-0">
        {visible.map((skill, index) => {
          const id = `sk-imp-${index}`;
          const showDir =
            skill.directory.toLowerCase() !== skill.name.toLowerCase();
          return (
            <li
              key={skill.directory}
              className={cn(
                "flex items-center gap-2.5 py-2 pe-3 ps-3.5",
                index > 0 && "border-t border-border",
              )}
            >
              <input
                id={id}
                type="checkbox"
                className={CHECKBOX_CLASS}
                checked={selected.has(skill.directory)}
                onChange={(event) =>
                  toggle(skill.directory, event.target.checked)
                }
              />
              <label htmlFor={id} className="flex min-w-0 flex-1 flex-col">
                <span className="flex min-w-0 items-center gap-1.5">
                  <span className="truncate text-body font-medium">
                    {skill.name}
                  </span>
                  {showDir && (
                    <span className="truncate font-mono text-caption text-fg-3">
                      {skill.directory}
                    </span>
                  )}
                </span>
                <span
                  className="truncate text-caption text-fg-2"
                  title={skill.path}
                >
                  {t("skillsPage.import.foundIn", {
                    places: skill.foundIn
                      .map(foundLabel)
                      .join(t("mcpPage.listSeparator")),
                  })}
                </span>
              </label>
            </li>
          );
        })}
      </ul>

      <section className="flex shrink-0 flex-col gap-2">
        <div className="flex items-center gap-0.5">
          <h3 className="m-0 text-body font-semibold">
            {t("skillsPage.import.appsTitle")}
          </h3>
          <HelpTip title={t("skillsPage.import.appsHelpTitle")}>
            {t("skillsPage.import.appsHelp")}
          </HelpTip>
        </div>
        <div className="grid grid-cols-2 gap-x-4 gap-y-2">
          {appChoices.map((app) => (
            <label
              key={app}
              className="flex h-6 cursor-pointer items-center gap-2 text-body"
            >
              <input
                type="checkbox"
                className={CHECKBOX_CLASS}
                checked={apps.has(app)}
                onChange={(event) =>
                  setApps((prev) => {
                    const next = new Set(prev);
                    if (event.target.checked) next.add(app);
                    else next.delete(app);
                    return next;
                  })
                }
              />
              <AppGlyph app={app} size={16} badgeClassName="bg-surface" />
              <span className="truncate">{APP_DISPLAY_NAME[app]}</span>
            </label>
          ))}
        </div>
      </section>

      <div className="flex shrink-0 justify-end gap-2 pt-1">
        <Button
          type="button"
          variant="neutral"
          size="regular"
          disabled={isImporting}
          onClick={onClose}
        >
          {t("common.cancel")}
        </Button>
        <Button
          type="button"
          variant="solid"
          size="regular"
          disabled={selected.size === 0 || isImporting}
          onClick={submit}
        >
          {t("skillsPage.import.submit", { count: selected.size })}
        </Button>
      </div>
    </V7Dialog>
  );
}
