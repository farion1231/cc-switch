import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Bot, Import, Loader2, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { toast } from "sonner";
import type { ImportSkillSelection } from "@/lib/api/skills";
import {
  useImportSkillsFromApps,
  useScanUnmanagedSkills,
} from "@/hooks/useSkills";
import type { AppId } from "@/lib/api/types";
import { AppToggleGroup } from "@/components/common/AppToggleGroup";
import { SKILLS_APP_IDS } from "@/config/appConfig";
import { ScrollArea } from "@/components/ui/scroll-area";

interface AgentsPanelProps {
  onOpenChange: (open: boolean) => void;
}

/** Agent skills live in ~/.agents/skills and are shared across agents by
 *  default. Enable them per app here so dedicated skills only reach the apps
 *  you pick. Pi enablement is derived from the native directory, so it is not
 *  offered as a toggle. */
const AGENT_APP_IDS = SKILLS_APP_IDS.filter((app) => app !== "pi");

export function AgentsPanel({}: AgentsPanelProps) {
  const { t } = useTranslation();
  const { data: unmanaged, refetch, isFetching } = useScanUnmanagedSkills({
    enabled: true,
  });
  const importMutation = useImportSkillsFromApps();
  const [selectedApps, setSelectedApps] = useState<
    Record<string, Partial<Record<AppId, boolean>>>
  >({});

  const agentSkills = useMemo(
    () => (unmanaged ?? []).filter((skill) => skill.foundIn.includes("agents")),
    [unmanaged],
  );

  const selectedCount = agentSkills.filter((skill) =>
    Object.values(selectedApps[skill.directory] ?? {}).some(Boolean),
  ).length;

  const handleImport = async () => {
    const imports: ImportSkillSelection[] = agentSkills.map((skill) => ({
      directory: skill.directory,
      apps: {
        claude: Boolean(selectedApps[skill.directory]?.claude),
        codex: Boolean(selectedApps[skill.directory]?.codex),
        gemini: Boolean(selectedApps[skill.directory]?.gemini),
        grokbuild: Boolean(selectedApps[skill.directory]?.grokbuild),
        opencode: Boolean(selectedApps[skill.directory]?.opencode),
        openclaw: Boolean(selectedApps[skill.directory]?.openclaw),
        hermes: Boolean(selectedApps[skill.directory]?.hermes),
        pi: false,
        cursor: Boolean(selectedApps[skill.directory]?.cursor),
      },
    }));
    try {
      const imported = await importMutation.mutateAsync(imports);
      toast.success(t("agents.importSuccess", { count: imported.length }), {
        closeButton: true,
      });
      setSelectedApps({});
    } catch (error) {
      toast.error(t("skills.importFailed"), { description: String(error) });
    }
  };

  return (
    <div className="px-6 flex flex-col flex-1 min-h-0">
      <div className="flex items-center justify-between gap-4 py-4">
        <div className="min-w-0">
          <h2 className="text-lg font-semibold">{t("agents.title")}</h2>
          <p className="text-sm text-muted-foreground mt-1 max-w-2xl">
            {t("agents.description")}
          </p>
        </div>
        <div className="flex items-center gap-2 flex-shrink-0">
          <Button
            variant="outline"
            size="sm"
            onClick={() => refetch()}
            disabled={isFetching}
          >
            {isFetching ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <RefreshCw className="h-4 w-4" />
            )}
            {t("agents.refresh")}
          </Button>
          <Button
            size="sm"
            onClick={handleImport}
            disabled={selectedCount === 0 || importMutation.isPending}
          >
            {importMutation.isPending ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Import className="h-4 w-4" />
            )}
            {t("agents.import")}
          </Button>
        </div>
      </div>

      <ScrollArea className="-mr-3 flex-1 min-h-0" type="auto">
        <div className="pb-24 pr-3 space-y-3">
          {agentSkills.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-16 text-center">
              <div className="w-16 h-16 rounded-full bg-white/5 flex items-center justify-center mb-4">
                <Bot className="w-8 h-8 text-muted-foreground" />
              </div>
              <h3 className="text-lg font-medium">{t("agents.title")}</h3>
              <p className="text-sm text-muted-foreground max-w-md mt-2">
                {t("agents.empty")}
              </p>
            </div>
          ) : (
            agentSkills.map((skill) => (
              <div
                key={skill.directory}
                className="rounded-xl border border-border-default bg-background p-4"
              >
                <div className="min-w-0">
                  <div className="font-medium">{skill.name}</div>
                  {skill.description && (
                    <p className="text-sm text-muted-foreground line-clamp-2 mt-0.5">
                      {skill.description}
                    </p>
                  )}
                  <p
                    className="text-xs text-muted-foreground/50 mt-1 truncate"
                    title={skill.path}
                  >
                    {t("agents.source")}
                    {skill.path}
                  </p>
                </div>
                <div className="mt-3 flex items-center gap-2 flex-wrap">
                  <span className="text-xs text-muted-foreground">
                    {t("agents.perApp")}
                  </span>
                  <AppToggleGroup
                    apps={selectedApps[skill.directory] ?? {}}
                    appIds={AGENT_APP_IDS}
                    onToggle={(app, enabled) =>
                      setSelectedApps((prev) => ({
                        ...prev,
                        [skill.directory]: {
                          ...(prev[skill.directory] ?? {}),
                          [app]: enabled,
                        },
                      }))
                    }
                  />
                </div>
              </div>
            ))
          )}
        </div>
      </ScrollArea>
    </div>
  );
}