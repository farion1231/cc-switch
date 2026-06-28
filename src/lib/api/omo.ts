import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "@/lib/environment";
import type { OmoLocalFileData } from "@/types/omo";

function ensureDesktop(feature: string) {
  if (!isTauri()) {
    throw new Error(`${feature} is only available in desktop mode`);
  }
}

export const omoApi = {
  readLocalFile: (): Promise<OmoLocalFileData> => {
    ensureDesktop("OMO local file import");
    return invoke("read_omo_local_file");
  },
  getCurrentOmoProviderId: (): Promise<string> =>
    isTauri() ? invoke("get_current_omo_provider_id") : Promise.resolve(""),
  disableCurrentOmo: (): Promise<void> =>
    isTauri() ? invoke("disable_current_omo") : Promise.resolve(),
};

export const omoSlimApi = {
  readLocalFile: (): Promise<OmoLocalFileData> => {
    ensureDesktop("OMO slim local file import");
    return invoke("read_omo_slim_local_file");
  },
  getCurrentProviderId: (): Promise<string> =>
    isTauri()
      ? invoke("get_current_omo_slim_provider_id")
      : Promise.resolve(""),
  disableCurrent: (): Promise<void> =>
    isTauri() ? invoke("disable_current_omo_slim") : Promise.resolve(),
};
