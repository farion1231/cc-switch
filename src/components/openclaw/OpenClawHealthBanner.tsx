import React, { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { TriangleAlert } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import type { OpenClawHealthWarning } from "@/types";

interface OpenClawHealthBannerProps {
  warnings: OpenClawHealthWarning[];
}

function getWarningText(
  code: string,
  fallback: string,
  t: ReturnType<typeof useTranslation>["t"],
) {
  switch (code) {
    case "invalid_tools_profile":
      return t("openclaw.health.invalidToolsProfile", {
        defaultValue:
          "tools.profile contains an unsupported value. OpenClaw currently expects minimal, coding, messaging, or full.",
      });
    case "legacy_agents_timeout":
      return t("openclaw.health.legacyTimeout", {
        defaultValue:
          "agents.defaults.timeout is deprecated. Save the Agents panel to migrate it to timeoutSeconds.",
      });
    case "stringified_env_vars":
      return t("openclaw.health.stringifiedEnvVars", {
        defaultValue:
          "env.vars should be an object, but the current value looks stringified or malformed.",
      });
    case "stringified_env_shell_env":
      return t("openclaw.health.stringifiedShellEnv", {
        defaultValue:
          "env.shellEnv should be an object, but the current value looks stringified or malformed.",
      });
    case "config_parse_failed":
      return t("openclaw.health.parseFailed", {
        defaultValue:
          "openclaw.json could not be parsed as valid JSON5. Fix the file before editing it here.",
      });
    default:
      return fallback;
  }
}

const OpenClawHealthBanner: React.FC<OpenClawHealthBannerProps> = ({
  warnings,
}) => {
  const { t } = useTranslation();

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
        <AlertTitle>
          {t("openclaw.health.title", {
            defaultValue: "OpenClaw config warnings detected",
          })}
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

export default OpenClawHealthBanner;
