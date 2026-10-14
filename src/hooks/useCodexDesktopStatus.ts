import { useQuery } from "@tanstack/react-query";
import { getCodexDesktopStatus } from "@/lib/api/codexDesktop";
import { isWindows } from "@/lib/platform";

export function useCodexDesktopStatus(enabled = true) {
  return useQuery({
    queryKey: ["codex-desktop-status"],
    queryFn: getCodexDesktopStatus,
    enabled: enabled && isWindows(),
    staleTime: 5_000,
    refetchInterval: 15_000,
    refetchOnWindowFocus: true,
    retry: false,
  });
}
