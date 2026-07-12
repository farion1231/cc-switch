import { useManagedAuth } from "./useManagedAuth";

/**
 * xAI Grok OAuth auth hook.
 *
 * Reuses generic managed auth with provider "xai_oauth".
 */
export function useXaiOauth() {
  return useManagedAuth("xai_oauth");
}
