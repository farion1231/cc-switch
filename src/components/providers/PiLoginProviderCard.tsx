import { Check } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ProviderIconBox } from "@/components/ProviderIconBox";
import type { PiLoginProvider } from "@/lib/api/pi";
import { resolveProviderIcon } from "@/utils/providerIcon";

/**
 * Runtime-only projection of Pi's auth.json. This is intentionally not a
 * Provider, has no sortable handle, and exposes no CC Switch write actions.
 */
export function PiLoginProviderCard({
  provider,
}: {
  provider: PiLoginProvider;
}) {
  const { t } = useTranslation();
  const managedHint = t("pi.login.managedHint");

  return (
    <div
      className="relative rounded-panel border border-border bg-surface"
      data-testid={`pi-login-provider-${provider.id}`}
    >
      <div className="flex min-h-[58px] items-center gap-3 py-2.5 pe-3 ps-2">
        <span className="w-4 shrink-0" />
        <ProviderIconBox
          icon={resolveProviderIcon("pi", provider.id)}
          name={provider.name}
        />
        <div className="min-w-0 flex-1">
          <h3
            className="m-0 truncate text-strong text-fg-1"
            title={provider.name}
          >
            {provider.name}
          </h3>
          <div className="mt-0.5 truncate text-caption text-fg-2">
            {managedHint}
          </div>
        </div>
        <span
          className="flex shrink-0 items-center gap-1.5 text-caption text-fg-2"
          title={managedHint}
        >
          <Check className="h-3.5 w-3.5" aria-hidden="true" />
          {t("pi.login.configured")}
        </span>
      </div>
    </div>
  );
}
