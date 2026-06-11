import { server } from "./server";

declare global {
  // eslint-disable-next-line no-var
  var __emitTauriEvent: ((event: string, payload: unknown) => void) | undefined;
}

export const emitTauriEvent = (event: string, payload: unknown) => {
  globalThis.__emitTauriEvent?.(event, payload);
};

// Ensure the MSW server is referenced so tree shaking doesn't remove imports
void server;
