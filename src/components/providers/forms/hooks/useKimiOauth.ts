/** @fileoverview Binds Kimi OAuth UI state to the shared managed-auth workflow. */

import { useManagedAuth } from "./useManagedAuth";

/** Kimi OAuth device-code authentication hook. */
export function useKimiOauth() {
  return useManagedAuth("kimi_oauth");
}
