import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Loader2, Play, Search, X } from "lucide-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { authApi } from "@/lib/api";
import type { ManagedAuthProvider, ManagedAuthStatus } from "@/lib/api/auth";
import { kimiOAuthApi, type KimiOAuthStatus } from "@/lib/api/kimiOAuth";
import { mcodeOAuthApi, type McodeOAuthStatus } from "@/lib/api/mcodeOAuth";
import { canSwitchAgyAccount } from "@/lib/authBinding";
import { subscriptionKeys } from "@/lib/query/subscription";
import { extractErrorMessage } from "@/utils/errorUtils";
import { isTextEditableTarget } from "@/utils/domUtils";
import { resolveProviderIcon } from "@/utils/providerIcon";
import { WORKBUDDY_ACCOUNT_AUTH_PROVIDER } from "@/config/workbuddyProviderPresets";
import { ProviderIconBox } from "@/components/ProviderIconBox";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  AgyAccountQuota,
  CopilotAccountQuota,
  KimiAccountQuota,
  McodeAccountQuota,
  WorkBuddyAccountQuota,
} from "@/components/settings/auth/AccountQuota";
import {
  DshBalance,
  accountName as dshAccountName,
  useDshAccountStatus,
} from "@/components/providers/DshAccount";
import { ProviderEmptyState } from "@/components/providers/ProviderEmptyState";
import { CardChipBadge } from "@/components/providers/ProviderCard";
import type { CardChip } from "@/components/providers/presentation";
import { useWorkBuddySwitch } from "@/components/providers/useWorkBuddyOfficial";
import {
  WORKBUDDY_LIVE_KEY,
  WORKBUDDY_STATUS_KEY,
} from "@/components/providers/workbuddyOfficial";
import { WORKBUDDY_QUOTA_KEY } from "@/components/providers/workbuddyQuota";

export type OfficialPanelAppId =
  | "agy"
  | "copilot"
  | "workbuddy"
  | "kimi"
  | "mcode"
  | "dsh";

interface OfficialAccountsPanelProps {
  appId: OfficialPanelAppId;
  searchOpen?: boolean;
  onSearchOpenChange?: (open: boolean) => void;
  onOpenAuthCenter?: () => void;
  onOpenAggregation?: () => void;
}

interface OfficialAccountItem {
  id: string;
  name: string;
  subtitle?: string | null;
  isCurrent: boolean;
  needsReauth: boolean;
  canSwitch: boolean;
  quota?: ReactNode;
}

const APP_ICON_HINT: Record<OfficialPanelAppId, string> = {
  agy: "antigravity",
  copilot: "copilot",
  workbuddy: "workbuddy",
  kimi: "kimi",
  mcode: "minimax",
  dsh: "deepseek",
};

const APP_TO_MANAGED_AUTH: Partial<
  Record<OfficialPanelAppId, ManagedAuthProvider>
> = {
  agy: "agy_oauth",
  copilot: "github_copilot",
  workbuddy: WORKBUDDY_ACCOUNT_AUTH_PROVIDER,
};

export function OfficialAccountsPanel({
  appId,
  searchOpen: controlledSearchOpen,
  onSearchOpenChange,
  onOpenAuthCenter,
  onOpenAggregation,
}: OfficialAccountsPanelProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [searchTerm, setSearchTerm] = useState("");
  const [internalSearchOpen, setInternalSearchOpen] = useState(false);
  const searchInputRef = useRef<HTMLInputElement>(null);

  const isSearchControlled = controlledSearchOpen !== undefined;
  const isSearchOpen = isSearchControlled
    ? controlledSearchOpen
    : internalSearchOpen;
  const setSearchOpen = (open: boolean) => {
    if (isSearchControlled) {
      onSearchOpenChange?.(open);
    } else {
      setInternalSearchOpen(open);
    }
  };

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "f") {
        if (isTextEditableTarget(event.target)) return;
        event.preventDefault();
        setSearchOpen(true);
        setTimeout(() => searchInputRef.current?.focus(), 0);
      } else if (event.key === "Escape" && isSearchOpen) {
        event.preventDefault();
        setSearchOpen(false);
        setSearchTerm("");
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isSearchOpen, isSearchControlled, onSearchOpenChange]);

  const managedProvider = APP_TO_MANAGED_AUTH[appId];
  const isWorkBuddy = appId === "workbuddy";
  const isKimi = appId === "kimi";
  const isMcode = appId === "mcode";
  const isDsh = appId === "dsh";

  const managedStatusQuery = useQuery<ManagedAuthStatus>({
    queryKey: isWorkBuddy
      ? WORKBUDDY_STATUS_KEY
      : ["managed-auth-status", managedProvider],
    queryFn: () => authApi.authGetStatus(managedProvider!),
    enabled: Boolean(managedProvider),
    staleTime: 30_000,
    refetchInterval: isWorkBuddy ? 5_000 : false,
  });

  const workbuddyLiveQuery = useQuery({
    queryKey: WORKBUDDY_LIVE_KEY,
    queryFn: authApi.workbuddyAccountLiveState,
    enabled: isWorkBuddy,
    refetchInterval: isWorkBuddy ? 5_000 : false,
  });

  const workbuddyRunning = workbuddyLiveQuery.data?.running ?? false;
  const wasWorkbuddyRunning = useRef(workbuddyRunning);
  useEffect(() => {
    if (workbuddyRunning && !wasWorkbuddyRunning.current) {
      void queryClient.invalidateQueries({ queryKey: WORKBUDDY_QUOTA_KEY });
    }
    wasWorkbuddyRunning.current = workbuddyRunning;
  }, [workbuddyRunning, queryClient]);

  const kimiStatusQuery = useQuery<KimiOAuthStatus>({
    queryKey: ["kimi-oauth-status"],
    queryFn: () => kimiOAuthApi.status(),
    enabled: isKimi,
    staleTime: 30_000,
  });

  const mcodeStatusQuery = useQuery<McodeOAuthStatus>({
    queryKey: ["mcode-oauth-status"],
    queryFn: () => mcodeOAuthApi.status(),
    enabled: isMcode,
    staleTime: 30_000,
  });

  const dshStatusQuery = useDshAccountStatus(isDsh);

  const { switchToAccount: switchWorkBuddy, switching: switchingWorkBuddy } =
    useWorkBuddySwitch();

  const [switchingAccountId, setSwitchingAccountId] = useState<string | null>(
    null,
  );

  const switchManagedMutation = useMutation({
    mutationFn: ({
      provider,
      accountId,
    }: {
      provider: ManagedAuthProvider;
      accountId: string;
    }) => authApi.authSetDefaultAccount(provider, accountId),
    onSuccess: async (_data, { provider }) => {
      if (provider === "agy_oauth") {
        const running = await authApi.agyAccountIsRunning().catch(() => false);
        if (running) {
          toast.warning(
            t("authCenter.agy.switchedWhileRunning", {
              defaultValue:
                "已切换。检测到 agy 正在运行：已打开的 agy 仍用原账号，还可能把原账号写回去，请退出后重新打开。",
            }),
          );
        } else {
          toast.success(
            t("authCenter.agy.switched", {
              defaultValue: "已切换，新开的 agy 会使用这个账号。",
            }),
          );
        }
        await queryClient.invalidateQueries({
          queryKey: subscriptionKeys.quota("agy"),
        });
      } else {
        toast.success(
          t("provider.accountSwitched", {
            defaultValue: "已切换当前账号",
          }),
        );
      }
      await queryClient.invalidateQueries({
        queryKey: ["managed-auth-status", provider],
      });
    },
    onError: (error) => {
      toast.error(extractErrorMessage(error) || String(error));
    },
    onSettled: () => setSwitchingAccountId(null),
  });

  const switchNativeOAuthMutation = useMutation({
    mutationFn: ({
      target,
      accountId,
    }: {
      target: "kimi" | "mcode";
      accountId: string;
    }) =>
      target === "kimi"
        ? kimiOAuthApi.useAccount(accountId)
        : mcodeOAuthApi.useAccount(accountId),
    onSuccess: async (_data, { target }) => {
      toast.success(
        t("provider.accountSwitched", {
          defaultValue: "已切换当前账号",
        }),
      );
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: [target === "kimi" ? "kimi-oauth-status" : "mcode-oauth-status"],
        }),
        queryClient.invalidateQueries({ queryKey: ["providers", target] }),
      ]);
    },
    onError: (error) => {
      toast.error(extractErrorMessage(error) || String(error));
    },
    onSettled: () => setSwitchingAccountId(null),
  });

  const managedHint = t("provider.managedInAuthCenterHint", {
    defaultValue: "账号在授权中心管理",
  });

  const accounts = useMemo<OfficialAccountItem[]>(() => {
    if (managedProvider) {
      const status = managedStatusQuery.data;
      if (!status) return [];
      const defaultId = status.default_account_id;
      return status.accounts.map((account) => {
        const reauth = Boolean(account.requires_reauth);
        let quota: ReactNode = undefined;
        if (!reauth) {
          if (appId === "agy") {
            quota = <AgyAccountQuota accountId={account.id} login={account.login} />;
          } else if (appId === "copilot") {
            quota = (
              <CopilotAccountQuota accountId={account.id} login={account.login} />
            );
          } else if (appId === "workbuddy") {
            quota = (
              <WorkBuddyAccountQuota accountId={account.id} login={account.login} />
            );
          }
        }
        return {
          id: account.id,
          name: account.login,
          subtitle: reauth
            ? t("managedAuth.selectedAccountNeedsReauth", {
                defaultValue: "已绑定账号不存在或需要重新登录",
              })
            : managedHint,
          isCurrent: defaultId === account.id,
          needsReauth: reauth,
          canSwitch: appId === "agy" ? canSwitchAgyAccount() : true,
          quota,
        };
      });
    }

    if (isKimi) {
      const status = kimiStatusQuery.data;
      if (!status) return [];
      return status.accounts
        .filter((account) => account.authenticated)
        .map((account) => {
          const displayName = account.nickname || account.name;
          return {
            id: account.id,
            name: displayName,
            subtitle:
              account.nickname && account.nickname !== account.name
                ? account.name
                : managedHint,
            isCurrent: status.currentAccountId === account.id,
            needsReauth: false,
            canSwitch: true,
            quota: <KimiAccountQuota accountId={account.id} login={displayName} />,
          };
        });
    }

    if (isMcode) {
      const status = mcodeStatusQuery.data;
      if (!status) return [];
      return status.accounts
        .filter((account) => account.authenticated)
        .map((account) => ({
          id: account.id,
          name: account.name,
          subtitle:
            account.email && account.email !== account.name
              ? account.email
              : managedHint,
          isCurrent: status.currentAccountId === account.id,
          needsReauth: false,
          canSwitch: true,
          quota: <McodeAccountQuota accountId={account.id} login={account.name} />,
        }));
    }

    if (isDsh) {
      const status = dshStatusQuery.data;
      if (!status?.signedIn) return [];
      const name = dshAccountName(status, t("dsh.account.unnamed"));
      return [
        {
          id: "default",
          name,
          subtitle: status.expired
            ? t("dsh.account.expired")
            : t("dsh.account.cardSubtitle"),
          isCurrent: !status.expired,
          needsReauth: Boolean(status.expired),
          canSwitch: false,
          quota:
            !status.expired && !status.error ? (
              <DshBalance
                login={name}
                status={status}
                queriedAt={dshStatusQuery.dataUpdatedAt}
                loading={dshStatusQuery.isFetching}
                onRefresh={() => void dshStatusQuery.refetch()}
              />
            ) : undefined,
        },
      ];
    }

    return [];
  }, [
    appId,
    dshStatusQuery.data,
    dshStatusQuery.dataUpdatedAt,
    dshStatusQuery.isFetching,
    dshStatusQuery.refetch,
    isDsh,
    isKimi,
    isMcode,
    kimiStatusQuery.data,
    mcodeStatusQuery.data,
    managedHint,
    managedProvider,
    managedStatusQuery.data,
    t,
  ]);

  const isLoading = managedProvider
    ? managedStatusQuery.isLoading
    : isKimi
      ? kimiStatusQuery.isLoading
      : isMcode
        ? mcodeStatusQuery.isLoading
        : isDsh
          ? dshStatusQuery.isLoading
          : false;

  const filteredAccounts = useMemo(() => {
    const keyword = searchTerm.trim().toLowerCase();
    if (!keyword) return accounts;
    return accounts.filter(
      (account) =>
        account.name.toLowerCase().includes(keyword) ||
        account.subtitle?.toLowerCase().includes(keyword),
    );
  }, [accounts, searchTerm]);

  const handleSwitch = async (account: OfficialAccountItem) => {
    if (account.isCurrent || account.needsReauth || !account.canSwitch) return;
    if (appId === "workbuddy") {
      setSwitchingAccountId(account.id);
      try {
        await switchWorkBuddy(account.id, account.name);
      } finally {
        setSwitchingAccountId(null);
      }
      return;
    }
    if (appId === "kimi" || appId === "mcode") {
      setSwitchingAccountId(account.id);
      switchNativeOAuthMutation.mutate({ target: appId, accountId: account.id });
      return;
    }
    if (managedProvider) {
      setSwitchingAccountId(account.id);
      switchManagedMutation.mutate({
        provider: managedProvider,
        accountId: account.id,
      });
    }
  };

  if (isLoading) {
    return (
      <div className="space-y-2">
        {[0, 1, 2].map((index) => (
          <div
            key={index}
            className="h-[60px] w-full rounded-panel border border-dashed border-border bg-subtle"
          />
        ))}
      </div>
    );
  }

  if (accounts.length === 0) {
    return (
      <ProviderEmptyState
        appId={appId}
        officialOnly
        onOpenAuthCenter={onOpenAuthCenter}
        onOpenAggregation={onOpenAggregation}
      />
    );
  }

  const icon = resolveProviderIcon(appId, APP_ICON_HINT[appId]);
  const isAnySwitching =
    switchingWorkBuddy ||
    switchManagedMutation.isPending ||
    switchNativeOAuthMutation.isPending;

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-2">
        <div className="text-caption font-medium text-fg-2">
          {t("provider.loggedInAccountsCount", {
            count: accounts.length,
            defaultValue: "已登录账号 · {{count}}",
          })}
        </div>
        <AnimatePresence>
          {isSearchOpen && (
            <motion.div
              key="official-account-search"
              initial={{ opacity: 0, x: 8 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: 8 }}
              transition={{ duration: 0.15, ease: "easeOut" }}
              className="relative flex w-64 min-w-[10rem] shrink items-center"
            >
              <Search className="pointer-events-none absolute start-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-fg-3" />
              <Input
                ref={searchInputRef}
                value={searchTerm}
                onChange={(event) => setSearchTerm(event.target.value)}
                placeholder={t("provider.searchPlaceholder", {
                  defaultValue: "Search name, notes, or URL...",
                })}
                aria-label={t("provider.searchAriaLabel", {
                  defaultValue: "Search providers",
                })}
                className="h-control-sm pe-7 ps-8 text-caption"
              />
              <button
                type="button"
                aria-label={t("common.clear", { defaultValue: "清除" })}
                onClick={() => {
                  setSearchTerm("");
                  setSearchOpen(false);
                }}
                className="absolute end-1.5 top-1/2 -translate-y-1/2 rounded p-0.5 text-fg-3 hover:text-fg-1"
              >
                <X className="h-3.5 w-3.5" />
              </button>
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      {filteredAccounts.length === 0 ? (
        <div className="rounded-panel border border-dashed border-border bg-subtle p-6 text-center text-caption text-fg-2">
          {t("provider.noSearchResults", {
            defaultValue: "未找到匹配的账号",
          })}
        </div>
      ) : (
        <div className="space-y-2">
          {filteredAccounts.map((account) => {
            const chips: CardChip[] = [
              {
                key: "official",
                label: t("providerCard.chip.official"),
                tone: "outline",
              },
            ];
            if (account.needsReauth) {
              chips.push({
                key: "expired",
                label: t("dsh.account.expiredChip", {
                  defaultValue: "已失效",
                }),
                tone: "warning",
              });
            }
            const isSwitchingThis =
              isAnySwitching && switchingAccountId === account.id;
            return (
              <div
                key={account.id}
                className="group relative rounded-panel border border-border bg-surface transition-[border-color,box-shadow,background-color] duration-150 hover:border-border-strong hover:shadow-v7-sm"
              >
                <div className="flex min-h-[58px] items-center gap-3 px-3 py-2.5">
                  <ProviderIconBox
                    icon={icon}
                    name={account.name}
                    className="bg-white text-neutral-900"
                    fallbackClassName="bg-transparent text-neutral-600"
                  />
                  <div className="min-w-0 flex-1">
                    <div className="flex min-w-0 items-center gap-1.5">
                      <h3
                        className="m-0 min-w-0 truncate text-strong text-fg-1"
                        title={account.name}
                      >
                        {account.name}
                      </h3>
                      {chips.map((chip) => (
                        <CardChipBadge key={chip.key} chip={chip} />
                      ))}
                    </div>
                    {account.subtitle && (
                      <div className="mt-0.5 truncate text-caption text-fg-2">
                        {account.subtitle}
                      </div>
                    )}
                  </div>

                  <div className="flex shrink-0 items-center gap-3">
                    {account.quota && (
                      <div className="hidden items-center gap-3 sm:flex">
                        {account.quota}
                      </div>
                    )}

                    <div className="flex min-w-20 items-center justify-end gap-1">
                      {account.isCurrent ? (
                        <span className="inline-flex items-center gap-1.5 whitespace-nowrap px-1 text-body font-medium text-fg-1">
                          <span
                            aria-hidden="true"
                            className="h-1.5 w-1.5 rounded-full bg-direct"
                          />
                          {t("providerCard.status.inUse", {
                            defaultValue: "使用中",
                          })}
                        </span>
                      ) : account.canSwitch && !account.needsReauth ? (
                        <Button
                          variant="neutral"
                          size="sm"
                          disabled={isAnySwitching}
                          onClick={() => void handleSwitch(account)}
                        >
                          {isSwitchingThis ? (
                            <Loader2 className="h-3.5 w-3.5 animate-spin" />
                          ) : (
                            <Play className="h-3.5 w-3.5" />
                          )}
                          {t("providerCard.action.switch", {
                            defaultValue: "切换",
                          })}
                        </Button>
                      ) : account.needsReauth && onOpenAuthCenter ? (
                        <Button
                          variant="neutral"
                          size="sm"
                          onClick={onOpenAuthCenter}
                        >
                          {t("provider.goToAuthCenter", {
                            defaultValue: "前往授权中心",
                          })}
                        </Button>
                      ) : null}
                    </div>
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
