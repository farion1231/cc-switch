import { useCallback, useEffect, useMemo, useRef } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { authApi } from "@/lib/api";
import { extractErrorMessage } from "@/utils/errorUtils";
import { WORKBUDDY_ACCOUNT_AUTH_PROVIDER } from "@/config/workbuddyProviderPresets";
import type { Provider } from "@/types";
import {
  WORKBUDDY_LIVE_KEY,
  WORKBUDDY_STATUS_KEY,
  boundWorkBuddyAccountId,
  type WorkBuddyOfficialState,
} from "./workbuddyOfficial";
import { WORKBUDDY_QUOTA_KEY } from "./workbuddyQuota";

export const openWorkBuddyApp = () =>
  authApi.workbuddyAccountOpenApp().catch((error) => {
    toast.error(extractErrorMessage(error) || String(error));
  });

/**
 * 把 WorkBuddy 的登录切到某个已收录账号：沿用授权中心的通用切号命令（还原该账号的整组登录文件，
 * 要求 WorkBuddy 已退出、凭据锁不新鲜，出错回滚）。WorkBuddy 在运行时先拦下来提示退出。
 * 供应商列表的官方卡「添加」和授权中心账号的「切换 WorkBuddy 到这个账号」共用这一条路径；
 * 官方卡是否「已添加」按当前登录的账号现算，所以从哪边切，两边都会跟着变。
 */
export function useWorkBuddySwitch() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const refresh = useCallback(
    () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: WORKBUDDY_STATUS_KEY }),
        queryClient.invalidateQueries({ queryKey: WORKBUDDY_LIVE_KEY }),
        // 切号后「当前账号」变了：额度要按新的当前账号重新判断
        queryClient.invalidateQueries({ queryKey: WORKBUDDY_QUOTA_KEY }),
      ]),
    [queryClient],
  );

  const { mutateAsync, isPending } = useMutation({
    mutationFn: (accountId: string) =>
      authApi.authSetDefaultAccount(WORKBUDDY_ACCOUNT_AUTH_PROVIDER, accountId),
  });
  // 连点时只切一次
  const busy = useRef(false);

  /** `name` 是成功提示里的名字：官方卡用卡名，授权中心用账号名 */
  const switchToAccount = useCallback(
    async (accountId: string, name: string) => {
      if (busy.current) return;
      busy.current = true;
      const state = await authApi.workbuddyAccountLiveState().catch(() => null);
      if (state?.running) {
        toast.error(t("workbuddy.official.quitFirst"), {
          description: t("workbuddy.official.quitFirstBody"),
          closeButton: true,
        });
        busy.current = false;
        await refresh();
        return;
      }
      try {
        await mutateAsync(accountId);
        toast.success(t("workbuddy.official.switched", { name }), {
          duration: 8000,
          closeButton: true,
          action: {
            label: t("authCenter.workbuddy.openApp"),
            onClick: () => void openWorkBuddyApp(),
          },
        });
      } catch (error) {
        toast.error(t("workbuddy.official.switchFailed"), {
          description: extractErrorMessage(error) || String(error),
          closeButton: true,
        });
      } finally {
        busy.current = false;
        await refresh();
      }
    },
    [mutateAsync, refresh, t],
  );

  return { switchToAccount, switching: isPending };
}

/** 供应商列表上的官方卡：读账号与运行状态，「添加」= 切到卡上绑定的账号。 */
export function useWorkBuddyOfficial(enabled: boolean) {
  const queryClient = useQueryClient();
  // 读状态时后端会顺手收录 WorkBuddy 当前登录的号，轮询让「当前在用」跟得上
  const { data: status } = useQuery({
    queryKey: WORKBUDDY_STATUS_KEY,
    queryFn: () => authApi.authGetStatus(WORKBUDDY_ACCOUNT_AUTH_PROVIDER),
    enabled,
    staleTime: 30_000,
    refetchInterval: enabled ? 5_000 : false,
  });
  const { data: live } = useQuery({
    queryKey: WORKBUDDY_LIVE_KEY,
    queryFn: authApi.workbuddyAccountLiveState,
    enabled,
    refetchInterval: enabled ? 5_000 : false,
  });
  const { switchToAccount, switching } = useWorkBuddySwitch();

  const switchTo = useCallback(
    async (provider: Provider) => {
      const accountId = boundWorkBuddyAccountId(provider);
      if (accountId) await switchToAccount(accountId, provider.name);
    },
    [switchToAccount],
  );

  const running = live?.running ?? false;
  // WorkBuddy 刚打开时当前账号的额度就查得到了：别等 5 分钟的轮询
  const wasRunning = useRef(running);
  useEffect(() => {
    if (running && !wasRunning.current) {
      void queryClient.invalidateQueries({ queryKey: WORKBUDDY_QUOTA_KEY });
    }
    wasRunning.current = running;
  }, [running, queryClient]);
  const state = useMemo<WorkBuddyOfficialState>(
    () => ({
      accountIds: status
        ? new Set(status.accounts.map((account) => account.id))
        : null,
      currentAccountId: status?.default_account_id ?? null,
      running,
      switching,
    }),
    [status, running, switching],
  );
  return { state, switchTo };
}
