import React, { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { ExternalLink, TriangleAlert } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { useOpenHermesWebUI } from "@/hooks/useHermes";
import type { HermesHealthWarning } from "@/types";

interface HermesHealthBannerProps {
  warnings: HermesHealthWarning[];
}

function getWarningText(
  code: string,
  fallback: string,
  t: ReturnType<typeof useTranslation>["t"],
) {
  switch (code) {
    case "config_parse_failed":
      return t("hermes.health.parseFailed", {
        defaultValue:
          "config.yaml could not be parsed as valid YAML. Fix the file before editing it here.",
      });
    case "config_not_found":
      return t("hermes.health.configNotFound", {
        defaultValue:
          "Hermes config.yaml not found. Create it at ~/.hermes/config.yaml or configure the path in settings.",
      });
    case "env_parse_failed":
      return t("hermes.health.envParseFailed", {
        defaultValue: "The .env file could not be parsed.",
      });
    default:
      return fallback;
  }
}

const HermesHealthBanner: React.FC<HermesHealthBannerProps> = ({
  warnings,
}) => {
  const { t } = useTranslation();
  const openHermesWebUI = useOpenHermesWebUI();

  const items = useMemo(
    () =>
      warnings.map((warning) => ({
        ...warning,
        text: getWarningText(warning.code, warning.message, t),
      })),
    [t, warnings],
  );

  if (warnings.length === 0) {
    return null;
  }

  return (
    <div className="px-6 pt-4">
      <Alert className="border-amber-500/30 bg-amber-500/5">
        <TriangleAlert className="h-4 w-4" />
        <AlertTitle className="flex items-center justify-between gap-2">
          <span>
            {t("hermes.health.title", {
              defaultValue: "Hermes config warnings detected",
            })}
          </span>
          <Button
            variant="outline"
            size="sm"
            onClick={() => void openHermesWebUI("/config")}
            className="shrink-0"
          >
            <ExternalLink className="w-3.5 h-3.5 mr-1" />
            {t("hermes.webui.fixInWebUI")}
          </Button>
        </AlertTitle>
        <AlertDescription>
          <ul className="list-disc space-y-1 pl-5">
            {items.map((warning) => (
              <li key={`${warning.code}:${warning.path ?? warning.message}`}>
                {warning.text}
                {warning.path ? ` (${warning.path})` : ""}
              </li>
            ))}
          </ul>
        </AlertDescription>
      </Alert>
    </div>
  );
};

export default HermesHealthBanner;
