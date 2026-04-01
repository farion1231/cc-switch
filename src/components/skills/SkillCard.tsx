import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ExternalLink, Download, Trash2, Loader2 } from "lucide-react";
import { settingsApi } from "@/lib/api";
import type { DiscoverableSkill } from "@/lib/api/skills";

type SkillCardSkill = DiscoverableSkill & { installed: boolean };

interface SkillCardProps {
  skill: SkillCardSkill;
  onInstall: (directory: string) => Promise<void>;
  onUninstall: (directory: string) => Promise<void>;
  installs?: number;
}

export function SkillCard({
  skill,
  onInstall,
  onUninstall,
  installs,
}: SkillCardProps) {
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

  const showDirectory =
    Boolean(skill.directory) &&
    skill.directory.trim().toLowerCase() !== skill.name.trim().toLowerCase();

  return (
    <Card className="glass-card flex flex-col h-full transition-all duration-300 hover:shadow-lg group relative overflow-hidden">
      <div className="absolute inset-0 bg-gradient-to-br from-primary/5 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500 pointer-events-none" />
      <CardHeader className="pb-3">
        <div className="flex items-start justify-between gap-2">
          <div className="flex-1 min-w-0">
            <CardTitle className="text-base font-semibold truncate">
              {skill.name}
            </CardTitle>
            <div className="flex items-center gap-2 mt-1.5">
              {showDirectory && (
                <CardDescription className="text-xs truncate">
                  {skill.directory}
                </CardDescription>
              )}
              {skill.repoOwner && skill.repoName && (
                <Badge
                  variant="outline"
                  className="shrink-0 text-[10px] px-1.5 py-0 h-4 border-border-default"
                >
                  {skill.repoOwner}/{skill.repoName}
                </Badge>
              )}
              {typeof installs === "number" && (
                <Badge
                  variant="secondary"
                  className="shrink-0 text-[10px] px-1.5 py-0 h-4"
                >
                  <Download className="h-2.5 w-2.5 mr-0.5" />
                  {installs.toLocaleString()}
                </Badge>
              )}
            </div>
          </div>
          {skill.installed && (
            <Badge
              variant="default"
              className="shrink-0 border-0 bg-primary text-primary-foreground hover:bg-primary/90"
            >
              {t("skills.installed")}
            </Badge>
          )}
        </div>
      </CardHeader>
      {skill.description ? (
        <CardContent className="flex-1 pt-0">
          <p className="text-sm text-muted-foreground/90 line-clamp-4 leading-relaxed">
            {skill.description}
          </p>
        </CardContent>
      ) : (
        <div className="flex-1" />
      )}
      <CardFooter className="flex gap-2 pt-3 border-t border-border/50 relative z-10">
        {skill.readmeUrl && (
          <Button
            variant="ghost"
            size="sm"
            onClick={handleOpenGithub}
            disabled={loading}
            className="flex-1"
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
            className="flex-1 border-[hsl(var(--destructive)/0.24)] text-[hsl(var(--destructive))] hover:bg-[hsl(var(--destructive)/0.08)] hover:text-[hsl(var(--destructive))]"
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
            className="flex-1"
          >
            {loading ? (
              <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
            ) : (
              <Download className="h-3.5 w-3.5 mr-1.5" />
            )}
            {loading ? t("skills.installing") : t("skills.install")}
          </Button>
        )}
      </CardFooter>
    </Card>
  );
}
