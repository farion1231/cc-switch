import { useTranslation } from "react-i18next";
import type { PiProviderPatchPreview } from "@/types/pi";
import { Button } from "@/components/ui/button";

interface PiProviderDiffPreviewProps {
  preview: PiProviderPatchPreview | null;
  isApplying?: boolean;
  onApply: () => void;
  onDelete?: () => void;
  canDelete?: boolean;
}

export function PiProviderDiffPreview({
  preview,
  isApplying,
  onApply,
  onDelete,
  canDelete,
}: PiProviderDiffPreviewProps) {
  const { t } = useTranslation();

  if (!preview) {
    return (
      <div className="rounded-lg border border-dashed border-border-default p-4 text-sm text-muted-foreground">
        {t("pi.review.empty")}
      </div>
    );
  }

  return (
    <div className="space-y-3 rounded-lg border border-border-default p-4">
      <div className="flex items-center justify-between gap-3">
        <div>
          <h3 className="text-sm font-semibold">{t("pi.review.title")}</h3>
          <p className="text-xs text-muted-foreground">
            {t("pi.review.fileHash", {
              hash: preview.currentFileHash || t("pi.review.newFile"),
            })}
          </p>
        </div>
        <div className="flex gap-2">
          {onDelete && (
            <Button
              type="button"
              variant="destructive"
              onClick={onDelete}
              disabled={isApplying || !canDelete}
            >
              {t("pi.review.delete")}
            </Button>
          )}
          <Button type="button" onClick={onApply} disabled={isApplying}>
            {isApplying ? t("pi.review.applying") : t("pi.review.apply")}
          </Button>
        </div>
      </div>
      <ul className="list-disc pl-5 text-sm">
        {preview.summary.map((item) => (
          <li key={item}>{item}</li>
        ))}
      </ul>
      <pre className="max-h-72 overflow-auto rounded-md bg-muted p-3 text-xs">
        {JSON.stringify(preview.nextModelsJson, null, 2)}
      </pre>
    </div>
  );
}
