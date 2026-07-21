import { useCallback, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { authApi } from "@/lib/api";
import { useManagedAuth } from "./useManagedAuth";

/**
 * Kiro (AWS CodeWhisperer/Q) 认证 hook
 *
 * 复用通用 useManagedAuth（设备码流程：Builder ID / IdC），
 * 并额外提供社交登录（Google / GitHub，PKCE + localhost 回调）。
 */
export function useKiroAuth() {
  const base = useManagedAuth("kiro");
  const queryClient = useQueryClient();

  const [isSocialLoggingIn, setIsSocialLoggingIn] = useState(false);
  const [socialError, setSocialError] = useState<string | null>(null);
  const [isImporting, setIsImporting] = useState(false);
  const [importError, setImportError] = useState<string | null>(null);
  const [isApiKeyLoggingIn, setIsApiKeyLoggingIn] = useState(false);
  const [apiKeyError, setApiKeyError] = useState<string | null>(null);

  const socialLogin = useCallback(
    async (provider?: "google" | "github") => {
      setSocialError(null);
      setIsSocialLoggingIn(true);
      try {
        await authApi.authKiroSocialLogin(provider);
        await base.refetchStatus();
        await queryClient.invalidateQueries({
          queryKey: ["managed-auth-status", "kiro"],
        });
      } catch (e) {
        setSocialError(e instanceof Error ? e.message : String(e));
      } finally {
        setIsSocialLoggingIn(false);
      }
    },
    [base, queryClient],
  );

  const importDynamic = useCallback(async () => {
    setImportError(null);
    setIsImporting(true);
    try {
      await authApi.authKiroImportDynamic();
      await base.refetchStatus();
      await queryClient.invalidateQueries({
        queryKey: ["managed-auth-status", "kiro"],
      });
    } catch (e) {
      setImportError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsImporting(false);
    }
  }, [base, queryClient]);

  const apiKeyLogin = useCallback(
    async (apiKey: string) => {
      setApiKeyError(null);
      setIsApiKeyLoggingIn(true);
      try {
        await authApi.authKiroApiKeyLogin(apiKey);
        await base.refetchStatus();
        await queryClient.invalidateQueries({
          queryKey: ["managed-auth-status", "kiro"],
        });
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        setApiKeyError(msg);
        throw e;
      } finally {
        setIsApiKeyLoggingIn(false);
      }
    },
    [base, queryClient],
  );

  return {
    ...base,
    socialLogin,
    isSocialLoggingIn,
    socialError,
    importDynamic,
    isImporting,
    importError,
    apiKeyLogin,
    isApiKeyLoggingIn,
    apiKeyError,
  };
}
