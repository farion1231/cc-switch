import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { DshDefaultModel, DshProvider } from "@/lib/api/dsh";
import { dshErrorMessage, isDshConflictError } from "@/lib/api/dsh";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

interface DshDefaultModelPickerProps {
  providers: readonly DshProvider[];
  value: DshDefaultModel | null;
  disabled?: boolean;
  onSave: (selection: DshDefaultModel) => Promise<void>;
}

/** Provider/model selector for future DSH agents, separate from route CRUD. */
export function DshDefaultModelPicker({
  providers,
  value,
  disabled = false,
  onSave,
}: DshDefaultModelPickerProps) {
  const { t } = useTranslation();
  const options = useMemo(
    () =>
      providers.flatMap((provider) =>
        provider.models.map((model) => ({
          provider: provider.route,
          providerName: provider.displayName,
          model: model.id,
          modelName: model.name,
        })),
      ),
    [providers],
  );
  const [provider, setProvider] = useState(value?.provider ?? "");
  const [model, setModel] = useState(value?.model ?? "");
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string>();

  useEffect(() => {
    setProvider(value?.provider ?? "");
    setModel(value?.model ?? "");
  }, [value]);

  const providerOptions = useMemo(() => {
    const seen = new Set<string>();
    return options.filter((option) => {
      if (seen.has(option.provider)) return false;
      seen.add(option.provider);
      return true;
    });
  }, [options]);
  const modelOptions = options.filter((option) => option.provider === provider);
  const unavailable = Boolean(
    value &&
      !options.some(
        (option) =>
          option.provider === value.provider && option.model === value.model,
      ),
  );

  const save = async () => {
    if (!provider || !model) return;
    setBusy(true);
    setFailure(undefined);
    try {
      // The first version of this picker has no effort control. Omitting it
      // explicitly clears an old effort that belonged to a prior route.
      await onSave({ provider, model });
    } catch (error) {
      setFailure(
        isDshConflictError(error)
          ? t("dsh.errors.conflict")
          : dshErrorMessage(error, t("dsh.errors.defaultModelSave")),
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <section
      className="rounded-lg border bg-card p-4 shadow-sm"
      aria-label={t("dsh.defaultModel.title")}
    >
      <div className="mb-3 flex items-start justify-between gap-3">
        <div>
          <h2 className="font-semibold">{t("dsh.defaultModel.title")}</h2>
          <p className="mt-1 text-xs text-muted-foreground">
            {t("dsh.defaultModel.description")}
          </p>
        </div>
        <Button
          type="button"
          size="sm"
          onClick={() => void save()}
          disabled={disabled || busy || !provider || !model}
        >
          {t("common.save")}
        </Button>
      </div>
      {unavailable && (
        <p className="mb-3 rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-700 dark:text-amber-300">
          {t("dsh.defaultModel.unavailable")}
        </p>
      )}
      <div className="grid gap-3 sm:grid-cols-2">
        <div className="space-y-1.5">
          <Label>{t("dsh.fields.provider")}</Label>
          <Select
            value={provider}
            onValueChange={(next) => {
              setProvider(next);
              setModel(
                options.find((option) => option.provider === next)?.model ?? "",
              );
            }}
            disabled={disabled}
          >
            <SelectTrigger>
              <SelectValue
                placeholder={t("dsh.defaultModel.providerPlaceholder")}
              />
            </SelectTrigger>
            <SelectContent>
              {providerOptions.map((option) => (
                <SelectItem key={option.provider} value={option.provider}>
                  {option.providerName} ({option.provider})
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-1.5">
          <Label>{t("dsh.fields.model")}</Label>
          <Select
            value={model}
            onValueChange={setModel}
            disabled={disabled || !provider}
          >
            <SelectTrigger>
              <SelectValue
                placeholder={t("dsh.defaultModel.modelPlaceholder")}
              />
            </SelectTrigger>
            <SelectContent>
              {modelOptions.map((option) => (
                <SelectItem
                  key={`${option.provider}:${option.model}`}
                  value={option.model}
                >
                  {option.modelName ?? option.model}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>
      {failure && (
        <p className="mt-2 text-xs text-destructive" role="alert">
          {failure}
        </p>
      )}
    </section>
  );
}
