import { useCallback, useMemo } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { authApi } from "@/lib/api";
import { toast } from "@/lib/toast";
import type { Provider } from "@/types";
import type { CodebuddyAccountsState } from "@/components/providers/presentation";
import { extractErrorMessage } from "@/utils/errorUtils";

const STATUS_KEY = ["managed-auth-status", "codebuddy_oauth"] as const;
const LIVE_KEY = ["codebuddy-account-live"] as const;

/**
 * 供应商列表里 CodeBuddy 官方登录卡要的东西：授权中心收录的账号、CodeBuddy 现在登着的号
 * （和授权中心共用同一份查询，15 秒刷新一次，CLI 里 /login 换号也能跟上），以及卡片上的「添加」
 * （切到卡片绑定的账号；同一时间只有一张官方卡算「已添加」）。
 */
export function useCodebuddyOfficialCards(enabled: boolean): {
  state: CodebuddyAccountsState;
  switchAccount: (provider: Provider) => void;
} {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const status = useQuery({
    queryKey: STATUS_KEY,
    queryFn: () => authApi.authGetStatus("codebuddy_oauth"),
    enabled,
    staleTime: 30_000,
    refetchInterval: enabled ? 15_000 : false,
  });

  const mutation = useMutation({
    mutationFn: (provider: Provider) =>
      authApi.codebuddyAccountSwitchProvider(provider.id),
    onSuccess: (_accountId, provider) => {
      toast.success(
        t("codebuddy.official.added", {
          defaultValue:
            "已添加 {{name}}，CodeBuddy 已登录这个账号。正在运行的 CodeBuddy 会自动换过去，新开的也会用它。",
          name: provider.name,
        }),
        { closeButton: true },
      );
    },
    onError: (error) => {
      toast.error(
        t("codebuddy.official.addFailed", {
          defaultValue: "添加失败：{{error}}",
          error: extractErrorMessage(error) || String(error),
        }),
        { closeButton: true },
      );
    },
    onSettled: async () => {
      await queryClient.invalidateQueries({ queryKey: STATUS_KEY });
      await queryClient.invalidateQueries({ queryKey: LIVE_KEY });
    },
  });

  const data = status.data;
  const state = useMemo<CodebuddyAccountsState>(
    () => ({
      status: status.isSuccess ? "ready" : status.isError ? "error" : "loading",
      currentAccountId: data?.default_account_id ?? null,
      accounts: (data?.accounts ?? []).map((account) => ({
        id: account.id,
        requiresReauth: account.requires_reauth,
      })),
      switchingId: mutation.isPending ? mutation.variables?.id : null,
    }),
    [
      status.isSuccess,
      status.isError,
      data,
      mutation.isPending,
      mutation.variables,
    ],
  );

  const { mutate } = mutation;
  const switchAccount = useCallback(
    (provider: Provider) => mutate(provider),
    [mutate],
  );
  return { state, switchAccount };
}
