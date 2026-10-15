import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

/** Existing supplier settings grouped by upstream protocol; no new config. */
export function UpstreamSettingsGroup({
  protocol,
  enabled,
  children,
}: {
  protocol: "messages" | "chat" | "responses" | "shared";
  enabled: boolean;
  children?: ReactNode;
}) {
  const { t } = useTranslation();
  if (!enabled) return <>{children}</>;
  if (protocol === "shared")
    return (
      <details className="space-y-3 border-t border-border pt-3">
        <summary className="cursor-pointer text-sm font-medium">
          {t("providerForm.protocolSettings.shared")}
        </summary>
        <p className="text-xs text-fg-2">
          {t("providerForm.protocolSettings.sharedHint")}
        </p>
        {children}
      </details>
    );
  return (
    <fieldset className="space-y-3 rounded-md border border-border p-3">
      <legend className="px-1 text-sm font-medium">
        {t(`providerForm.protocolSettings.${protocol}`)}
      </legend>
      <p className="text-xs leading-relaxed text-fg-2">
        {t(`providerForm.protocolSettings.${protocol}Hint`)}
      </p>
      {children}
    </fieldset>
  );
}
