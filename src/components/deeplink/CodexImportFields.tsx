import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import type { DeepLinkImportRequest } from "@/lib/api/deeplink";
import { reasoningLevels, updateImportModel } from "@/utils/codexImport";

export function CodexImportFields({
  request,
  onChange,
}: {
  request: DeepLinkImportRequest;
  onChange: (request: DeepLinkImportRequest) => void;
}) {
  const { t } = useTranslation();
  const rows = request.modelCatalog?.models ?? [];
  return (
    <section
      className="space-y-3 rounded-md border p-4"
      aria-label={t("codexCustomization.import.label")}
    >
      <p className="font-medium">{t("codexCustomization.import.title")}</p>
      <p className="text-sm text-muted-foreground">
        {t("codexCustomization.import.description")}
      </p>
      <label className="block text-sm">
        {t("codexCustomization.import.endpoint")}
        <Input
          value={request.endpoint ?? ""}
          onChange={(e) => onChange({ ...request, endpoint: e.target.value })}
        />
      </label>
      <label className="block text-sm">
        {t("codexCustomization.import.apiKey")}
        <Input
          type="password"
          autoComplete="off"
          value={request.apiKey ?? ""}
          onChange={(e) => onChange({ ...request, apiKey: e.target.value })}
        />
      </label>
      {rows.map((row, index) => (
        <div key={index} className="space-y-2 rounded border p-3">
          <div className="flex gap-2">
            <Input
              aria-label={t("codexCustomization.import.modelRow", {
                index: index + 1,
              })}
              placeholder={t("codexCustomization.import.modelId")}
              value={row.model}
              onChange={(e) =>
                onChange(
                  updateImportModel(request, index, {
                    ...row,
                    model: e.target.value,
                  }),
                )
              }
            />
            <Button
              type="button"
              variant="outline"
              onClick={() =>
                onChange({
                  ...request,
                  modelCatalog: {
                    ...request.modelCatalog,
                    models: rows.filter((_, i) => i !== index),
                  },
                })
              }
            >
              {t("codexCustomization.import.remove")}
            </Button>
          </div>
          <p className="text-xs text-muted-foreground">
            {t("codexCustomization.import.levelsHint")}
          </p>
          <div className="flex flex-wrap gap-3">
            {reasoningLevels.map((level) => (
              <label key={level} className="flex items-center gap-1 text-sm">
                <input
                  type="checkbox"
                  checked={row.reasoningLevels?.includes(level) ?? false}
                  onChange={(e) =>
                    onChange(
                      updateImportModel(request, index, {
                        ...row,
                        reasoningLevels: e.target.checked
                          ? [...(row.reasoningLevels ?? []), level]
                          : row.reasoningLevels?.filter((v) => v !== level),
                      }),
                    )
                  }
                />
                {level}
              </label>
            ))}
          </div>
        </div>
      ))}
      <Button
        type="button"
        variant="outline"
        onClick={() =>
          onChange({
            ...request,
            modelCatalog: {
              ...request.modelCatalog,
              models: [...rows, { model: "" }],
            },
          })
        }
      >
        {t("codexCustomization.import.addModel")}
      </Button>
      <label className="block text-sm">
        {t("codexCustomization.import.defaultModel")}
        <select
          className="mt-1 w-full rounded border bg-background p-2"
          value={request.model ?? ""}
          onChange={(e) => onChange({ ...request, model: e.target.value })}
        >
          <option value="">{t("codexCustomization.import.chooseModel")}</option>
          {rows
            .filter((row) => row.model)
            .map((row, i) => (
              <option key={i} value={row.model}>
                {row.model}
              </option>
            ))}
        </select>
      </label>
      <p className="text-xs text-muted-foreground">
        {t("codexCustomization.import.usageHint")}
      </p>
    </section>
  );
}
