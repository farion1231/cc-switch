import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import { ExternalLink, Download, Trash2, Loader2 } from "lucide-react";
import { settingsApi } from "@/lib/api";
import type { DiscoverableSkill } from "@/lib/api/skills";

type SkillItemSkill = DiscoverableSkill & { installed: boolean };

interface SkillItemProps {
  skill: SkillItemSkill;
  onInstall: (directory: string) => Promise<void>;
  onUninstall: (directory: string) => Promise<void>;
}

export function SkillItem({ skill, onInstall, onUninstall }: SkillItemProps) {
  const { t } = useTranslation();
  const [loading, setLoading] = useState(false);

  const handleInstall = async () => {
    setLoading(true);
    try {
      await onInstall(skill.directory);
    } finally {
      setLoading(false);
    }
  };

  const handleUninstall = async () => {
    setLoading(true);
    try {
      await onUninstall(skill.directory);
    } finally {
      setLoading(false);
    }
  };

  const handleOpenGithub = async () => {
    if (skill.readmeUrl) {
      try {
        await settingsApi.openExternal(skill.readmeUrl);
      } catch (error) {
        console.error("Failed to open URL:", error);
      }
    }
  };

  return (
    <Card className="glass-card px-4 py-3">
      <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <p className="text-sm font-semibold text-foreground truncate">
              {skill.name}
            </p>
            {skill.installed && (
              <Badge
                variant="default"
                className="shrink-0 bg-green-600/90 hover:bg-green-600 dark:bg-green-700/90 dark:hover:bg-green-700 text-white border-0"
              >
                {t("skills.installed")}
              </Badge>
            )}
            <Badge variant="outline" className="text-xs border-border-default">
              {skill.repoOwner}/{skill.repoName}
            </Badge>
          </div>
          <p className="mt-1 text-sm text-muted-foreground line-clamp-2">
            {skill.description || t("skills.noDescription")}
          </p>
        </div>
        <div className="flex items-center gap-2 shrink-0">
          {skill.readmeUrl && (
            <Button
              variant="ghost"
              size="sm"
              onClick={handleOpenGithub}
              disabled={loading}
            >
              <ExternalLink className="h-3.5 w-3.5 mr-1.5" />
              {t("skills.view")}
            </Button>
          )}
          {skill.installed ? (
            <Button
              variant="outline"
              size="sm"
              onClick={handleUninstall}
              disabled={loading}
              className="border-red-200 text-red-600 hover:bg-red-50 hover:text-red-700 dark:border-red-900/50 dark:text-red-400 dark:hover:bg-red-950/50 dark:hover:text-red-300"
            >
              {loading ? (
                <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
              ) : (
                <Trash2 className="h-3.5 w-3.5 mr-1.5" />
              )}
              {loading ? t("skills.uninstalling") : t("skills.uninstall")}
            </Button>
          ) : (
            <Button
              variant="mcp"
              size="sm"
              onClick={handleInstall}
              disabled={loading || !skill.repoOwner}
            >
              {loading ? (
                <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
              ) : (
                <Download className="h-3.5 w-3.5 mr-1.5" />
              )}
              {loading ? t("skills.installing") : t("skills.install")}
            </Button>
          )}
        </div>
      </div>
    </Card>
  );
}
