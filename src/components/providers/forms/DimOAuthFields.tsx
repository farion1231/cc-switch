import { useEffect, useId, useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { dimOAuthApi, type DimModel } from "@/lib/api/dim-oauth";
import type { ProviderMeta } from "@/types";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useDimOauth } from "./hooks/useDimOauth";
import { DimAccountUsage } from "./DimOAuthSection";

interface Props {
  appId: "claude" | "codex";
  accountId: string | null;
  onAccountChange: (id: string | null) => void;
  onManageAccounts?: () => void;
  model: string;
  apiFormat: NonNullable<ProviderMeta["apiFormat"]>;
  effort: string;
  onValidityChange: (valid: boolean) => void;
  onSelect: (
    model: DimModel,
    format: NonNullable<ProviderMeta["apiFormat"]>,
    effort: string,
  ) => void;
}
export function DimOAuthFields({
  appId,
  accountId,
  onAccountChange,
  onManageAccounts,
  model,
  apiFormat,
  effort,
  onSelect,
  onValidityChange,
}: Props) {
  const { t } = useTranslation();
  const id = useId();
  const auth = useDimOauth();
  const account = auth.accounts.find(
    (item) => item.id === accountId && !item.requires_reauth,
  );
  const models = useQuery({
    queryKey: ["dim_oauth", "models", accountId],
    queryFn: () => dimOAuthApi.models(accountId!),
    enabled: !!account,
    staleTime: 60000,
  });
  const available = useMemo(
    () =>
      (models.data ?? []).filter(
        (item) =>
          appId !== "codex" ||
          item.api_formats.some((format) => format !== "gemini_native"),
      ),
    [models.data, appId],
  );
  const selected = available.find((item) => item.id === model);
  const formats = useMemo(
    () =>
      selected?.api_formats.filter(
        (format) => appId !== "codex" || format !== "gemini_native",
      ) ?? [],
    [selected, appId],
  );
  useEffect(() => {
    if (selected && apiFormat === "gemini_native" && effort)
      onSelect(selected, apiFormat, "");
    else if (selected && !formats.includes(apiFormat))
      onSelect(selected, formats[0], "");
  }, [selected, apiFormat, formats, effort, onSelect]);
  useEffect(() => {
    onValidityChange(
      models.isSuccess &&
        !!account &&
        !!selected &&
        formats.includes(apiFormat) &&
        (!effort ||
          (apiFormat !== "gemini_native" && selected.efforts.includes(effort))),
    );
  }, [
    models.isSuccess,
    account,
    selected,
    formats,
    apiFormat,
    effort,
    onValidityChange,
  ]);
  return (
    <div className="space-y-5">
      <div className="space-y-2">
        <div className="flex items-center justify-between gap-3">
          <Label htmlFor={`${id}-account`}>{t("dimOauth.account")}</Label>
          <Button type="button" variant="link" onClick={onManageAccounts}>
            {t("dimOauth.manageAccounts")}
          </Button>
        </div>
        <Select
          value={accountId ?? "none"}
          onValueChange={(value) =>
            onAccountChange(value === "none" ? null : value)
          }
        >
          <SelectTrigger id={`${id}-account`}>
            <SelectValue placeholder={t("dimOauth.selectAccount")} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="none">{t("dimOauth.selectAccount")}</SelectItem>
            {auth.accounts.map((item) => (
              <SelectItem
                key={item.id}
                value={item.id}
                disabled={item.requires_reauth}
              >
                {item.login} · {t(`dimOauth.site.${item.site}`)}
                {item.requires_reauth ? ` · ${t("dimOauth.reauth")}` : ""}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        {auth.status.isError && (
          <p role="alert" className="text-caption text-destructive">
            {String(auth.status.error)}{" "}
            <button type="button" onClick={() => void auth.status.refetch()}>
              {t("common.retry")}
            </button>
          </p>
        )}
        {!account && (
          <p className="text-caption text-fg-2">
            {t("dimOauth.loginRequired")}
          </p>
        )}
      </div>
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <Label htmlFor={`${id}-model`}>{t("dimOauth.model")}</Label>
          <Button
            type="button"
            variant="ghost"
            disabled={!account || models.isFetching}
            onClick={() => void models.refetch()}
          >
            {t("dimOauth.refreshModels")}
          </Button>
        </div>
        <Select
          value={selected?.id ?? ""}
          disabled={!account || models.isPending}
          onValueChange={(value) => {
            const next = available.find((item) => item.id === value);
            if (next)
              onSelect(
                next,
                next.api_formats.find(
                  (format) => appId !== "codex" || format !== "gemini_native",
                )!,
                "",
              );
          }}
        >
          <SelectTrigger id={`${id}-model`}>
            <SelectValue placeholder={model || t("dimOauth.selectModel")} />
          </SelectTrigger>
          <SelectContent>
            {available.map((item) => (
              <SelectItem key={item.id} value={item.id}>
                {item.name} · {item.id}
                {item.zdr ? " · ZDR" : ""}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        {models.isError && (
          <p role="alert" className="text-caption text-destructive">
            {String(models.error)}
          </p>
        )}
        {models.isSuccess && available.length === 0 && (
          <p className="text-caption text-fg-2">{t("dimOauth.noModels")}</p>
        )}
      </div>
      {selected && (
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          <div className="space-y-2">
            <Label htmlFor={`${id}-format`}>{t("dimOauth.format")}</Label>
            <Select
              value={apiFormat}
              onValueChange={(value) =>
                onSelect(
                  selected,
                  value as Props["apiFormat"],
                  value === "gemini_native" ? "" : effort,
                )
              }
            >
              <SelectTrigger id={`${id}-format`}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {formats.map((format) => (
                  <SelectItem key={format} value={format}>
                    {t(`dimOauth.formats.${format}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-2">
            <Label htmlFor={`${id}-effort`}>{t("dimOauth.effort")}</Label>
            <Select
              disabled={apiFormat === "gemini_native"}
              value={effort || "default"}
              onValueChange={(value) =>
                onSelect(selected, apiFormat, value === "default" ? "" : value)
              }
            >
              <SelectTrigger id={`${id}-effort`}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="default">
                  {t("dimOauth.defaultEffort")}
                </SelectItem>
                {selected.efforts.map((value) => (
                  <SelectItem key={value} value={value}>
                    {value}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>
      )}
      <p className="text-caption text-fg-2">{t("dimOauth.routing")}</p>
      {account && <DimAccountUsage accountId={account.id} />}
    </div>
  );
}
