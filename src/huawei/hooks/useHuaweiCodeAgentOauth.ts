import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { huaweiCodeAgent2Api } from "../huaweiCodeAgent2Api";
import type { HuaweiCodeAgentAccount } from "../types";

const QUERY_KEY = ["huawei-codeagent-2-accounts"];

export function useHuaweiCodeAgentOauth() {
  const queryClient = useQueryClient();

  const {
    data: accounts = [],
    isLoading: isLoadingAccounts,
  } = useQuery<HuaweiCodeAgentAccount[]>({
    queryKey: QUERY_KEY,
    queryFn: () => huaweiCodeAgent2Api.listAccounts(),
    staleTime: 10_000,
  });

  const defaultAccount = accounts.find((a) => a.is_default);
  const defaultAccountId = defaultAccount?.id ?? null;
  const hasAnyAccount = accounts.length > 0;
  const usableAccounts = accounts.filter((a) => !a.requires_reauth);
  const isAuthenticated = usableAccounts.length > 0;

  const invalidate = async () => {
    await queryClient.invalidateQueries({ queryKey: QUERY_KEY });
  };

  const addAccountMutation = useMutation({
    mutationFn: ({ user, password }: { user: string; password: string }) =>
      huaweiCodeAgent2Api.startLogin(user, password),
    onSuccess: invalidate,
  });

  const removeAccountMutation = useMutation({
    mutationFn: (accountId: string) => huaweiCodeAgent2Api.removeAccount(accountId),
    onSuccess: invalidate,
  });

  const setDefaultMutation = useMutation({
    mutationFn: (accountId: string) => huaweiCodeAgent2Api.setDefaultAccount(accountId),
    onSuccess: invalidate,
  });

  const logoutMutation = useMutation({
    mutationFn: () => huaweiCodeAgent2Api.logout(),
    onSuccess: invalidate,
  });

  return {
    accounts,
    defaultAccountId,
    hasAnyAccount,
    isAuthenticated,
    isLoadingAccounts,
    addAccount: addAccountMutation.mutateAsync,
    isAddingAccount: addAccountMutation.isPending,
    removeAccount: removeAccountMutation.mutateAsync,
    isRemovingAccount: removeAccountMutation.isPending,
    setDefaultAccount: setDefaultMutation.mutateAsync,
    isSettingDefaultAccount: setDefaultMutation.isPending,
    logout: logoutMutation.mutateAsync,
  };
}
