import { useEffect } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useQuery } from "@tanstack/react-query";
import { getCodexDesktopStatus } from "@/lib/api/codexDesktop";
import { isWindows } from "@/lib/platform";

export function useCodexDesktopStatus(enabled = true) {
  const active = enabled && isWindows();
  const query = useQuery({
    queryKey: ["codex-desktop-status"],
    queryFn: getCodexDesktopStatus,
    enabled: active,
    staleTime: 5_000,
    refetchOnWindowFocus: "always",
    retry: false,
  });
  const { refetch } = query;
  useEffect(() => {
    if (!active) return;
    const refresh = () => void refetch({ cancelRefetch: false });
    let disposed = false;
    let unlisten: (() => void) | undefined;
    window.addEventListener("focus", refresh);
    if (isTauri()) {
      void (async () => {
        const off = await getCurrentWindow().onFocusChanged(({ payload }) => {
          if (payload && !disposed) refresh();
        });
        if (disposed) off();
        else unlisten = off;
      })().catch((error) =>
        console.debug("Failed to observe Codex desktop status focus", error),
      );
    }
    return () => {
      disposed = true;
      window.removeEventListener("focus", refresh);
      unlisten?.();
    };
  }, [active, refetch]);
  return query;
}
