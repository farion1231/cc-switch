import type { QueryClient } from "@tanstack/react-query";
import { openclawKeys } from "@/hooks/useOpenClaw";
import { hermesKeys } from "@/hooks/useHermes";
import { invalidateZcodeProviderCaches } from "@/hooks/useZcode";
import { appHas } from "@/config/appSpecs";
import type { AppId } from "@/lib/api/types";
import { invalidatePiProviderCaches } from "./pi";

/**
 * 累加式应用的「已添加」和「默认」都现读客户端文件：往客户端文件里加了、删了供应商
 * （列表上的添加 / 移除、深链接导入）之后都要重读，否则卡片会停在旧的分组里。
 */
export async function refreshAdditiveLiveState(
  queryClient: QueryClient,
  app: AppId,
): Promise<void> {
  if (appHas(app, "piFamily")) {
    await invalidatePiProviderCaches(queryClient, app);
  } else if (app === "opencode") {
    await Promise.all([
      queryClient.invalidateQueries({
        queryKey: ["opencodeLiveProviderIds"],
      }),
      queryClient.invalidateQueries({ queryKey: ["opencodeDefaultModel"] }),
    ]);
  } else if (app === "openclaw") {
    await Promise.all([
      queryClient.invalidateQueries({
        queryKey: openclawKeys.liveProviderIds,
      }),
      queryClient.invalidateQueries({ queryKey: openclawKeys.defaultModel }),
      queryClient.invalidateQueries({ queryKey: openclawKeys.health }),
    ]);
  } else if (app === "hermes") {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: hermesKeys.liveProviderIds }),
      queryClient.invalidateQueries({ queryKey: hermesKeys.modelConfig }),
    ]);
  } else if (app === "zcode") {
    await invalidateZcodeProviderCaches(queryClient);
  } else if (
    app === "mcode" ||
    app === "dsh" ||
    app === "kimi" ||
    app === "copilot" ||
    app === "workbuddy" ||
    app === "codebuddy"
  ) {
    await queryClient.invalidateQueries({ queryKey: ["providers", app] });
  }
}
