import { useTranslation } from "react-i18next";
import type { CopilotByokImportWarning } from "@/lib/api/copilotByok";
import { CopilotErrorDetails } from "./CopilotErrorMessage";

export function CopilotImportWarnings({
  warnings,
}: {
  warnings: Array<CopilotByokImportWarning | string>;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-2">
      {warnings.map((warning, index) => {
        if (typeof warning === "string") {
          // Older backends returned diagnostics without a translatable code.
          return (
            <div key={index}>
              <p>{t("copilotByok.warnings.legacy")}</p>
              <CopilotErrorDetails details={warning} />
            </div>
          );
        }
        if (warning.code === "secretReference") {
          return (
            <p key={index}>
              {t("copilotByok.warnings.secretReference", {
                provider: warning.groupName,
              })}
            </p>
          );
        }
        return (
          <div key={index}>
            <p>
              {t("copilotByok.warnings.skippedGroup", {
                provider:
                  warning.groupName ||
                  t("copilotByok.warnings.unnamedGroup", {
                    index: warning.groupIndex,
                  }),
              })}
            </p>
            <CopilotErrorDetails details={warning.detail} />
          </div>
        );
      })}
    </div>
  );
}
