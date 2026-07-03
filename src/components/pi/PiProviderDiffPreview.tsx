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
  if (!preview) {
    return (
      <div className="rounded-lg border border-dashed border-border-default p-4 text-sm text-muted-foreground">
        Preview a patch before writing to Pi models.json.
      </div>
    );
  }

  return (
    <div className="space-y-3 rounded-lg border border-border-default p-4">
      <div className="flex items-center justify-between gap-3">
        <div>
          <h3 className="text-sm font-semibold">Review & Apply</h3>
          <p className="text-xs text-muted-foreground">
            Current file hash: {preview.currentFileHash || "(new file)"}
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
              Delete Provider
            </Button>
          )}
          <Button type="button" onClick={onApply} disabled={isApplying}>
            {isApplying ? "Applying..." : "Apply to models.json"}
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
