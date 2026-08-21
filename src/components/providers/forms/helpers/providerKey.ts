/**
 * Provider keys are persisted as config object keys, so they are restricted to
 * a lowercase slug charset. Shared by the OpenCode / OpenClaw / Hermes editors
 * in ProviderForm and by the Pi editor.
 */
export function normalizeProviderKey(value: string): string {
  return value.toLowerCase().replace(/[^a-z0-9-]/g, "");
}
