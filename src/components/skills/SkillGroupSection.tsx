import type { ReactNode } from "react";
import { ChevronDown, ChevronRight, Pencil } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { HoverTip } from "@/components/ui/hover-tip";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import type { AppId } from "@/lib/api/types";
import type { InstalledSkillGroup } from "./skillGroups";

export function SkillGroupSection({
  group,
  appIds,
  expanded,
  disabled,
  onCollapse,
  onToggle,
  onEdit,
  children,
}: {
  group: InstalledSkillGroup;
  appIds: AppId[];
  expanded: boolean;
  disabled: boolean;
  onCollapse: () => void;
  onToggle: (app: AppId, enabled: boolean) => void;
  onEdit: () => void;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  return (
    <li role="presentation">
      <section aria-label={group.label}>
        <div className="flex min-h-11 items-center border-t border-border bg-subtle px-2">
          <button
            type="button"
            aria-expanded={expanded}
            aria-label={t("skillsPage.groups.expand", { name: group.label })}
            className="flex min-w-0 flex-1 items-center gap-2 px-1 text-left text-body font-medium"
            onClick={onCollapse}
          >
            {expanded ? (
              <ChevronDown className="h-4 w-4 shrink-0" />
            ) : (
              <ChevronRight className="h-4 w-4 shrink-0" />
            )}
            <span className="truncate">{group.label}</span>
            <span className="text-caption text-fg-2">
              {group.skills.length}
            </span>
          </button>
          {group.bulk ? (
            appIds.map((app) => {
              // Full membership is authoritative; children may be filtered or collapsed.
              const count = group.skills.filter(
                (skill) => skill.apps[app],
              ).length;
              const checked =
                count === 0
                  ? false
                  : count === group.skills.length
                    ? true
                    : "indeterminate";
              const label = t("skillsPage.groups.toggle", {
                name: group.label,
                app: APP_DISPLAY_NAME[app],
              });
              return (
                <span key={app} className="flex w-9 shrink-0 justify-center">
                  <HoverTip content={label}>
                    <span>
                      <Checkbox
                        checked={checked}
                        disabled={disabled || group.skills.length === 0}
                        aria-label={label}
                        onCheckedChange={() => onToggle(app, checked !== true)}
                      />
                    </span>
                  </HoverTip>
                </span>
              );
            })
          ) : (
            <div style={{ width: appIds.length * 36 }} className="shrink-0" />
          )}
          <span className="flex w-16 shrink-0 justify-center">
            {group.project && (
              <Button
                type="button"
                variant="quiet"
                size="icon-compact"
                disabled={disabled}
                aria-label={t("skillsPage.groups.editNamed", {
                  name: group.label,
                })}
                onClick={onEdit}
              >
                <Pencil className="h-3.5 w-3.5" />
              </Button>
            )}
          </span>
        </div>
        {expanded && <ul className="m-0 list-none p-0">{children}</ul>}
      </section>
    </li>
  );
}
