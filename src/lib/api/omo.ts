import { invoke } from "@tauri-apps/api/core";
import type { OmoLocalFileData } from "@/types/omo";

export const omoApi = {
  readLocalFile: (): Promise<OmoLocalFileData> => invoke("read_omo_local_file"),
  getCurrentOmoProviderId: (): Promise<string> =>
    invoke("get_current_omo_provider_id"),
  disableCurrentOmo: (): Promise<void> => invoke("disable_current_omo"),
};

export const omoSlimApi = {
  readLocalFile: (): Promise<OmoLocalFileData> =>
    invoke("read_omo_slim_local_file"),
  getCurrentProviderId: (): Promise<string> =>
    invoke("get_current_omo_slim_provider_id"),
  disableCurrent: (): Promise<void> => invoke("disable_current_omo_slim"),
};
