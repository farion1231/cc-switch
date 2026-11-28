import { useCallback, useMemo, useRef } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { authApi, type AgyRunningState } from "@/lib/api";
import {
  canSwitchAgyAccount,
  resolveManagedAccountId,
} from "@/lib/authBinding";
import {
  AGY_RUNNING_STATE_KEY,
  agyAuthStatusQuery,
  invalidateAgyLoginCaches,
} from "@/lib/query/subscription";
import { extractErrorMessage } from "@/utils/errorUtils";
import { isOfficialAccount } from "@/utils/providerCapabilities";
import type { Provider } from "@/types";
import type { LaunchAccountsInput } from "./presentation";

export function agyQuitFirstReason(
  t: TFunction,
  running: AgyRunningState | undefined,
): string | undefined {
  if (!running) return undefined;
  if (running.desktop && running.cli) {
    return t("agy.official.quitBothFirst", {
      defaultValue: "请先退出 Antigravity 桌面端和 agy CLI 再切换账号",
    });
  }
  if (running.desktop) {
    return t("agy.official.quitDesktopFirst", {
      defaultValue: "请先完全退出 Antigravity 桌面端再切换账号",
    });
  }
  if (running.cli) {
    return t("agy.official.quitCliFirst", {
      defaultValue: "请先退出正在运行的 agy CLI 再切换账号",
    });
  }
  return undefined;
}

export function agyQuitFirstBody(
  t: TFunction,
  running: AgyRunningState,
): string {
  if (running.desktop && running.cli) {
    return t("agy.official.quitBothFirstBody", {
      defaultValue:
        "完全退出 Antigravity 桌面端（含托盘/后台进程）以及运行中的 agy CLI 后再切换，否则它们不会刷新且可能把旧登录写回去。",
    });
  }
  if (running.desktop) {
    return t("agy.official.quitDesktopFirstBody", {
      defaultValue:
        "完全退出 Antigravity 桌面端（含托盘/后台进程）后再切换，否则它不会刷新且可能把旧登录写回去。",
    });
  }
  return t("agy.official.quitCliFirstBody", {
    defaultValue:
      "退出正在运行的 agy CLI 会话后再切换，否则已打开的会话仍用旧账号并可能把它写回去。",
  });
}

/**
 * Antigravity 账号切换共享 hook（供应商卡片和授权中心共用）：
 * 切换前实时检测 Antigravity 桌面端 / agy CLI 是否在运行；若在运行则拦截并提示退出，
 * 避免写入后桌面端不刷新或旧进程把原登录回写覆盖。
 */
export function useAgyAccountSwitch() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const busy = useRef(false);

  const { mutateAsync, isPending } = useMutation({
    mutationFn: (accountId: string) =>
      authApi.authSetDefaultAccount("agy_oauth", accountId),
  });

  const switchToAccount = useCallback(
    async (accountId: string, name: string) => {
      if (busy.current) return;
      busy.current = true;
      let attemptedSwitch = false;
      try {
        const state = await authApi.agyAccountRunningState().catch(() => null);
        if (state && (state.cli || state.desktop)) {
          toast.error(agyQuitFirstReason(t, state) ?? "", {
            description: agyQuitFirstBody(t, state),
            closeButton: true,
          });
          queryClient.setQueryData(AGY_RUNNING_STATE_KEY, state);
          return;
        }
        attemptedSwitch = true;
        await mutateAsync(accountId);
        toast.success(
          t("agy.official.switched", {
            name,
            defaultValue:
              "已切换到「{{name}}」，重新打开 Antigravity 即可使用该账号。",
          }),
          { closeButton: true },
        );
      } catch (error) {
        toast.error(
          t("agy.official.switchFailed", {
            defaultValue: "切换 Antigravity 账号失败",
          }),
          {
            description: extractErrorMessage(error) || String(error),
            closeButton: true,
          },
        );
      } finally {
        busy.current = false;
        if (attemptedSwitch) {
          await invalidateAgyLoginCaches(queryClient);
        }
      }
    },
    [mutateAsync, queryClient, t],
  );

  return { switchToAccount, switching: isPending };
}

/**
 * 供应商列表上绑定了授权中心账号的 Antigravity 官方卡：
 * 自动同步当前在用账号与 Antigravity 运行状态，支持直接在卡片上切换账号。
 */
export function useAgyOfficial(
  enabled: boolean,
  providers: Record<string, Provider>,
): LaunchAccountsInput | undefined {
  const { t } = useTranslation();

  const boundAccounts = useMemo(() => {
    const map = new Map<string, string>();
    if (!enabled || !canSwitchAgyAccount()) return map;
    for (const provider of Object.values(providers)) {
      if (!isOfficialAccount("agy", provider)) continue;
      const accountId = resolveManagedAccountId(
        provider.meta,
        "agy_oauth",
      )?.trim();
      if (accountId) {
        map.set(provider.id, accountId);
      }
    }
    return map;
  }, [enabled, providers]);

  const active = boundAccounts.size > 0;
  const {
    data: rawAuthStatus,
    isSuccess: isAuthStatusSuccess,
    isError: isAuthStatusError,
  } = useQuery({
    ...agyAuthStatusQuery,
    enabled: active,
    refetchInterval: active ? 15_000 : false,
  });
  const { data: rawRunningState } = useQuery({
    queryKey: AGY_RUNNING_STATE_KEY,
    queryFn: () =>
      authApi
        .agyAccountRunningState()
        .catch(() => ({ cli: false, desktop: false })),
    enabled: active,
    staleTime: 0,
    refetchInterval: active ? 5_000 : false,
  });
  const authStatus = active ? rawAuthStatus : undefined;
  const runningState = active ? rawRunningState : undefined;
  const authLoadStatus: "loading" | "error" | "ready" = !active
    ? "loading"
    : isAuthStatusError
      ? "error"
      : isAuthStatusSuccess
        ? "ready"
        : "loading";
  const { switchToAccount, switching } = useAgyAccountSwitch();

  return useMemo<LaunchAccountsInput | undefined>(() => {
    if (!active) return undefined;
    const currentProviderIds = new Set<string>();
    const switchableProviderIds = new Set<string>();
    const missingProviderIds = new Set<string>();
    const requiresReauth = new Set<string>();
    const accountsById = new Map(
      (authStatus?.accounts ?? []).map((account) => [account.id, account]),
    );
    const activeAccountId = authStatus?.default_account_id ?? null;

    for (const [providerId, accountId] of boundAccounts) {
      switchableProviderIds.add(providerId);
      const account = accountsById.get(accountId);
      if (authStatus && !account) {
        missingProviderIds.add(providerId);
        continue;
      }
      if (account?.requires_reauth) {
        requiresReauth.add(providerId);
      }
      if (account && account.id === activeAccountId) {
        currentProviderIds.add(providerId);
      }
    }

    return {
      status: authLoadStatus,
      currentProviderIds,
      switchableProviderIds,
      missingProviderIds,
      requiresReauth,
      runningReason: agyQuitFirstReason(t, runningState),
      switching,
      onSwitch: (provider) => {
        const accountId = boundAccounts.get(provider.id);
        if (accountId) {
          void switchToAccount(accountId, provider.name);
        }
      },
    };
  }, [
    active,
    authLoadStatus,
    authStatus,
    boundAccounts,
    runningState,
    switchToAccount,
    switching,
    t,
  ]);
}
