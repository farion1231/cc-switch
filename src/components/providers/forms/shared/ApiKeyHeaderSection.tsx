import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { ChevronRight, X } from "lucide-react";

interface ApiKeyHeaderSectionProps {
  apiKeyHeaderName?: string;
  onApiKeyHeaderNameChange: (name: string | undefined) => void;
}

export function ApiKeyHeaderSection({
  apiKeyHeaderName,
  onApiKeyHeaderNameChange,
}: ApiKeyHeaderSectionProps) {
  const { t } = useTranslation();

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <FormLabel>
          {t("providerForm.apiKeyHeader", {
            defaultValue: "API Key Header",
          })}
        </FormLabel>
        {apiKeyHeaderName ? (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-6 px-2 text-xs text-muted-foreground hover:text-foreground"
            onClick={() => onApiKeyHeaderNameChange(undefined)}
          >
            <X className="h-3 w-3 mr-1" />
            {t("providerForm.apiKeyHeaderReset", {
              defaultValue: "恢复默认",
            })}
          </Button>
        ) : null}
      </div>
      {apiKeyHeaderName !== undefined ? (
        <Input
          type="text"
          value={apiKeyHeaderName}
          onChange={(e) =>
            onApiKeyHeaderNameChange(e.target.value || undefined)
          }
          placeholder={t("providerForm.apiKeyHeaderPlaceholder", {
            defaultValue: "例如：x-api-key",
          })}
          autoComplete="off"
        />
      ) : (
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="w-full justify-between text-muted-foreground font-normal h-9"
          onClick={() => onApiKeyHeaderNameChange("")}
        >
          <span>
            {t("providerForm.apiKeyHeaderDefault", {
              defaultValue: "Authorization: Bearer（默认）",
            })}
          </span>
          <ChevronRight className="h-3.5 w-3.5 shrink-0" />
        </Button>
      )}
      <p className="text-xs text-muted-foreground">
        {t("providerForm.apiKeyHeaderHint", {
          defaultValue: "自定义后将使用原始 Key 值直接发送，不带 Bearer 前缀",
        })}
      </p>
    </div>
  );
}
