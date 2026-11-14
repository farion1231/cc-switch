import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { DeepLinkImportRequest } from "../../lib/api/deeplink";
import { decodeBase64Utf8 } from "../../lib/utils/base64";
import { Switch } from "../ui/switch";

export function PromptConfirmation({
  request,
  enableAfterImport,
  onEnableAfterImportChange,
}: {
  request: DeepLinkImportRequest;
  /** 导入后是否立即启用：用户在确认框里决定，链接里的 enabled 只是初始值 */
  enableAfterImport: boolean;
  onEnableAfterImportChange: (enabled: boolean) => void;
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
        <label className="block text-sm font-medium text-fg-2">
          {t("deeplink.prompt.app")}
        </label>
        <div className="mt-1 text-sm capitalize">{request.app}</div>
      </div>

      <div>
        <label className="block text-sm font-medium text-fg-2">
          {t("deeplink.prompt.name")}
        </label>
        <div className="mt-1 text-sm">{request.name}</div>
      </div>

      {request.description && (
        <div>
          <label className="block text-sm font-medium text-fg-2">
            {t("deeplink.prompt.description")}
          </label>
          <div className="mt-1 text-sm">{request.description}</div>
        </div>
      )}

      <div>
        <label className="block text-sm font-medium text-fg-2">
          {t("deeplink.prompt.contentPreview")}
        </label>
        <pre className="mt-1 max-h-48 overflow-auto bg-subtle p-2 rounded text-xs whitespace-pre-wrap border">
          {decodedContent.substring(0, 500)}
          {decodedContent.length > 500 && "..."}
        </pre>
      </div>

      <div className="flex items-center justify-between gap-4">
        <label
          htmlFor="deeplink-prompt-enable-after-import"
          className="text-sm font-medium"
        >
          {t("deeplink.prompt.enableAfterImport")}
        </label>
        <Switch
          id="deeplink-prompt-enable-after-import"
          checked={enableAfterImport}
          onCheckedChange={onEnableAfterImportChange}
        />
      </div>

      {enableAfterImport && (
        <div className="text-warning-text text-sm flex items-center gap-2">
          <span>⚠️</span>
          <span>{t("deeplink.prompt.enabledWarning")}</span>
        </div>
      )}
    </div>
  );
}
