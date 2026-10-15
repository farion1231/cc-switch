import { useTranslation } from "react-i18next";
import { Download, Loader2 } from "lucide-react";
import { useApiKeyVarTemplate } from "@/lib/query/apiKeyVars";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { extractErrorMessage } from "@/utils/errorUtils";

const ENV_NAMES = [
  "CC_SWITCH_CONTRACT_VERSION",
  "CC_SWITCH_VAR_NAME",
  "CC_SWITCH_REASON",
  "CC_SWITCH_CURRENT_EXPIRES_AT",
] as const;

/** 折叠说明和配置弹窗复用同一内容，代码与下载包共用后端文件清单。 */
export function ApiKeyVarTemplateGuide() {
  const { t } = useTranslation();
  const {
    data: files,
    isPending,
    isFetching,
    error,
    refetch,
  } = useApiKeyVarTemplate();

  return (
    <div className="min-w-0 space-y-5 text-body text-fg-2">
      <div className="space-y-2">
        <p>{t("apiKeyVars.template.demoNotice")}</p>
        <ol className="list-decimal space-y-2 ps-5">
          <li>{t("apiKeyVars.template.stepExtract")}</li>
          <li>{t("apiKeyVars.template.stepRun")}</li>
          <li>{t("apiKeyVars.template.stepTest")}</li>
          <li>{t("apiKeyVars.template.stepConnect", { ref: "{{MY_KEY}}" })}</li>
        </ol>
      </div>

      <div className="space-y-2">
        <h3 className="font-medium text-fg-1">
          {t("apiKeyVars.template.contractTitle")}
        </h3>
        <p>{t("apiKeyVars.template.contractOutput")}</p>
        <p>{t("apiKeyVars.template.contractTime")}</p>
        <p>{t("apiKeyVars.template.contractSchedule")}</p>
        <p>{t("apiKeyVars.template.contractExit")}</p>
      </div>

      <div className="space-y-2">
        <h3 className="font-medium text-fg-1">
          {t("apiKeyVars.template.environmentTitle")}
        </h3>
        <dl className="space-y-2">
          {ENV_NAMES.map((name) => (
            <div key={name}>
              <dt className="break-all font-mono text-caption text-fg-1">
                {name}
              </dt>
              <dd className="text-caption">
                {t(`apiKeyVars.template.environment.${name}`)}
              </dd>
            </div>
          ))}
        </dl>
      </div>

      <div className="min-w-0 space-y-2">
        <h3 className="font-medium text-fg-1">
          {t("apiKeyVars.template.filesTitle")}
        </h3>
        {isPending && !error ? (
          <div role="status" className="flex items-center gap-2 py-3">
            <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
            {t("apiKeyVars.template.loading")}
          </div>
        ) : error ? (
          <div role="alert" className="space-y-2">
            <p className="break-words text-danger-text">
              {t("apiKeyVars.template.loadFailed", {
                error: extractErrorMessage(error),
              })}
            </p>
            <Button
              variant="neutral"
              size="compact"
              disabled={isFetching}
              onClick={() => void refetch()}
            >
              {isFetching && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
              {t("apiKeyVars.template.retry")}
            </Button>
          </div>
        ) : files?.length ? (
          <Tabs defaultValue={files[0].path} className="min-w-0">
            <div className="overflow-x-auto">
              <TabsList
                className="w-max justify-start"
                aria-label={t("apiKeyVars.template.filesTitle")}
              >
                {files.map((file) => (
                  <TabsTrigger
                    key={file.path}
                    value={file.path}
                    className="min-w-0 font-mono text-caption"
                  >
                    {file.path}
                  </TabsTrigger>
                ))}
              </TabsList>
            </div>
            {files.map((file) => (
              <TabsContent key={file.path} value={file.path}>
                <pre className="max-h-96 overflow-auto rounded-panel border border-border bg-subtle p-3 text-caption text-fg-1">
                  <code>{file.content}</code>
                </pre>
              </TabsContent>
            ))}
          </Tabs>
        ) : null}
      </div>
    </div>
  );
}

export function ApiKeyVarTemplateGuideDialog({
  open,
  onOpenChange,
  downloading,
  onDownload,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  downloading: boolean;
  onDownload: () => void;
}) {
  const { t } = useTranslation();
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent zIndex="nested" className="max-w-3xl">
        <DialogHeader>
          <DialogTitle>{t("apiKeyVars.template.title")}</DialogTitle>
          <DialogDescription>
            {t("apiKeyVars.template.intro")}
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 overflow-y-auto px-6 py-2">
          {open && <ApiKeyVarTemplateGuide />}
        </div>
        <DialogFooter>
          <Button
            variant="neutral"
            size="regular"
            disabled={downloading}
            onClick={onDownload}
          >
            {downloading ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Download className="h-3.5 w-3.5" />
            )}
            {t("apiKeyVars.template.download")}
          </Button>
          <Button
            variant="solid"
            size="regular"
            onClick={() => onOpenChange(false)}
          >
            {t("common.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
