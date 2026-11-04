import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  dimOAuthApi,
  type DimLoginFlow,
  type DimSite,
} from "@/lib/api/dim-oauth";
import { deliverLoginLink } from "@/lib/authLoginLink";

export function useDimOauth({ enabled = true }: { enabled?: boolean } = {}) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const status = useQuery({
    queryKey: ["dim_oauth", "status"],
    queryFn: dimOAuthApi.status,
    enabled,
  });
  const [flow, setFlow] = useState<DimLoginFlow | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pollingState, setPollingState] = useState<
    "idle" | "polling" | "success" | "error"
  >("idle");
  const mounted = useRef(true);
  const attempt = useRef(0);
  const pendingFlow = useRef<string | null>(null);
  const refresh = () => client.invalidateQueries({ queryKey: ["dim_oauth"] });
  const start = useMutation({
    mutationFn: async (site: DimSite) => {
      const currentAttempt = ++attempt.current;
      setError(null);
      setPollingState("idle");
      const result = await dimOAuthApi.start(site);
      if (!mounted.current || currentAttempt !== attempt.current) {
        await dimOAuthApi.cancel(result.flow_id);
        return;
      }
      pendingFlow.current = result.flow_id;
      setFlow(result);
      setPollingState("polling");
      await deliverLoginLink(result.auth_url, t);
    },
    onError: (reason) => {
      setError(String(reason));
      setPollingState("error");
    },
  });
  const remove = useMutation({
    mutationFn: dimOAuthApi.remove,
    onSuccess: refresh,
  });
  const setDefault = useMutation({
    mutationFn: dimOAuthApi.setDefault,
    onSuccess: refresh,
  });
  useEffect(() => {
    if (!flow) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const deadline = Date.now() + flow.expires_in * 1000;
    const poll = async () => {
      try {
        if (Date.now() >= deadline) throw new Error(t("dimOauth.loginExpired"));
        const account = await dimOAuthApi.poll(flow.flow_id);
        if (stopped) return;
        if (account) {
          pendingFlow.current = null;
          setFlow(null);
          setPollingState("success");
          await client.invalidateQueries({ queryKey: ["dim_oauth"] });
        } else timer = setTimeout(poll, flow.interval * 1000);
      } catch (reason) {
        if (stopped) return;
        pendingFlow.current = null;
        void dimOAuthApi.cancel(flow.flow_id).catch(() => {});
        setFlow(null);
        setError(String(reason));
        setPollingState("error");
      }
    };
    timer = setTimeout(poll, flow.interval * 1000);
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [flow, client, t]);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      attempt.current++;
      if (pendingFlow.current)
        void dimOAuthApi.cancel(pendingFlow.current).catch(() => {});
    };
  }, []);
  const cancel = () => {
    attempt.current++;
    const id = pendingFlow.current;
    pendingFlow.current = null;
    setFlow(null);
    setPollingState("idle");
    if (id)
      void dimOAuthApi.cancel(id).catch((reason) => setError(String(reason)));
  };
  return {
    status,
    accounts: status.data?.accounts ?? [],
    defaultAccountId: status.data?.default_account_id ?? null,
    start,
    remove,
    setDefault,
    flow,
    error,
    pollingState,
    cancel,
  };
}
