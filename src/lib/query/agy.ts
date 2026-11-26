import { useCallback, useEffect, useMemo, useRef } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { authApi } from "@/lib/api";
import {
  canSwitchAgyAccount,
  resolveManagedAccountId,
} from "@/lib/authBinding";
import {
  agyAuthStatusQuery,
  invalidateAgyLoginCaches,
} from "@/lib/query/subscription";
import { toast } from "@/lib/toast";
import type { Provider } from "@/types";
import type { AgyAccountsState } from "@/components/providers/presentation";

/**
 * Antigravity 官方登录卡的状态与一键切号（仿照 `useCodebuddyOfficialCards`）：
 * 读授权中心里的 Antigravity 账号列表和当前在用的号；点击卡片上的「切换」把 agy 的全局登录
 * 切到该卡绑定的账号。列表变化时自动刷新 Antigravity 供应商列表，新收录的号无需手动刷新即成卡。
 */
export function useAgyOfficialCards(enabled: boolean): {
  state: AgyAccountsState;
  switchAccount: (provider: Provider) => void;
} {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const active = enabled && canSwitchAgyAccount();
  const status = useQuery({
    ...agyAuthStatusQuery,
    enabled: active,
  });

  const prevAccountIdsRef = useRef<string | null>(null);
  useEffect(() => {
    if (!active || !status.data) return;
    const signature = status.data.accounts
      .map((account) => account.id)
      .sort()
      .join("\n");
    if (
      prevAccountIdsRef.current !== null &&
      prevAccountIdsRef.current !== signature
    ) {
      void queryClient.invalidateQueries({ queryKey: ["providers", "agy"] });
    }
    prevAccountIdsRef.current = signature;
  }, [active, queryClient, status.data]);

  const mutation = useMutation({
    mutationFn: async (provider: Provider) => {
      const accountId = resolveManagedAccountId(
        provider.meta,
        "agy_oauth",
      )?.trim();
      if (!accountId) {
        throw new Error(
          t("provider.form.agy.accountMissing", {
            defaultValue: "不在这台电脑上的账号",
          }),
        );
      }
      await authApi.authSetDefaultAccount("agy_oauth", accountId);
    },
    onSuccess: async () => {
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
    },
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : String(error));
    },
    onSettled: async () => {
      await invalidateAgyLoginCaches(queryClient);
    },
  });

  const data = active ? status.data : undefined;
  const state = useMemo<AgyAccountsState>(
    () => ({
      status: !active
        ? "loading"
        : status.isSuccess
          ? "ready"
          : status.isError
            ? "error"
            : "loading",
      currentAccountId: data?.default_account_id ?? null,
      accounts: (data?.accounts ?? []).map((account) => ({
        id: account.id,
        requiresReauth: account.requires_reauth,
      })),
      switchingId: mutation.isPending ? mutation.variables?.id : null,
    }),
    [
      active,
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
