import React, { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { TriangleAlert } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import type { PiHealthWarning } from "@/types";

interface PiHealthBannerProps {
  warnings: PiHealthWarning[];
}

function getWarningText(
  code: string,
  provider: string | undefined,
  fallback: string,
  t: ReturnType<typeof useTranslation>["t"],
) {
  const name = provider?.trim();
  switch (code) {
    case "missing_base_url":
      return name
        ? t("pi.health.missingBaseUrlNamed", {
            name,
            defaultValue: "Provider '{{name}}' is missing 'baseUrl'.",
          })
        : t("pi.health.missingBaseUrl", {
            defaultValue:
              "Provider is missing 'baseUrl'. Pi requires a base URL to connect to the API.",
          });
    case "missing_api":
      return name
        ? t("pi.health.missingApiNamed", {
            name,
            defaultValue:
              "Provider '{{name}}' is missing 'api' (openai-completions / anthropic-messages / …).",
          })
        : t("pi.health.missingApi", {
            defaultValue:
              "Provider is missing 'api' field. Pi requires an API type (openai-completions, anthropic-messages, etc.).",
          });
    case "missing_models":
      return name
        ? t("pi.health.missingModelsNamed", {
            name,
            defaultValue: "Provider '{{name}}' has no models configured.",
          })
        : t("pi.health.missingModels", {
            defaultValue: "Provider has no models configured.",
          });
    default:
      return fallback;
  }
}

const PiHealthBanner: React.FC<PiHealthBannerProps> = ({
  warnings,
}) => {
  const { t } = useTranslation();

  const items = useMemo(
    () =>
      warnings.map((warning) => ({
        ...warning,
        text: getWarningText(
          warning.code,
          warning.provider,
          warning.message,
          t,
        ),
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
        <AlertTitle>
          {t("pi.health.title", {
            defaultValue: "Pi config warnings detected",
          })}
        </AlertTitle>
        <AlertDescription>
          <ul className="list-disc space-y-1 pl-5">
            {items.map((warning) => (
              <li
                key={`${warning.code}:${warning.provider ?? ""}:${warning.path ?? warning.message}`}
              >
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

export default PiHealthBanner;
