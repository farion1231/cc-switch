import { useManagedAuth } from "./useManagedAuth";

/** xAI OAuth device-code authentication hook. */
export function useXaiOauth() {
  return useManagedAuth("xai_oauth");
}
