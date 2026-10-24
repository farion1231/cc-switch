import { useTranslation } from "react-i18next";
import { useEffect, useRef, useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import {
  codexSelectionApi,
  type CodexSelection,
  type CodexSwitchPreview,
} from "@/lib/api/codexSelection";

export function CodexSwitchReview({
  preview,
  onClose,
}: {
  preview: CodexSwitchPreview;
  onClose: (selection: CodexSelection | null) => void;
}) {
  const { t } = useTranslation();
  const [selected, setSelected] = useState(() =>
    preview.fields.filter((f) => f.selected).map((f) => f.id),
  );
  const [model, setModel] = useState(preview.defaultModel ?? "");
  const [effort, setEffort] = useState(preview.currentEffort ?? "");
  const effectiveModel = selected.includes('["model"]')
    ? model
    : (preview.currentModel ?? "");
  const models = selected.includes("catalog")
    ? preview.models
    : preview.currentModels;
  const levels =
    models.find((m) => m.model === effectiveModel)?.reasoningLevels ?? [];
  const changesModels = selected.some((id) =>
    ["connection", "catalog", '["model"]'].includes(id),
  );
  const valid =
    (!preview.exitToDirect || selected.includes("connection")) &&
    (!changesModels ||
      preview.proxy ||
      preview.native ||
      (!!effectiveModel && levels.includes(effort)));
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose(null);
      }}
    >
      <DialogContent
        zIndex="top"
        className="max-w-3xl max-h-[90vh] overflow-y-auto"
      >
        <DialogHeader>
          <DialogTitle>{t("codexCustomization.review.title")}</DialogTitle>
          <DialogDescription>
            {preview.proxy
              ? t("codexCustomization.review.proxy")
              : preview.native
                ? t("codexCustomization.review.native")
                : preview.exitToDirect
                  ? t("codexCustomization.review.exit")
                  : preview.routing
                    ? t("codexCustomization.review.routing")
                    : t("codexCustomization.review.direct")}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          {preview.fields.map((field) => (
            <div key={field.id} className="rounded border p-3">
              <label className="flex items-center gap-2 font-medium">
                <input
                  type="checkbox"
                  checked={selected.includes(field.id)}
                  onChange={(e) =>
                    setSelected((old) =>
                      e.target.checked
                        ? [...old, field.id]
                        : old.filter((id) => id !== field.id),
                    )
                  }
                />
                {t(field.label, { defaultValue: field.label })}
              </label>
              <div className="mt-2 grid grid-cols-2 gap-2 text-xs">
                <div>
                  <span className="text-muted-foreground">
                    {t("codexCustomization.review.before")}
                  </span>
                  <pre className="whitespace-pre-wrap break-all">
                    {field.before || t("codexCustomization.review.unset")}
                  </pre>
                </div>
                <div>
                  <span className="text-muted-foreground">
                    {t("codexCustomization.review.after")}
                  </span>
                  <pre className="whitespace-pre-wrap break-all">
                    {field.after || t("codexCustomization.review.unset")}
                  </pre>
                </div>
              </div>
            </div>
          ))}
          {!preview.proxy && !preview.native && changesModels && (
            <>
              <label className="block text-sm">
                {t("codexCustomization.import.defaultModel")}
                <select
                  aria-label={t("codexCustomization.import.defaultModel")}
                  className="mt-1 w-full rounded border bg-background p-2"
                  disabled={!selected.includes('["model"]')}
                  value={effectiveModel}
                  onChange={(e) => setModel(e.target.value)}
                >
                  <option value="">
                    {t("codexCustomization.review.chooseModel")}
                  </option>
                  {models.map((row) => (
                    <option key={row.model} value={row.model}>
                      {row.model}
                    </option>
                  ))}
                </select>
              </label>
              <label className="block text-sm">
                {t("codexCustomization.review.effortWithCurrent", {
                  current:
                    preview.currentEffort ??
                    t("codexCustomization.review.unset"),
                })}
                <select
                  aria-label={t("codexCustomization.review.effort")}
                  className="mt-1 w-full rounded border bg-background p-2"
                  value={levels.includes(effort) ? effort : ""}
                  onChange={(e) => setEffort(e.target.value)}
                >
                  <option value="">
                    {t("codexCustomization.review.chooseEffort")}
                  </option>
                  {levels.map((level) => (
                    <option key={level} value={level}>
                      {level}
                    </option>
                  ))}
                </select>
              </label>
              {!valid && (
                <p role="alert" className="text-sm text-warning-text">
                  {t("codexCustomization.review.invalid")}
                </p>
              )}
            </>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onClose(null)}>
            {t("codexCustomization.review.cancel")}
          </Button>
          <Button
            disabled={!valid || !selected.length}
            onClick={() =>
              onClose({
                revision: preview.revision,
                selected,
                model: effectiveModel || null,
                effort: effort || null,
                ...(preview.exitToDirect ? { exitToDirect: true } : {}),
              })
            }
          >
            {t("codexCustomization.review.apply")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function useCodexSwitchReview(app: string) {
  const [preview, setPreview] = useState<CodexSwitchPreview | null>(null);
  const pending = useRef<((value: CodexSelection | null) => void) | null>(null);
  const generation = useRef(0);
  const close = (value: CodexSelection | null) => {
    pending.current?.(value);
    pending.current = null;
    setPreview(null);
  };
  useEffect(() => {
    generation.current++;
    close(null);
    return () => {
      generation.current++;
      pending.current?.(null);
      pending.current = null;
    };
  }, [app]);
  const review = async (id: string, exitToDirect = false) => {
    const turn = ++generation.current;
    close(null);
    const next = await codexSelectionApi.preview(id, exitToDirect);
    if (generation.current !== turn) return null;
    return new Promise<CodexSelection | null>((resolve) => {
      pending.current = resolve;
      setPreview(next);
    });
  };
  return {
    review,
    dialog: preview ? (
      <CodexSwitchReview
        key={preview.revision}
        preview={preview}
        onClose={close}
      />
    ) : null,
  };
}
