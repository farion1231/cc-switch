import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Notice } from "@/components/ui/notice";
import type { Provider } from "@/types";

const DISMISSED_KEY = "cc-switch.cline.cliModelNoticeDismissed";

/** Custom providers carry `models.json` metadata; built-in ones only have settings. */
export function hasClineCustomProvider(providers: Record<string, Provider>) {
  return Object.values(providers).some(
    (provider) =>
      typeof provider.settingsConfig?.provider === "object" &&
      provider.settingsConfig.provider !== null,
  );
}

function readDismissed() {
  try {
    return window.localStorage.getItem(DISMISSED_KEY) === "1";
  } catch {
    return false;
  }
}

/**
 * Cline CLI 3.x only fills the `/model` list from its built-in catalog, so a
 * custom provider shows no models there (the desktop app lists them). Explain
 * how to pick a model in the CLI; the choice to hide it stays on this device.
 */
export function ClineCliModelNotice({
  providers,
}: {
  providers: Record<string, Provider>;
}) {
  const { t } = useTranslation();
  const [dismissed, setDismissed] = useState(readDismissed);
  if (dismissed || !hasClineCustomProvider(providers)) return null;

  return (
    <Notice
      title={t("cline.cliModel.title")}
      onDismiss={() => {
        setDismissed(true);
        try {
          window.localStorage.setItem(DISMISSED_KEY, "1");
        } catch {
          // Private mode: hidden until the next launch.
        }
      }}
      dismissLabel={t("common.close")}
    >
      {t("cline.cliModel.body")}
    </Notice>
  );
}
