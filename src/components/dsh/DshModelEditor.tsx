import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";
import type { DshModel } from "@/lib/api/dsh";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  formatDshCapacity,
  parseDshCapacity,
  validateDshModels,
} from "./dshModelUtils";

interface DshModelEditorProps {
  models: DshModel[];
  onChange: (models: DshModel[]) => void;
  disabled?: boolean;
  discoverable?: boolean;
  onDiscover?: () => void;
  discovering?: boolean;
}

/** Editable model catalog used by both native and custom DSH forms. */
export function DshModelEditor({
  models,
  onChange,
  disabled = false,
  discoverable = false,
  onDiscover,
  discovering = false,
}: DshModelEditorProps) {
  const { t } = useTranslation();
  const failure = useMemo(() => validateDshModels(models), [models]);

  const update = (index: number, patch: Partial<DshModel>) => {
    onChange(
      models.map((model, row) =>
        row === index ? { ...model, ...patch } : model,
      ),
    );
  };

  const remove = (index: number) => {
    onChange(models.filter((_, row) => row !== index));
  };

  return (
    <fieldset className="space-y-3" disabled={disabled}>
      <div className="flex items-center justify-between gap-3">
        <div>
          <Label>{t("dsh.models.title")}</Label>
          <p className="mt-1 text-xs text-muted-foreground">
            {t("dsh.models.description")}
          </p>
        </div>
        <div className="flex gap-2">
          {discoverable && onDiscover && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={onDiscover}
              disabled={discovering}
            >
              {discovering
                ? t("dsh.models.discovering")
                : t("dsh.models.discover")}
            </Button>
          )}
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => onChange([...models, { id: "" }])}
          >
            <Plus className="h-3.5 w-3.5" />
            {t("dsh.models.add")}
          </Button>
        </div>
      </div>

      {models.length === 0 && (
        <div className="rounded-md border border-dashed p-4 text-sm text-muted-foreground">
          {t("dsh.models.empty")}
        </div>
      )}

      <div className="space-y-3">
        {models.map((model, index) => (
          <div key={`${index}-${model.id}`} className="rounded-md border p-3">
            <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]">
              <div className="space-y-1.5">
                <Label htmlFor={`dsh-model-id-${index}`}>
                  {t("dsh.models.id")}
                </Label>
                <Input
                  id={`dsh-model-id-${index}`}
                  value={model.id}
                  onChange={(event) =>
                    update(index, { id: event.target.value })
                  }
                  placeholder="deepseek-chat"
                  aria-invalid={failure?.index === index && !model.id.trim()}
                />
              </div>
              <div className="space-y-1.5">
                <Label htmlFor={`dsh-model-name-${index}`}>
                  {t("dsh.models.displayName")}
                </Label>
                <Input
                  id={`dsh-model-name-${index}`}
                  value={model.name ?? ""}
                  onChange={(event) =>
                    update(index, { name: event.target.value || undefined })
                  }
                  placeholder="DeepSeek Chat"
                />
              </div>
              <div className="flex items-end justify-end">
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  aria-label={t("dsh.models.remove", { index: index + 1 })}
                  onClick={() => remove(index)}
                >
                  <Trash2 className="h-4 w-4 text-destructive" />
                </Button>
              </div>
            </div>
            <div className="mt-3 grid gap-3 sm:grid-cols-2">
              <div className="space-y-1.5">
                <Label htmlFor={`dsh-model-context-${index}`}>
                  {t("dsh.models.contextWindow")}
                </Label>
                <Input
                  id={`dsh-model-context-${index}`}
                  inputMode="numeric"
                  value={formatDshCapacity(model.contextWindow)}
                  onChange={(event) =>
                    update(index, {
                      contextWindow: parseDshCapacity(event.target.value),
                    })
                  }
                  placeholder="1M"
                />
              </div>
              <div className="space-y-1.5">
                <Label htmlFor={`dsh-model-max-${index}`}>
                  {t("dsh.models.maxTokens")}
                </Label>
                <Input
                  id={`dsh-model-max-${index}`}
                  inputMode="numeric"
                  value={formatDshCapacity(model.maxTokens)}
                  onChange={(event) =>
                    update(index, {
                      maxTokens: parseDshCapacity(event.target.value),
                    })
                  }
                  placeholder="256K"
                />
              </div>
            </div>
            {failure?.index === index && (
              <p className="mt-2 text-xs text-destructive" role="alert">
                {t(failure.messageKey, {
                  field: failure.field
                    ? t(`dsh.validation.fields.${failure.field}`)
                    : "",
                })}
              </p>
            )}
          </div>
        ))}
      </div>
    </fieldset>
  );
}
