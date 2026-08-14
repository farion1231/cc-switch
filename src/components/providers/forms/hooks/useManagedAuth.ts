import { useState, useCallback, useRef, useEffect } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { authApi, settingsApi } from "@/lib/api";
import { copyText } from "@/lib/clipboard";
import type {
  ManagedAuthProvider,
  ManagedAuthStatus,
  ManagedAuthDeviceCodeResponse,
} from "@/lib/api";

type PollingState = "idle" | "polling" | "success" | "error";

const HOT_PATH_STATUS_REFETCH_INTERVAL_MS = 15_000;

/**
 * Returns the status refresh cadence for providers whose proxy path can mark
 * an account as requiring login without a foreground Auth Center action.
 */
export function managedAuthStatusRefetchInterval(
  authProvider: ManagedAuthProvider,
): number | false {
  return authProvider === "xai_oauth" || authProvider === "kimi_oauth"
    ? HOT_PATH_STATUS_REFETCH_INTERVAL_MS
    : false;
}

export function useManagedAuth(
  authProvider: ManagedAuthProvider,
  githubDomain?: string,
) {
  const queryClient = useQueryClient();
  const queryKey = ["managed-auth-status", authProvider];

  const [pollingState, setPollingState] = useState<PollingState>("idle");
  const [deviceCode, setDeviceCode] =
    useState<ManagedAuthDeviceCodeResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  const pollingIntervalRef = useRef<ReturnType<typeof setInterval> | null>(
    null,
  );
  const pollingTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const authAttemptGenerationRef = useRef(0);
  const pollingRequestInFlightRef = useRef<number | null>(null);

  const {
    data: authStatus,
    isLoading: isLoadingStatus,
    refetch: refetchStatus,
  } = useQuery<ManagedAuthStatus>({
    queryKey,
    queryFn: () => authApi.authGetStatus(authProvider),
    staleTime: 30000,
    // A rejected refresh token can be persisted by a provider's proxy hot path.
    // Refresh local status so an open Auth Center reflects that transition.
    refetchInterval: managedAuthStatusRefetchInterval(authProvider),
  });

  const stopPolling = useCallback(() => {
    if (pollingIntervalRef.current) {
      clearInterval(pollingIntervalRef.current);
      pollingIntervalRef.current = null;
    }
    if (pollingTimeoutRef.current) {
      clearTimeout(pollingTimeoutRef.current);
      pollingTimeoutRef.current = null;
    }
  }, []);

  const invalidateAuthAttempt = useCallback(() => {
    stopPolling();
    pollingRequestInFlightRef.current = null;
    authAttemptGenerationRef.current += 1;
    return authAttemptGenerationRef.current;
  }, [stopPolling]);

  useEffect(() => {
    return () => {
      invalidateAuthAttempt();
    };
  }, [invalidateAuthAttempt]);

  const startLoginMutation = useMutation({
    mutationFn: (_attemptGeneration: number) =>
      authApi.authStartLogin(authProvider, githubDomain),
    onSuccess: async (response, attemptGeneration) => {
      if (attemptGeneration !== authAttemptGenerationRef.current) {
        return;
      }

      setDeviceCode(response);
      setPollingState("polling");
      setError(null);

      try {
        await copyText(response.user_code);
      } catch (e) {
        console.debug("[ManagedAuth] Failed to copy user code:", e);
      }
      if (attemptGeneration !== authAttemptGenerationRef.current) {
        return;
      }

      try {
        await settingsApi.openExternal(response.verification_uri);
      } catch (e) {
        console.debug("[ManagedAuth] Failed to open browser:", e);
      }
      if (attemptGeneration !== authAttemptGenerationRef.current) {
        return;
      }

      // Add a small buffer on top of GitHub's suggested interval to avoid
      // hitting slow_down responses too aggressively during device polling.
      const interval = Math.max((response.interval || 5) + 3, 8) * 1000;
      const expiresAt = Date.now() + response.expires_in * 1000;

      const pollOnce = async () => {
        if (
          attemptGeneration !== authAttemptGenerationRef.current ||
          pollingRequestInFlightRef.current === attemptGeneration
        ) {
          return;
        }
        pollingRequestInFlightRef.current = attemptGeneration;

        try {
          if (Date.now() > expiresAt) {
            invalidateAuthAttempt();
            setPollingState("error");
            setError("Device code expired. Please try again.");
            return;
          }

          const newAccount = await authApi.authPollForAccount(
            authProvider,
            response.device_code,
            githubDomain,
          );
          if (attemptGeneration !== authAttemptGenerationRef.current) {
            return;
          }

          if (newAccount) {
            stopPolling();
            setPollingState("success");
            await refetchStatus();
            if (attemptGeneration !== authAttemptGenerationRef.current) {
              return;
            }
            await queryClient.invalidateQueries({ queryKey });
            if (attemptGeneration !== authAttemptGenerationRef.current) {
              return;
            }
            setPollingState("idle");
            setDeviceCode(null);
          }
        } catch (e) {
          if (attemptGeneration !== authAttemptGenerationRef.current) {
            return;
          }

          const errorMessage = e instanceof Error ? e.message : String(e);
          if (
            !errorMessage.includes("pending") &&
            !errorMessage.includes("slow_down")
          ) {
            stopPolling();
            setPollingState("error");
            setError(errorMessage);
          }
        } finally {
          if (pollingRequestInFlightRef.current === attemptGeneration) {
            pollingRequestInFlightRef.current = null;
          }
        }
      };

      void pollOnce();
      pollingIntervalRef.current = setInterval(pollOnce, interval);
      pollingTimeoutRef.current = setTimeout(() => {
        if (attemptGeneration !== authAttemptGenerationRef.current) {
          return;
        }

        invalidateAuthAttempt();
        setPollingState("error");
        setError("Device code expired. Please try again.");
      }, response.expires_in * 1000);
    },
    onError: (e, attemptGeneration) => {
      if (attemptGeneration !== authAttemptGenerationRef.current) {
        return;
      }

      setPollingState("error");
      setError(e instanceof Error ? e.message : String(e));
    },
  });

  const logoutMutation = useMutation({
    mutationFn: () => authApi.authLogout(authProvider),
    onSuccess: async () => {
      setPollingState("idle");
      setDeviceCode(null);
      setError(null);
      queryClient.setQueryData(queryKey, {
        provider: authProvider,
        authenticated: false,
        default_account_id: null,
        accounts: [],
      });
      await queryClient.invalidateQueries({ queryKey });
    },
    onError: async (e) => {
      console.error("[ManagedAuth] Failed to logout:", e);
      setError(e instanceof Error ? e.message : String(e));
      await refetchStatus();
    },
  });

  const removeAccountMutation = useMutation({
    mutationFn: (accountId: string) =>
      authApi.authRemoveAccount(authProvider, accountId),
    onSuccess: async () => {
      setPollingState("idle");
      setDeviceCode(null);
      setError(null);
      await refetchStatus();
      await queryClient.invalidateQueries({ queryKey });
    },
    onError: (e) => {
      console.error("[ManagedAuth] Failed to remove account:", e);
      setError(e instanceof Error ? e.message : String(e));
    },
  });

  const setDefaultAccountMutation = useMutation({
    mutationFn: (accountId: string) =>
      authApi.authSetDefaultAccount(authProvider, accountId),
    onSuccess: async () => {
      await refetchStatus();
      await queryClient.invalidateQueries({ queryKey });
    },
    onError: (e) => {
      console.error("[ManagedAuth] Failed to set default account:", e);
      setError(e instanceof Error ? e.message : String(e));
    },
  });

  const startAuth = useCallback(() => {
    const attemptGeneration = invalidateAuthAttempt();
    setPollingState("idle");
    setDeviceCode(null);
    setError(null);
    startLoginMutation.mutate(attemptGeneration);
  }, [invalidateAuthAttempt, startLoginMutation]);

  const cancelAuth = useCallback(() => {
    invalidateAuthAttempt();
    setPollingState("idle");
    setDeviceCode(null);
    setError(null);
  }, [invalidateAuthAttempt]);

  const logout = useCallback(() => {
    logoutMutation.mutate();
  }, [logoutMutation]);

  const removeAccount = useCallback(
    (accountId: string) => {
      removeAccountMutation.mutate(accountId);
    },
    [removeAccountMutation],
  );

  const setDefaultAccount = useCallback(
    (accountId: string) => {
      setDefaultAccountMutation.mutate(accountId);
    },
    [setDefaultAccountMutation],
  );

  const accounts = authStatus?.accounts ?? [];

  return {
    authStatus,
    isLoadingStatus,
    accounts,
    hasAnyAccount: accounts.length > 0,
    isAuthenticated: authStatus?.authenticated ?? false,
    defaultAccountId: authStatus?.default_account_id ?? null,
    migrationError: authStatus?.migration_error ?? null,
    pollingState,
    deviceCode,
    error,
    isPolling: pollingState === "polling",
    isAddingAccount: startLoginMutation.isPending || pollingState === "polling",
    isRemovingAccount: removeAccountMutation.isPending,
    isSettingDefaultAccount: setDefaultAccountMutation.isPending,
    startAuth,
    addAccount: startAuth,
    cancelAuth,
    logout,
    removeAccount,
    setDefaultAccount,
    refetchStatus,
  };
}
