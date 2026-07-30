import { ArrowRightLeft, CheckCircle2, Circle } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ProviderSwitchPreview } from "@/lib/api/deeplink";

export function ProviderSwitchConfirmation({
  preview,
}: {
  preview: ProviderSwitchPreview;
}) {
  const { t } = useTranslation();
  const StatusIcon = preview.isCurrent ? CheckCircle2 : Circle;

  return (
    <div className="space-y-4">
      <div className="flex items-start gap-3 border-y border-border-default py-4">
        <ArrowRightLeft
          className="mt-0.5 h-5 w-5 shrink-0 text-blue-600 dark:text-blue-400"
          aria-hidden="true"
        />
        <div className="min-w-0 space-y-1">
          <div className="break-words text-sm font-semibold text-foreground">
            {preview.name}
          </div>
          <div className="break-all font-mono text-xs text-muted-foreground">
            {preview.hostname}
          </div>
        </div>
      </div>
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <StatusIcon
          className={preview.isCurrent ? "h-4 w-4 text-green-600" : "h-4 w-4"}
          aria-hidden="true"
        />
        <span>
          {preview.isCurrent
            ? t("deeplink.providerSwitch.current")
            : t("deeplink.providerSwitch.notCurrent")}
        </span>
      </div>
    </div>
  );
}
