import { invoke } from "@tauri-apps/api/core";
import type { OmoLocalFileData } from "@/types/omo";

export const omoApi = {
  readLocalFile: (): Promise<OmoLocalFileData> => invoke("read_omo_local_file"),
  getCurrentOmoProviderId: (): Promise<string> =>
    invoke("get_current_omo_provider_id"),
  getOmoProviderCount: (): Promise<number> => invoke("get_omo_provider_count"),
  disableCurrentOmo: (): Promise<void> => invoke("disable_current_omo"),
};
