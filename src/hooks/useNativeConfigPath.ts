import { useQuery } from "@tanstack/react-query";
import { settingsApi } from "@/lib/api";
import type { AppId } from "@/lib/api/types";
import { shortenHomePath } from "@/components/sessions/utils";

/**
 * 供应商编辑器标题里显示的配置文件真实路径：跟随目录覆盖和应用自己的环境变量
 * （如 KIMI_CODE_HOME）。目录还没读到时只显示文件名。
 */
export function useNativeConfigPath(appId: AppId, file: string) {
  const { data: dir } = useQuery({
    queryKey: ["nativeConfigDir", appId],
    queryFn: () => settingsApi.getConfigDir(appId),
    staleTime: 60_000,
  });
  if (!dir) return file;
  const short = shortenHomePath(dir);
  const sep = short.includes("\\") && !short.includes("/") ? "\\" : "/";
  return `${short.replace(/[\\/]+$/, "")}${sep}${file}`;
}
