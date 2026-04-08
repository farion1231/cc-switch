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
import type { DiscoverableRule } from "@/lib/api/rules";

type RuleCardRule = DiscoverableRule & { installed: boolean };

interface RuleCardProps {
  rule: RuleCardRule;
  onInstall: (directory: string) => Promise<void>;
  onUninstall: (directory: string) => Promise<void>;
}

export function RuleCard({ rule, onInstall, onUninstall }: RuleCardProps) {
  const { t } = useTranslation();
  const [loading, setLoading] = useState(false);

  const handleInstall = async () => {
    setLoading(true);
    try {
      await onInstall(rule.directory);
    } finally {
      setLoading(false);
    }
  };

  const handleUninstall = async () => {
    setLoading(true);
    try {
      await onUninstall(rule.directory);
    } finally {
      setLoading(false);
    }
  };

  const handleOpenGithub = async () => {
    if (rule.readmeUrl) {
      try {
        await settingsApi.openExternal(rule.readmeUrl);
      } catch (error) {
        console.error("Failed to open URL:", error);
      }
    }
  };

  const showDirectory =
    Boolean(rule.directory) &&
    rule.directory.trim().toLowerCase() !== rule.name.trim().toLowerCase();

  return (
    <Card className="glass-card flex flex-col h-full transition-all duration-300 hover:shadow-lg group relative overflow-hidden">
      <div className="absolute inset-0 bg-gradient-to-br from-primary/5 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500 pointer-events-none" />
      <CardHeader className="pb-3">
        <div className="flex items-start justify-between gap-2">
          <div className="flex-1 min-w-0">
            <CardTitle className="text-base font-semibold truncate">
              {rule.name}
            </CardTitle>
            <div className="flex items-center gap-2 mt-1.5">
              {showDirectory && (
                <CardDescription className="text-xs truncate">
                  {rule.directory}
                </CardDescription>
              )}
              {rule.repoOwner && rule.repoName && (
                <Badge
                  variant="outline"
                  className="shrink-0 text-[10px] px-1.5 py-0 h-4 border-border-default"
                >
                  {rule.repoOwner}/{rule.repoName}
                </Badge>
              )}
            </div>
          </div>
          {rule.installed && (
            <Badge
              variant="default"
              className="shrink-0 bg-green-600/90 hover:bg-green-600 dark:bg-green-700/90 dark:hover:bg-green-700 text-white border-0"
            >
              {t("rules.installed")}
            </Badge>
          )}
        </div>
      </CardHeader>
      <CardContent className="flex-1 pt-0">
        <p className="text-sm text-muted-foreground/90 line-clamp-4 leading-relaxed">
          {rule.description || t("rules.noDescription")}
        </p>
      </CardContent>
      <CardFooter className="flex gap-2 pt-3 border-t border-border/50 relative z-10">
        {rule.readmeUrl && (
          <Button
            variant="ghost"
            size="sm"
            onClick={handleOpenGithub}
            disabled={loading}
            className="flex-1"
          >
            <ExternalLink className="h-3.5 w-3.5 mr-1.5" />
            {t("rules.view")}
          </Button>
        )}
        {rule.installed ? (
          <Button
            variant="outline"
            size="sm"
            onClick={handleUninstall}
            disabled={loading}
            className="flex-1 border-red-200 text-red-600 hover:bg-red-50 hover:text-red-700 dark:border-red-900/50 dark:text-red-400 dark:hover:bg-red-950/50 dark:hover:text-red-300"
          >
            {loading ? (
              <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
            ) : (
              <Trash2 className="h-3.5 w-3.5 mr-1.5" />
            )}
            {loading ? t("rules.uninstalling") : t("rules.uninstall")}
          </Button>
        ) : (
          <Button
            variant="mcp"
            size="sm"
            onClick={handleInstall}
            disabled={loading || !rule.repoOwner}
            className="flex-1"
          >
            {loading ? (
              <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
            ) : (
              <Download className="h-3.5 w-3.5 mr-1.5" />
            )}
            {loading ? t("rules.installing") : t("rules.install")}
          </Button>
        )}
      </CardFooter>
    </Card>
  );
}
