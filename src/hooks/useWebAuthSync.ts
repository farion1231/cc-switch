import { useEffect, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { clearAuthToken } from "@/lib/api/web-client";
import { isTauri } from "@/lib/environment";

function syncAuthUrl(isAuthenticated: boolean) {
  if (isTauri()) return;

  const pathname = window.location.pathname;
  const target = isAuthenticated ? "/" : "/login";
  if (pathname === target) return;
  window.history.replaceState({}, "", target);
}

export function useWebAuthSync(
  isAuthenticated: boolean,
  setIsAuthenticated: (value: boolean) => void,
) {
  const queryClient = useQueryClient();
  const mountedRef = useRef(true);

  useEffect(() => {
    if (isTauri()) return;
    mountedRef.current = true;

    syncAuthUrl(isAuthenticated);

    const handleExpired = () => {
      if (!isAuthenticated) return;
      clearAuthToken();
      queryClient.clear();
      if (mountedRef.current) {
        setIsAuthenticated(false);
      }
      syncAuthUrl(false);
    };

    window.addEventListener("auth:expired", handleExpired);
    return () => {
      mountedRef.current = false;
      window.removeEventListener("auth:expired", handleExpired);
    };
  }, [isAuthenticated, queryClient, setIsAuthenticated]);
}
