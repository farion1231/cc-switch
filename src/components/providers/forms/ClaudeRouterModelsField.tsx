import {
  ArrowDown,
  ArrowUp,
  Download,
  Loader2,
  Plus,
  Trash2,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import type { FetchedModel } from "@/lib/api/model-fetch";
import type { ClaudeRouterConfig, ClaudeRouterModel } from "@/types";
import { ModelInputWithFetch } from "./shared";

export type ClaudeRouterValidationError =
  | "modelsRequired"
  | "fieldsRequired"
  | "duplicateAlias";

export function normalizeClaudeRouterConfig(
  value: ClaudeRouterConfig,
): ClaudeRouterConfig {
  return {
    enabled: value.enabled,
    models: value.models.map((model) => ({
      alias: model.alias.trim(),
      displayName: model.displayName.trim(),
      upstreamModel: model.upstreamModel.trim(),
    })),
  };
}

export function getClaudeRouterValidationError(
  value: ClaudeRouterConfig,
): ClaudeRouterValidationError | null {
  if (!value.enabled) return null;

  const normalized = normalizeClaudeRouterConfig(value);
  if (normalized.models.length === 0) return "modelsRequired";
  if (
    normalized.models.some(
      (model) => !model.alias || !model.displayName || !model.upstreamModel,
    )
  ) {
    return "fieldsRequired";
  }

  const aliases = new Set<string>();
  for (const model of normalized.models) {
    if (aliases.has(model.alias)) return "duplicateAlias";
    aliases.add(model.alias);
  }
  return null;
}

interface ClaudeRouterModelsFieldProps {
  value: ClaudeRouterConfig;
  onChange: (value: ClaudeRouterConfig) => void;
  fetchedModels: FetchedModel[];
  isLoading: boolean;
  onFetch: () => void;
  disableEnable?: boolean;
}

export function ClaudeRouterModelsField({
  value,
  onChange,
  fetchedModels,
  isLoading,
  onFetch,
  disableEnable = false,
}: ClaudeRouterModelsFieldProps) {
  const { t } = useTranslation();
  const validationError = getClaudeRouterValidationError(value);

  const updateModel = (
    index: number,
    field: keyof ClaudeRouterModel,
    nextValue: string,
  ) => {
    onChange({
      ...value,
      models: value.models.map((model, modelIndex) =>
        modelIndex === index ? { ...model, [field]: nextValue } : model,
      ),
    });
  };

  const moveModel = (index: number, direction: -1 | 1) => {
    const destination = index + direction;
    if (destination < 0 || destination >= value.models.length) return;
    const models = [...value.models];
    [models[index], models[destination]] = [models[destination], models[index]];
    onChange({ ...value, models });
  };

  return (
    <section className="space-y-3 rounded-lg border border-border-default p-4">
      <div className="flex items-start justify-between gap-4">
        <div className="space-y-1">
          <Label htmlFor="claude-router-enabled">
            {t("claudeRouter.form.enabled", {
              defaultValue: "Enabled for Router",
            })}
          </Label>
          <p className="text-xs text-muted-foreground">
            {t("claudeRouter.form.description", {
              defaultValue:
                "Expose selected models through the Claude Code gateway without changing the active provider.",
            })}
          </p>
          {disableEnable && !value.enabled && (
            <p className="text-xs text-amber-600 dark:text-amber-400">
              {t("claudeRouter.form.officialUnsupported", {
                defaultValue:
                  "Official providers cannot be enabled for the router.",
              })}
            </p>
          )}
        </div>
        <Switch
          id="claude-router-enabled"
          aria-label={t("claudeRouter.form.enabled", {
            defaultValue: "Enabled for Router",
          })}
          checked={value.enabled}
          disabled={disableEnable && !value.enabled}
          onCheckedChange={(enabled) => onChange({ ...value, enabled })}
        />
      </div>

      <div className="flex flex-col gap-3 border-t border-border-default pt-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <p className="text-sm font-medium">
            {t("claudeRouter.form.models", { defaultValue: "Gateway models" })}
          </p>
          <p className="text-xs text-muted-foreground">
            {t("claudeRouter.form.modelsHint", {
              defaultValue:
                "Aliases become stable public model IDs; upstream model names remain editable.",
            })}
          </p>
        </div>
        <div className="flex w-full flex-wrap gap-2 sm:w-auto sm:shrink-0">
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={onFetch}
            disabled={isLoading}
          >
            {isLoading ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Download className="h-3.5 w-3.5" />
            )}
            {t("providerForm.fetchModels", { defaultValue: "Fetch models" })}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() =>
              onChange({
                ...value,
                models: [
                  ...value.models,
                  { alias: "", displayName: "", upstreamModel: "" },
                ],
              })
            }
          >
            <Plus className="h-3.5 w-3.5" />
            {t("claudeRouter.form.addModel", { defaultValue: "Add model" })}
          </Button>
        </div>
      </div>

      {value.models.length === 0 ? (
        <p className="rounded-md border border-dashed p-3 text-xs text-muted-foreground">
          {t("claudeRouter.form.empty", {
            defaultValue: "No gateway models configured.",
          })}
        </p>
      ) : (
        <div className="space-y-3">
          {value.models.map((model, index) => {
            const position = index + 1;
            return (
              <div
                key={index}
                className="grid gap-3 rounded-md border border-border bg-muted/20 p-3 sm:grid-cols-2"
              >
                <div className="space-y-1.5">
                  <Label htmlFor={`claude-router-alias-${index}`}>
                    {t("claudeRouter.form.alias", {
                      position,
                      defaultValue: `Alias ${position}`,
                    })}
                  </Label>
                  <Input
                    id={`claude-router-alias-${index}`}
                    value={model.alias}
                    onChange={(event) =>
                      updateModel(index, "alias", event.target.value)
                    }
                    autoComplete="off"
                  />
                </div>
                <div className="space-y-1.5">
                  <Label htmlFor={`claude-router-display-name-${index}`}>
                    {t("claudeRouter.form.displayName", {
                      position,
                      defaultValue: `Display Name ${position}`,
                    })}
                  </Label>
                  <Input
                    id={`claude-router-display-name-${index}`}
                    value={model.displayName}
                    onChange={(event) =>
                      updateModel(index, "displayName", event.target.value)
                    }
                    autoComplete="off"
                  />
                </div>
                <div className="space-y-1.5 sm:col-span-2">
                  <Label htmlFor={`claude-router-upstream-${index}`}>
                    {t("claudeRouter.form.upstreamModel", {
                      position,
                      defaultValue: `Upstream Model ${position}`,
                    })}
                  </Label>
                  <ModelInputWithFetch
                    id={`claude-router-upstream-${index}`}
                    value={model.upstreamModel}
                    onChange={(nextValue) =>
                      updateModel(index, "upstreamModel", nextValue)
                    }
                    fetchedModels={fetchedModels}
                    isLoading={isLoading}
                  />
                </div>
                <div className="flex justify-end gap-1 sm:col-span-2">
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    disabled={index === 0}
                    aria-label={t("claudeRouter.form.moveUp", {
                      position,
                      defaultValue: `Move model ${position} up`,
                    })}
                    onClick={() => moveModel(index, -1)}
                  >
                    <ArrowUp className="h-4 w-4" />
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    disabled={index === value.models.length - 1}
                    aria-label={t("claudeRouter.form.moveDown", {
                      position,
                      defaultValue: `Move model ${position} down`,
                    })}
                    onClick={() => moveModel(index, 1)}
                  >
                    <ArrowDown className="h-4 w-4" />
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    aria-label={t("claudeRouter.form.remove", {
                      position,
                      defaultValue: `Remove model ${position}`,
                    })}
                    onClick={() =>
                      onChange({
                        ...value,
                        models: value.models.filter(
                          (_, modelIndex) => modelIndex !== index,
                        ),
                      })
                    }
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {value.enabled && validationError && (
        <p className="text-xs text-destructive" role="alert">
          {t(`claudeRouter.validation.${validationError}`, {
            defaultValue:
              validationError === "modelsRequired"
                ? "Add at least one model before enabling the router."
                : validationError === "fieldsRequired"
                  ? "Every router model requires an alias, display name, and upstream model."
                  : "Router model aliases must be unique.",
          })}
        </p>
      )}
    </section>
  );
}
