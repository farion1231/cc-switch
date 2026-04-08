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
import { RuleCard } from "./RuleCard";
import { RuleRepoManagerPanel } from "./RuleRepoManagerPanel";
import {
  useDiscoverableRules,
  useInstalledRules,
  useInstallRule,
  useRuleRepos,
  useAddRuleRepo,
  useRemoveRuleRepo,
} from "@/hooks/useRules";
import type { AppId } from "@/lib/api/types";
import type { DiscoverableRule, RuleRepo } from "@/lib/api/rules";
import { formatRuleError } from "@/lib/errors/ruleErrorParser";

interface RulesPageProps {
  initialApp?: AppId;
}

export interface RulesPageHandle {
  refresh: () => void;
  openRepoManager: () => void;
}

export const RulesPage = forwardRef<RulesPageHandle, RulesPageProps>(
  ({ initialApp = "claude" }, ref) => {
    const { t } = useTranslation();
    const [repoManagerOpen, setRepoManagerOpen] = useState(false);
    const [searchQuery, setSearchQuery] = useState("");
    const [filterRepo, setFilterRepo] = useState<string>("all");
    const [filterStatus, setFilterStatus] = useState<
      "all" | "installed" | "uninstalled"
    >("all");

    const currentApp = initialApp;

    const {
      data: discoverableRules,
      isLoading: loadingDiscoverable,
      isFetching: fetchingDiscoverable,
      refetch: refetchDiscoverable,
    } = useDiscoverableRules();
    const { data: installedRules } = useInstalledRules();
    const { data: repos = [], refetch: refetchRepos } = useRuleRepos();

    const installMutation = useInstallRule();
    const addRepoMutation = useAddRuleRepo();
    const removeRepoMutation = useRemoveRuleRepo();

    const installedKeys = useMemo(() => {
      if (!installedRules) return new Set<string>();
      return new Set(installedRules.map((r) => r.id.toLowerCase()));
    }, [installedRules]);

    type DiscoverableRuleItem = DiscoverableRule & { installed: boolean };

    const repoOptions = useMemo(() => {
      if (!discoverableRules) return [];
      const repoSet = new Set<string>();
      discoverableRules.forEach((r) => {
        if (r.repoOwner && r.repoName) {
          repoSet.add(`${r.repoOwner}/${r.repoName}`);
        }
      });
      return Array.from(repoSet).sort();
    }, [discoverableRules]);

    const rules: DiscoverableRuleItem[] = useMemo(() => {
      if (!discoverableRules) return [];
      return discoverableRules.map((d) => {
        return {
          ...d,
          installed: installedKeys.has(d.key.toLowerCase()),
        };
      });
    }, [discoverableRules, installedKeys]);

    const loading = loadingDiscoverable || fetchingDiscoverable;

    useImperativeHandle(ref, () => ({
      refresh: () => {
        refetchDiscoverable();
        refetchRepos();
      },
      openRepoManager: () => setRepoManagerOpen(true),
    }));

    const handleInstall = async (directory: string) => {
      const rule = discoverableRules?.find(
        (r) =>
          r.directory === directory ||
          r.directory.split("/").pop() === directory,
      );
      if (!rule) {
        toast.error(t("rules.notFound"));
        return;
      }

      try {
        await installMutation.mutateAsync({
          rule,
          currentApp,
        });
        toast.success(t("rules.installSuccess", { name: rule.name }), {
          closeButton: true,
        });
      } catch (error) {
        const errorMessage =
          error instanceof Error ? error.message : String(error);
        const { title, description } = formatRuleError(
          errorMessage,
          t,
          "rules.installFailed",
        );
        toast.error(title, {
          description,
          duration: 10000,
        });
        console.error("Install rule failed:", error);
      }
    };

    const handleUninstall = async (_directory: string) => {
      toast.info(t("rules.uninstallInMainPanel"));
    };

    const handleAddRepo = async (repo: RuleRepo) => {
      try {
        await addRepoMutation.mutateAsync(repo);
        const { data: freshRules } = await refetchDiscoverable();
        const count =
          freshRules?.filter(
            (r) =>
              r.repoOwner === repo.owner &&
              r.repoName === repo.name &&
              (r.repoBranch || "main") === (repo.branch || "main"),
          ).length ?? 0;
        toast.success(
          t("rules.repo.addSuccess", {
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
        toast.success(t("rules.repo.removeSuccess", { owner, name }), {
          closeButton: true,
        });
      } catch (error) {
        toast.error(t("common.error"), {
          description: String(error),
        });
      }
    };

    const filteredRules = useMemo(() => {
      const byRepo = rules.filter((rule) => {
        if (filterRepo === "all") return true;
        const ruleRepo = `${rule.repoOwner}/${rule.repoName}`;
        return ruleRepo === filterRepo;
      });

      const byStatus = byRepo.filter((rule) => {
        if (filterStatus === "installed") return rule.installed;
        if (filterStatus === "uninstalled") return !rule.installed;
        return true;
      });

      if (!searchQuery.trim()) return byStatus;

      const query = searchQuery.toLowerCase();
      return byStatus.filter((rule) => {
        const name = rule.name?.toLowerCase() || "";
        const repo =
          rule.repoOwner && rule.repoName
            ? `${rule.repoOwner}/${rule.repoName}`.toLowerCase()
            : "";

        return name.includes(query) || repo.includes(query);
      });
    }, [rules, searchQuery, filterRepo, filterStatus]);

    return (
      <div className="px-6 flex flex-col flex-1 min-h-0 overflow-hidden bg-background/50">
        <div className="flex-1 overflow-y-auto overflow-x-hidden animate-fade-in">
          <div className="py-4">
            {loading ? (
              <div className="flex items-center justify-center h-64">
                <RefreshCw className="h-8 w-8 animate-spin text-muted-foreground" />
              </div>
            ) : rules.length === 0 ? (
              <div className="flex flex-col items-center justify-center h-64 text-center">
                <p className="text-lg font-medium text-foreground">
                  {t("rules.empty")}
                </p>
                <p className="mt-2 text-sm text-muted-foreground">
                  {t("rules.emptyDescription")}
                </p>
                <Button
                  variant="link"
                  onClick={() => setRepoManagerOpen(true)}
                  className="mt-3 text-sm font-normal"
                >
                  {t("rules.addRepo")}
                </Button>
              </div>
            ) : (
              <>
                <div className="mb-6 flex flex-col gap-3 md:flex-row md:items-center">
                  <div className="relative flex-1 min-w-0">
                    <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                    <Input
                      type="text"
                      placeholder={t("rules.searchPlaceholder")}
                      value={searchQuery}
                      onChange={(e) => setSearchQuery(e.target.value)}
                      className="pl-9 pr-3"
                    />
                  </div>
                  <div className="w-full md:w-56">
                    <Select value={filterRepo} onValueChange={setFilterRepo}>
                      <SelectTrigger className="bg-card border shadow-sm text-foreground">
                        <SelectValue
                          placeholder={t("rules.filter.repo")}
                          className="text-left truncate"
                        />
                      </SelectTrigger>
                      <SelectContent className="bg-card text-foreground shadow-lg max-h-64 min-w-[var(--radix-select-trigger-width)]">
                        <SelectItem
                          value="all"
                          className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                        >
                          {t("rules.filter.allRepos")}
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
                          placeholder={t("rules.filter.placeholder")}
                          className="text-left"
                        />
                      </SelectTrigger>
                      <SelectContent className="bg-card text-foreground shadow-lg">
                        <SelectItem
                          value="all"
                          className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                        >
                          {t("rules.filter.all")}
                        </SelectItem>
                        <SelectItem
                          value="installed"
                          className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                        >
                          {t("rules.filter.installed")}
                        </SelectItem>
                        <SelectItem
                          value="uninstalled"
                          className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                        >
                          {t("rules.filter.uninstalled")}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  {searchQuery && (
                    <p className="mt-2 text-sm text-muted-foreground">
                      {t("rules.count", { count: filteredRules.length })}
                    </p>
                  )}
                </div>

                {filteredRules.length === 0 ? (
                  <div className="flex flex-col items-center justify-center h-48 text-center">
                    <p className="text-lg font-medium text-foreground">
                      {t("rules.noResults")}
                    </p>
                    <p className="mt-2 text-sm text-muted-foreground">
                      {t("rules.emptyDescription")}
                    </p>
                  </div>
                ) : (
                  <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                    {filteredRules.map((rule) => (
                      <RuleCard
                        key={rule.key}
                        rule={rule}
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
          <RuleRepoManagerPanel
            repos={repos}
            rules={rules}
            onAdd={handleAddRepo}
            onRemove={handleRemoveRepo}
            onClose={() => setRepoManagerOpen(false)}
          />
        )}
      </div>
    );
  },
);

RulesPage.displayName = "RulesPage";
