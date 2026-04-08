import { useState, useMemo, forwardRef, useImperativeHandle } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { RefreshCw, Search } from "lucide-react";
import { toast } from "sonner";
import { AgentCard } from "./AgentCard";
import { AgentRepoManagerPanel } from "./AgentRepoManagerPanel";
import {
  useDiscoverableAgents,
  useInstalledAgents,
  useInstallAgent,
  useAgentRepos,
  useAddAgentRepo,
  useRemoveAgentRepo,
} from "@/hooks/useAgents";
import type { AppId } from "@/lib/api/types";
import type { DiscoverableAgent, AgentRepo } from "@/lib/api/agents";
import { formatAgentError } from "@/lib/errors/agentErrorParser";

interface AgentsDiscoveryPageProps {
  initialApp?: AppId;
}

export interface AgentsDiscoveryPageHandle {
  refresh: () => void;
  openRepoManager: () => void;
}

export const AgentsDiscoveryPage = forwardRef<
  AgentsDiscoveryPageHandle,
  AgentsDiscoveryPageProps
>(({ initialApp = "claude" }, ref) => {
  const { t } = useTranslation();
  const [repoManagerOpen, setRepoManagerOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [filterRepo, setFilterRepo] = useState<string>("all");
  const [filterStatus, setFilterStatus] = useState<
    "all" | "installed" | "uninstalled"
  >("all");

  const currentApp = initialApp;

  const {
    data: discoverableAgents,
    isLoading: loadingDiscoverable,
    isFetching: fetchingDiscoverable,
    refetch: refetchDiscoverable,
  } = useDiscoverableAgents();
  const { data: installedAgents } = useInstalledAgents();
  const { data: repos = [], refetch: refetchRepos } = useAgentRepos();

  const installMutation = useInstallAgent();
  const addRepoMutation = useAddAgentRepo();
  const removeRepoMutation = useRemoveAgentRepo();

  const installedKeys = useMemo(() => {
    if (!installedAgents) return new Set<string>();
    return new Set(installedAgents.map((a) => a.id.toLowerCase()));
  }, [installedAgents]);

  type DiscoverableAgentItem = DiscoverableAgent & { installed: boolean };

  const repoOptions = useMemo(() => {
    if (!discoverableAgents) return [];
    const repoSet = new Set<string>();
    discoverableAgents.forEach((a) => {
      if (a.repoOwner && a.repoName) {
        repoSet.add(`${a.repoOwner}/${a.repoName}`);
      }
    });
    return Array.from(repoSet).sort();
  }, [discoverableAgents]);

  const agents: DiscoverableAgentItem[] = useMemo(() => {
    if (!discoverableAgents) return [];
    return discoverableAgents.map((d) => {
      return {
        ...d,
        installed: installedKeys.has(d.key.toLowerCase()),
      };
    });
  }, [discoverableAgents, installedKeys]);

  const loading = loadingDiscoverable || fetchingDiscoverable;

  useImperativeHandle(ref, () => ({
    refresh: () => {
      refetchDiscoverable();
      refetchRepos();
    },
    openRepoManager: () => setRepoManagerOpen(true),
  }));

  const handleInstall = async (directory: string) => {
    const agent = discoverableAgents?.find(
      (a) =>
        a.directory === directory || a.directory.split("/").pop() === directory,
    );
    if (!agent) {
      toast.error(t("agents.notFound"));
      return;
    }

    try {
      await installMutation.mutateAsync({
        agent,
        currentApp,
      });
      toast.success(t("agents.installSuccess", { name: agent.name }), {
        closeButton: true,
      });
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : String(error);
      const { title, description } = formatAgentError(
        errorMessage,
        t,
        "agents.installFailed",
      );
      toast.error(title, {
        description,
        duration: 10000,
      });
      console.error("Install agent failed:", error);
    }
  };

  const handleUninstall = async (_directory: string) => {
    toast.info(t("agents.uninstallInMainPanel"));
  };

  const handleAddRepo = async (repo: AgentRepo) => {
    try {
      await addRepoMutation.mutateAsync(repo);
      const { data: freshAgents } = await refetchDiscoverable();
      const count =
        freshAgents?.filter(
          (a) =>
            a.repoOwner === repo.owner &&
            a.repoName === repo.name &&
            (a.repoBranch || "main") === (repo.branch || "main"),
        ).length ?? 0;
      toast.success(
        t("agents.repo.addSuccess", {
          owner: repo.owner,
          name: repo.name,
          count,
        }),
        { closeButton: true },
      );
    } catch (error) {
      toast.error(t("common.error"), {
        description: String(error),
      });
    }
  };

  const handleRemoveRepo = async (owner: string, name: string) => {
    try {
      await removeRepoMutation.mutateAsync({ owner, name });
      toast.success(t("agents.repo.removeSuccess", { owner, name }), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(t("common.error"), {
        description: String(error),
      });
    }
  };

  const filteredAgents = useMemo(() => {
    const byRepo = agents.filter((agent) => {
      if (filterRepo === "all") return true;
      const agentRepo = `${agent.repoOwner}/${agent.repoName}`;
      return agentRepo === filterRepo;
    });

    const byStatus = byRepo.filter((agent) => {
      if (filterStatus === "installed") return agent.installed;
      if (filterStatus === "uninstalled") return !agent.installed;
      return true;
    });

    if (!searchQuery.trim()) return byStatus;

    const query = searchQuery.toLowerCase();
    return byStatus.filter((agent) => {
      const name = agent.name?.toLowerCase() || "";
      const repo =
        agent.repoOwner && agent.repoName
          ? `${agent.repoOwner}/${agent.repoName}`.toLowerCase()
          : "";

      return name.includes(query) || repo.includes(query);
    });
  }, [agents, searchQuery, filterRepo, filterStatus]);

  return (
    <div className="px-6 flex flex-col flex-1 min-h-0 overflow-hidden bg-background/50">
      <div className="flex-1 overflow-y-auto overflow-x-hidden animate-fade-in">
        <div className="py-4">
          {loading ? (
            <div className="flex items-center justify-center h-64">
              <RefreshCw className="h-8 w-8 animate-spin text-muted-foreground" />
            </div>
          ) : agents.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-64 text-center">
              <p className="text-lg font-medium text-foreground">
                {t("agents.empty")}
              </p>
              <p className="mt-2 text-sm text-muted-foreground">
                {t("agents.emptyDescription")}
              </p>
              <Button
                variant="link"
                onClick={() => setRepoManagerOpen(true)}
                className="mt-3 text-sm font-normal"
              >
                {t("agents.addRepo")}
              </Button>
            </div>
          ) : (
            <>
              <div className="mb-6 flex flex-col gap-3 md:flex-row md:items-center">
                <div className="relative flex-1 min-w-0">
                  <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                  <Input
                    type="text"
                    placeholder={t("agents.searchPlaceholder")}
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    className="pl-9 pr-3"
                  />
                </div>
                <div className="w-full md:w-56">
                  <Select value={filterRepo} onValueChange={setFilterRepo}>
                    <SelectTrigger className="bg-card border shadow-sm text-foreground">
                      <SelectValue
                        placeholder={t("agents.filter.repo")}
                        className="text-left truncate"
                      />
                    </SelectTrigger>
                    <SelectContent className="bg-card text-foreground shadow-lg max-h-64 min-w-[var(--radix-select-trigger-width)]">
                      <SelectItem
                        value="all"
                        className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                      >
                        {t("agents.filter.allRepos")}
                      </SelectItem>
                      {repoOptions.map((repo) => (
                        <SelectItem
                          key={repo}
                          value={repo}
                          className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                          title={repo}
                        >
                          <span className="truncate block max-w-[200px]">
                            {repo}
                          </span>
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <div className="w-full md:w-36">
                  <Select
                    value={filterStatus}
                    onValueChange={(val) =>
                      setFilterStatus(
                        val as "all" | "installed" | "uninstalled",
                      )
                    }
                  >
                    <SelectTrigger className="bg-card border shadow-sm text-foreground">
                      <SelectValue
                        placeholder={t("agents.filter.placeholder")}
                        className="text-left"
                      />
                    </SelectTrigger>
                    <SelectContent className="bg-card text-foreground shadow-lg">
                      <SelectItem
                        value="all"
                        className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                      >
                        {t("agents.filter.all")}
                      </SelectItem>
                      <SelectItem
                        value="installed"
                        className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                      >
                        {t("agents.filter.installed")}
                      </SelectItem>
                      <SelectItem
                        value="uninstalled"
                        className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                      >
                        {t("agents.filter.uninstalled")}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                </div>
                {searchQuery && (
                  <p className="mt-2 text-sm text-muted-foreground">
                    {t("agents.count", { count: filteredAgents.length })}
                  </p>
                )}
              </div>

              {filteredAgents.length === 0 ? (
                <div className="flex flex-col items-center justify-center h-48 text-center">
                  <p className="text-lg font-medium text-foreground">
                    {t("agents.noResults")}
                  </p>
                  <p className="mt-2 text-sm text-muted-foreground">
                    {t("agents.emptyDescription")}
                  </p>
                </div>
              ) : (
                <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                  {filteredAgents.map((agent) => (
                    <AgentCard
                      key={agent.key}
                      agent={agent}
                      onInstall={handleInstall}
                      onUninstall={handleUninstall}
                    />
                  ))}
                </div>
              )}
            </>
          )}
        </div>
      </div>

      {repoManagerOpen && (
        <AgentRepoManagerPanel
          repos={repos}
          agents={agents}
          onAdd={handleAddRepo}
          onRemove={handleRemoveRepo}
          onClose={() => setRepoManagerOpen(false)}
        />
      )}
    </div>
  );
});

AgentsDiscoveryPage.displayName = "AgentsDiscoveryPage";
