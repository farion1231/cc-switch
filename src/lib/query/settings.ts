import { useQuery, type UseQueryResult } from "@tanstack/react-query";
import { settingsApi } from "@/lib/api/settings";
import type { Settings } from "@/types";

export const useSettingsQuery = (): UseQueryResult<Settings> => {
  return useQuery({
    queryKey: ["settings"],
    queryFn: async () => settingsApi.get(),
  });
};
