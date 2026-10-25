import { useCallback, useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  kimiOAuthApi,
  type KimiLoginStart,
  type KimiRegion,
} from "@/lib/api/kimiOAuth";
import { settingsApi } from "@/lib/api";
import { extractErrorMessage } from "@/utils/errorUtils";
import { toast } from "@/lib/toast";

export const KIMI_OAUTH_STATUS_KEY = ["kimi-oauth-status"];
const STATUS_KEY = KIMI_OAUTH_STATUS_KEY;

export function useKimiOAuth() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const status = useQuery({
    queryKey: STATUS_KEY,
    queryFn: kimiOAuthApi.status,
    refetchInterval: 30_000,
  });
  const [flow, setFlow] = useState<KimiLoginStart | null>(null);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [retryable, setRetryable] = useState(false);
  const generation = useRef(0);
  const activeFlow = useRef<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const deadline = useRef(0);

  const invalidate = useCallback(async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: STATUS_KEY }),
      queryClient.invalidateQueries({ queryKey: ["providers", "kimi"] }),
    ]);
  }, [queryClient]);

  const clearTimer = useCallback(() => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = null;
  }, []);

  const cancelBackend = useCallback(
    async (id: string) => {
      try {
        await kimiOAuthApi.cancel(id);
      } catch (e) {
        console.debug("[KimiOAuth] Failed to cancel login:", e);
      }
      await invalidate();
    },
    [invalidate],
  );

  const cancel = useCallback(() => {
    generation.current += 1;
    clearTimer();
    const id = activeFlow.current;
    activeFlow.current = null;
    setFlow(null);
    setStarting(false);
    setError(null);
    setRetryable(false);
    if (id) void cancelBackend(id);
  }, [cancelBackend, clearTimer]);

  useEffect(
    () => () => {
      generation.current += 1;
      clearTimer();
      const id = activeFlow.current;
      activeFlow.current = null;
      if (id) void cancelBackend(id);
    },
    [cancelBackend, clearTimer],
  );

  const poll = useCallback(
    async function pollLogin(id: string, current: number) {
      if (current !== generation.current) return;
      if (Date.now() >= deadline.current) {
        setError(t("kimi.oauth.expired"));
        setRetryable(false);
        setFlow(null);
        activeFlow.current = null;
        void cancelBackend(id);
        return;
      }
      try {
        const result = await kimiOAuthApi.poll(id);
        if (current !== generation.current) return;
        if (result.ended) {
          activeFlow.current = null;
          setFlow(null);
          setError(t(`kimi.oauth.${result.ended}`));
          setRetryable(false);
        } else if (result.authenticated) {
          activeFlow.current = null;
          setFlow(null);
          setError(null);
          setRetryable(false);
          await invalidate();
          // 重新登录可能落回已有账号，它的额度要换新凭据重查
          void queryClient.invalidateQueries({
            queryKey: ["kimi_oauth", "quota"],
          });
          toast.success(t("kimi.oauth.success"));
        } else {
          timer.current = setTimeout(
            () => void pollLogin(id, current),
            Math.max(1, result.interval) * 1000,
          );
        }
      } catch (e) {
        if (current !== generation.current) return;
        // Retrying resumes this flow: a token may already have been issued while
        // model discovery or disk persistence failed. Do not redeem it twice.
        setError(extractErrorMessage(e));
        setRetryable(true);
      }
    },
    [cancelBackend, invalidate, queryClient, t],
  );

  const start = useCallback(
    async (region: KimiRegion, accountId?: string) => {
      const current = ++generation.current;
      clearTimer();
      const previous = activeFlow.current;
      activeFlow.current = null;
      setFlow(null);
      setError(null);
      setRetryable(false);
      setStarting(true);
      try {
        if (previous) await cancelBackend(previous);
        if (current !== generation.current) return;
        const next = await kimiOAuthApi.start(region, accountId);
        if (current !== generation.current) {
          void cancelBackend(next.flowId);
          return;
        }
        activeFlow.current = next.flowId;
        deadline.current = Date.now() + next.expiresIn * 1000;
        setFlow(next);
        setStarting(false);
        // Keep the manual link and code visible even when the browser cannot open.
        void settingsApi.openExternal(next.verificationUri).catch((e) => {
          console.debug("[KimiOAuth] Could not open browser:", e);
        });
        timer.current = setTimeout(
          () => void poll(next.flowId, current),
          Math.max(1, next.interval) * 1000,
        );
      } catch (e) {
        if (current !== generation.current) return;
        setStarting(false);
        setError(extractErrorMessage(e));
      }
    },
    [cancelBackend, clearTimer, poll],
  );

  const retry = () => {
    const id = activeFlow.current;
    if (!id) return;
    setError(null);
    setRetryable(false);
    void poll(id, generation.current);
  };

  // 分开存的账号连同卡片、模型和本机凭据一起删；Kimi 自己的登录只移出列表，不替它登出
  const remove = useMutation({
    mutationFn: (accountId: string) => kimiOAuthApi.remove(accountId),
    onSuccess: async (_, accountId) => {
      queryClient.removeQueries({
        queryKey: ["kimi_oauth", "quota", accountId],
      });
      await invalidate();
    },
    // 登录流程的错误才进 error（授权中心按「登录没有完成」显示），账号操作的失败弹 toast
    onError: (e) => toast.error(extractErrorMessage(e)),
  });

  const useAccount = useMutation({
    mutationFn: (accountId: string) => kimiOAuthApi.useAccount(accountId),
    onSuccess: invalidate,
    onError: (e) => toast.error(extractErrorMessage(e)),
  });

  return {
    status,
    flow,
    starting,
    error,
    retryable,
    start,
    cancel,
    retry,
    remove,
    useAccount,
  };
}
