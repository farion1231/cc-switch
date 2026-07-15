import type { AppId } from "@/lib/api";
import type { Provider } from "@/types";

export const CODEX_OFFICIAL_PROVIDER_ID = "codex-official";

/** Keep the UI capability rule aligned with the Rust takeover policy. */
export function supportsOfficialProxyTakeover(
  appId: AppId,
  provider: Pick<Provider, "id" | "category">,
): boolean {
  return (
    appId === "codex" &&
    provider.id === CODEX_OFFICIAL_PROVIDER_ID &&
    provider.category === "official"
  );
}
