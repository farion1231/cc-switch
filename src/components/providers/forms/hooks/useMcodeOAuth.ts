import { useCallback, useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  mcodeOAuthApi,
  type McodeLoginStart,
  type McodeRegion,
} from "@/lib/api/mcodeOAuth";
import { deliverLoginLink } from "@/lib/authLoginLink";
import { extractErrorMessage } from "@/utils/errorUtils";
import { toast } from "@/lib/toast";

export const MCODE_OAUTH_STATUS_KEY = ["mcode-oauth-status"];

export function useMcodeOAuth() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const status = useQuery({
    queryKey: MCODE_OAUTH_STATUS_KEY,
    queryFn: mcodeOAuthApi.status,
    refetchInterval: 30_000,
  });
  const [flow, setFlow] = useState<McodeLoginStart | null>(null);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [retryable, setRetryable] = useState(false);
  const generation = useRef(0);
  const activeFlow = useRef<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const deadline = useRef(0);

  const invalidate = useCallback(async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: MCODE_OAUTH_STATUS_KEY }),
      queryClient.invalidateQueries({ queryKey: ["providers", "mcode"] }),
    ]);
  }, [queryClient]);

  const clearTimer = useCallback(() => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = null;
  }, []);

  const cancelBackend = useCallback(
    async (id: string) => {
      try {
        await mcodeOAuthApi.cancel(id);
      } catch (e) {
        console.debug("[McodeOAuth] Failed to cancel login:", e);
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
        setError(
          t("mcode.oauth.expired", { defaultValue: "登录已超时，请重试" }),
        );
        setRetryable(false);
        setFlow(null);
        activeFlow.current = null;
        void cancelBackend(id);
        return;
      }
      try {
        const result = await mcodeOAuthApi.poll(id);
        if (current !== generation.current) return;
        if (result.ended) {
          activeFlow.current = null;
          setFlow(null);
          setError(t(`mcode.oauth.${result.ended}`));
          setRetryable(false);
        } else if (result.authenticated) {
          activeFlow.current = null;
          setFlow(null);
          setError(null);
          setRetryable(false);
          await invalidate();
          void queryClient.invalidateQueries({
            queryKey: ["mcode_oauth", "quota"],
          });
          toast.success(
            t("mcode.oauth.success", {
              defaultValue: "MiniMax Code 账号登录成功",
            }),
          );
        } else {
          timer.current = setTimeout(
            () => void pollLogin(id, current),
            Math.max(1, result.interval) * 1000,
          );
        }
      } catch (e) {
        if (current !== generation.current) return;
        setError(extractErrorMessage(e));
        setRetryable(true);
      }
    },
    [cancelBackend, invalidate, queryClient, t],
  );

  const start = useCallback(
    async (region: McodeRegion, accountId?: string) => {
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
        const next = await mcodeOAuthApi.start(region, accountId);
        if (current !== generation.current) {
          void cancelBackend(next.flowId);
          return;
        }
        activeFlow.current = next.flowId;
        deadline.current = Date.now() + next.expiresIn * 1000;
        setFlow(next);
        setStarting(false);
        void deliverLoginLink(next.verificationUri, t);
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
    [cancelBackend, clearTimer, poll, t],
  );

  const retry = () => {
    const id = activeFlow.current;
    if (!id) return;
    setError(null);
    setRetryable(false);
    void poll(id, generation.current);
  };

  const remove = useMutation({
    mutationFn: (accountId: string) => mcodeOAuthApi.remove(accountId),
    onSuccess: async (_, accountId) => {
      queryClient.removeQueries({
        queryKey: ["mcode_oauth", "quota", accountId],
      });
      await invalidate();
    },
    onError: (e) => toast.error(extractErrorMessage(e)),
  });

  const useAccount = useMutation({
    mutationFn: (accountId: string) => mcodeOAuthApi.useAccount(accountId),
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
