import React, { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Trash2, ExternalLink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { type InstalledSkill, type AppType } from "@/hooks/useSkills";
import { settingsApi } from "@/lib/api";
import { cn } from "@/lib/utils";

interface SkillCardCompactProps {
  skill: InstalledSkill;
  onToggleApp: (id: string, app: AppType, enabled: boolean) => void;
  onUninstall: () => void;
}

export const SkillCardCompact: React.FC<SkillCardCompactProps> = ({
  skill,
  onToggleApp,
  onUninstall,
}) => {
  const { t } = useTranslation();

  const openDocs = async () => {
    if (!skill.readmeUrl) return;
    try {
      await settingsApi.openExternal(skill.readmeUrl);
    } catch {
      // ignore
    }
  };

  // 生成来源标签
  const sourceLabel = useMemo(() => {
    if (skill.repoOwner && skill.repoName) {
      return `${skill.repoOwner}/${skill.repoName}`;
    }
    return t("skills.local");
  }, [skill.repoOwner, skill.repoName, t]);

  // Count enabled apps
  const enabledCount = [
    skill.apps.claude,
    skill.apps.codex,
    skill.apps.gemini,
    skill.apps.opencode,
  ].filter(Boolean).length;

  return (
    <div className="group relative flex flex-col h-full p-3 rounded-xl border border-border bg-card hover:bg-muted/50 hover:border-border-active hover:shadow-sm transition-all duration-300">
      {/* Header: Name + Actions */}
      <div className="flex items-start justify-between gap-2">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <h3 className="font-medium text-sm text-foreground truncate">
              {skill.name}
            </h3>
            {skill.readmeUrl && (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-5 w-5 p-0 opacity-0 group-hover:opacity-100 transition-opacity"
                onClick={openDocs}
              >
                <ExternalLink className="h-3 w-3" />
              </Button>
            )}
          </div>
          {skill.description && (
            <p className="text-xs text-muted-foreground line-clamp-2 mt-1">
              {skill.description}
            </p>
          )}
          <p className="text-xs text-muted-foreground/70 truncate mt-1">
            {sourceLabel}
          </p>
        </div>

        {/* Quick actions */}
        <div className="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-7 w-7 p-0 hover:text-red-500 hover:bg-red-100 dark:hover:text-red-400 dark:hover:bg-red-500/10"
            onClick={onUninstall}
            title={t("skills.uninstall")}
          >
            <Trash2 size={14} />
          </Button>
        </div>
      </div>

      {/* Spacer */}
      <div className="flex-1" />

      {/* App toggles - compact grid */}
      <div className="mt-3 pt-2 border-t border-border/50">
        <div className="flex items-center justify-between text-xs text-muted-foreground mb-2">
          <span>{t("skills.enabledApps", { defaultValue: "Enabled" })}</span>
          <span className="font-medium">{enabledCount}/4</span>
        </div>
        <div className="grid grid-cols-2 gap-x-3 gap-y-1.5">
          <AppToggle
            label="Claude"
            checked={skill.apps.claude}
            onChange={(checked) => onToggleApp(skill.id, "claude", checked)}
          />
          <AppToggle
            label="Codex"
            checked={skill.apps.codex}
            onChange={(checked) => onToggleApp(skill.id, "codex", checked)}
          />
          <AppToggle
            label="Gemini"
            checked={skill.apps.gemini}
            onChange={(checked) => onToggleApp(skill.id, "gemini", checked)}
          />
          <AppToggle
            label="OpenCode"
            checked={skill.apps.opencode}
            onChange={(checked) => onToggleApp(skill.id, "opencode", checked)}
          />
        </div>
      </div>
    </div>
  );
};

// Compact app toggle component
interface AppToggleProps {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}

const AppToggle: React.FC<AppToggleProps> = ({ label, checked, onChange }) => {
  return (
    <div className="flex items-center justify-between">
      <span
        className={cn(
          "text-xs",
          checked ? "text-foreground" : "text-muted-foreground",
        )}
      >
        {label}
      </span>
      <Switch
        checked={checked}
        onCheckedChange={onChange}
        className="scale-75 origin-right"
      />
    </div>
  );
};

export default SkillCardCompact;
