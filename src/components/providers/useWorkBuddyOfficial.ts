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
 * 官方卡的「添加」（= 切号）：沿用授权中心的通用切号命令（还原绑定账号的整组登录文件，要求 WorkBuddy
 * 已退出、凭据锁不新鲜，出错回滚）。WorkBuddy 在运行时先拦下来提示退出。
 */
export function useWorkBuddyOfficial(enabled: boolean) {
  const { t } = useTranslation();
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

  const switchTo = useCallback(
    async (provider: Provider) => {
      const accountId = boundWorkBuddyAccountId(provider);
      if (!accountId || busy.current) return;
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
        toast.success(
          t("workbuddy.official.switched", { name: provider.name }),
          {
            duration: 8000,
            closeButton: true,
            action: {
              label: t("authCenter.workbuddy.openApp"),
              onClick: () => void openWorkBuddyApp(),
            },
          },
        );
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
      switching: isPending,
    }),
    [status, running, isPending],
  );
  return { state, switchTo };
}
