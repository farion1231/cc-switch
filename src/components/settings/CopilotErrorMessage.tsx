import { useTranslation } from "react-i18next";
import type { CopilotUiError } from "@/lib/copilotByokMessages";

export function CopilotErrorDetails({ details }: { details?: string }) {
  const { t } = useTranslation();
  if (!details) return null;
  return (
    <details className="mt-2 min-w-0 text-xs">
      <summary className="cursor-pointer">{t("copilotByok.details")}</summary>
      <p className="mt-1 whitespace-pre-wrap break-all font-mono">{details}</p>
    </details>
  );
}

export function CopilotErrorMessage({ error }: { error: CopilotUiError }) {
  const { t } = useTranslation();
  return (
    <div role="alert" className="text-sm text-destructive">
      <p>{t(error.messageKey)}</p>
      <CopilotErrorDetails details={error.details} />
    </div>
  );
}
