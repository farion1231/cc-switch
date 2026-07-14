import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { DeepLinkImportRequest } from "../../lib/api/deeplink";
import { decodeBase64Utf8 } from "../../lib/utils/base64";

export function PromptConfirmation({
  request,
}: {
  request: DeepLinkImportRequest;
}) {
  const { t } = useTranslation();

  const decodedContent = useMemo(() => {
    if (!request.content) return "";
    return decodeBase64Utf8(request.content);
  }, [request.content]);

  return (
    <div className="space-y-4">
      <h3 className="text-lg font-semibold">{t("deeplink.prompt.title")}</h3>

      <div>
        <label className="block text-sm font-medium text-muted-foreground">
          {t("deeplink.prompt.app")}
        </label>
        <div className="mt-1 text-sm capitalize">{request.app}</div>
      </div>

      <div>
        <label className="block text-sm font-medium text-muted-foreground">
          {t("deeplink.prompt.name")}
        </label>
        <div className="mt-1 text-sm">{request.name}</div>
      </div>

      {request.description && (
        <div>
          <label className="block text-sm font-medium text-muted-foreground">
            {t("deeplink.prompt.description")}
          </label>
          <div className="mt-1 text-sm">{request.description}</div>
        </div>
      )}

      <div>
        <label className="block text-sm font-medium text-muted-foreground">
          {t("deeplink.prompt.contentPreview")}
        </label>
        <pre className="mt-1 max-h-48 overflow-auto bg-muted/50 p-2 rounded text-xs whitespace-pre-wrap border">
          {decodedContent.substring(0, 500)}
          {decodedContent.length > 500 && "..."}
        </pre>
      </div>

      {request.enabled && (
        <div className="text-yellow-600 dark:text-yellow-500 text-sm flex items-center gap-2">
          <span>⚠️</span>
          <span>{t("deeplink.prompt.enabledWarning")}</span>
        </div>
      )}
    </div>
  );
}
