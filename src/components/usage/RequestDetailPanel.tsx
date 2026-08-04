import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useRequestDetail } from "@/lib/query/usage";
import { useRequestDetailPayload } from "@/lib/api/detailCapture";
import { getFreshInputTokens, isUnpricedUsage } from "@/types/usage";

interface RequestDetailPanelProps {
  requestId: string;
  onClose: () => void;
}

export function RequestDetailPanel({
  requestId,
  onClose,
}: RequestDetailPanelProps) {
  const { t, i18n } = useTranslation();
  const { data: request, isLoading, error } = useRequestDetail(requestId);
  const { data: payload } = useRequestDetailPayload(requestId);
  const [activeTab, setActiveTab] = useState("overview");
  const dateLocale =
    i18n.language === "zh"
      ? "zh-CN"
      : i18n.language === "zh-TW"
        ? "zh-TW"
        : i18n.language === "ja"
          ? "ja-JP"
          : "en-US";

  if (isLoading) {
    return (
      <Dialog open onOpenChange={onClose}>
        <DialogContent className="max-w-3xl">
          <div className="h-[400px] animate-pulse rounded bg-gray-100" />
        </DialogContent>
      </Dialog>
    );
  }

  if (error) {
    return (
      <Dialog open onOpenChange={onClose}>
        <DialogContent className="max-w-3xl">
          <DialogHeader>
            <DialogTitle>{t("usage.requestDetail", "请求详情")}</DialogTitle>
          </DialogHeader>
          <div className="rounded-lg border border-red-200 bg-red-50 p-4">
            <h3 className="mb-2 font-semibold text-red-800">
              {t("usage.error", "错误")}
            </h3>
            <p className="text-sm text-red-700 font-mono whitespace-pre-wrap break-all">
              {error instanceof Error ? error.message : String(error)}
            </p>
          </div>
        </DialogContent>
      </Dialog>
    );
  }

  if (!request) {
    return (
      <Dialog open onOpenChange={onClose}>
        <DialogContent className="max-w-3xl">
          <DialogHeader>
            <DialogTitle>{t("usage.requestDetail", "请求详情")}</DialogTitle>
          </DialogHeader>
          <div className="text-center text-muted-foreground">
            {t("usage.requestNotFound", "请求未找到")}
          </div>
        </DialogContent>
      </Dialog>
    );
  }

  const freshInput = getFreshInputTokens(request);
  const isCacheInclusive = request.inputTokens !== freshInput;
  const unpriced = isUnpricedUsage(request);

  const hasRequestDetail = payload?.requestBody || payload?.requestHeaders;
  const hasResponseDetail = payload?.responseBody || payload?.responseHeaders;

  return (
    <Dialog open onOpenChange={onClose}>
      <DialogContent className="max-w-3xl max-h-[85vh] flex flex-col">
        <DialogHeader>
          <DialogTitle>{t("usage.requestDetail", "请求详情")}</DialogTitle>
        </DialogHeader>

        <Tabs
          value={activeTab}
          onValueChange={setActiveTab}
          className="flex-1 flex flex-col min-h-0"
        >
          <TabsList>
            <TabsTrigger value="overview">
              {t("usage.detailTabs.overview", "概览")}
            </TabsTrigger>
            <TabsTrigger value="request" disabled={!hasRequestDetail}>
              {t("usage.detailTabs.request", "请求详情")}
            </TabsTrigger>
            <TabsTrigger value="response" disabled={!hasResponseDetail}>
              {t("usage.detailTabs.response", "响应详情")}
            </TabsTrigger>
          </TabsList>

          {/* 概览 Tab */}
          <TabsContent value="overview" className="flex-1 overflow-y-auto min-h-0">
            <div className="space-y-4">
              {/* 基本信息 */}
              <div className="rounded-lg border p-4">
                <h3 className="mb-3 font-semibold">
                  {t("usage.basicInfo", "基本信息")}
                </h3>
                <dl className="grid grid-cols-2 gap-3 text-sm">
                  <div>
                    <dt className="text-muted-foreground">
                      {t("usage.requestId", "请求ID")}
                    </dt>
                    <dd className="font-mono">{request.requestId}</dd>
                  </div>
                  <div>
                    <dt className="text-muted-foreground">
                      {t("usage.time", "时间")}
                    </dt>
                    <dd>
                      {new Date(request.createdAt * 1000).toLocaleString(
                        dateLocale,
                      )}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-muted-foreground">
                      {t("usage.provider", "供应商")}
                    </dt>
                    <dd className="text-sm">
                      <span className="font-medium">
                        {request.providerName || t("usage.unknownProvider", "未知")}
                      </span>
                      <span className="ml-2 font-mono text-xs text-muted-foreground">
                        {request.providerId}
                      </span>
                    </dd>
                  </div>
                  <div>
                    <dt className="text-muted-foreground">
                      {t("usage.appType", "应用类型")}
                    </dt>
                    <dd>{request.appType}</dd>
                  </div>
                  <div>
                    <dt className="text-muted-foreground">
                      {t("usage.model", "模型")}
                    </dt>
                    <dd className="font-mono">{request.model}</dd>
                    {request.requestModel &&
                      request.requestModel !== request.model && (
                        <>
                          <dt className="mt-1 text-muted-foreground">
                            {t("usage.requestModel", "请求模型")}
                          </dt>
                          <dd className="font-mono text-xs">
                            {request.requestModel}
                          </dd>
                        </>
                      )}
                    {request.pricingModel &&
                      request.pricingModel !== request.model && (
                        <>
                          <dt className="mt-1 text-muted-foreground">
                            {t("usage.pricingModel", "计价模型")}
                          </dt>
                          <dd className="font-mono text-xs">
                            {request.pricingModel}
                          </dd>
                        </>
                      )}
                  </div>
                  <div>
                    <dt className="text-muted-foreground">
                      {t("usage.status", "状态")}
                    </dt>
                    <dd>
                      <span
                        className={`inline-flex rounded-full px-2 py-1 text-xs ${
                          request.statusCode >= 200 && request.statusCode < 300
                            ? "bg-green-100 text-green-800"
                            : "bg-red-100 text-red-800"
                        }`}
                      >
                        {request.statusCode}
                      </span>
                    </dd>
                  </div>
                </dl>
              </div>

              {/* Token 使用量 */}
              <div className="rounded-lg border p-4">
                <h3 className="mb-3 font-semibold">
                  {t("usage.tokenUsage", "Token 使用量")}
                </h3>
                <dl className="grid grid-cols-2 gap-3 text-sm">
                  <div>
                    <dt className="text-muted-foreground">
                      {t("usage.inputTokens", "输入 Tokens")}
                    </dt>
                    <dd className="font-mono">
                      {freshInput.toLocaleString()}
                      {isCacheInclusive && (
                        <span className="ml-2 text-xs text-muted-foreground/70 font-normal">
                          ({t("usage.rawInputLabel", "原始")}:{" "}
                          {request.inputTokens.toLocaleString()})
                        </span>
                      )}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-muted-foreground">
                      {t("usage.outputTokens", "输出 Tokens")}
                    </dt>
                    <dd className="font-mono">
                      {request.outputTokens.toLocaleString()}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-muted-foreground">
                      {t("usage.cacheReadTokens", "缓存读取")}
                    </dt>
                    <dd className="font-mono">
                      {request.cacheReadTokens.toLocaleString()}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-muted-foreground">
                      {t("usage.cacheCreationTokens", "缓存写入")}
                    </dt>
                    <dd className="font-mono">
                      {request.cacheCreationTokens.toLocaleString()}
                    </dd>
                  </div>
                  <div className="col-span-2">
                    <dt className="text-muted-foreground">
                      {t("usage.totalTokens", "总计")}
                    </dt>
                    <dd className="text-lg font-semibold">
                      {(freshInput + request.outputTokens).toLocaleString()}
                    </dd>
                  </div>
                </dl>
              </div>

              {/* 成本明细 */}
              <div className="rounded-lg border p-4">
                <h3 className="mb-3 font-semibold">
                  {t("usage.costBreakdown", "成本明细")}
                </h3>
                <dl className="grid grid-cols-2 gap-3 text-sm">
                  <div>
                    <dt className="text-muted-foreground">
                      {t("usage.inputCost", "输入成本")}
                      <span className="ml-1 text-xs">
                        ({t("usage.baseCost", "基础")})
                      </span>
                    </dt>
                    <dd className="font-mono">
                      ${parseFloat(request.inputCostUsd).toFixed(6)}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-muted-foreground">
                      {t("usage.outputCost", "输出成本")}
                      <span className="ml-1 text-xs">
                        ({t("usage.baseCost", "基础")})
                      </span>
                    </dt>
                    <dd className="font-mono">
                      ${parseFloat(request.outputCostUsd).toFixed(6)}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-muted-foreground">
                      {t("usage.cacheReadCost", "缓存读取成本")}
                      <span className="ml-1 text-xs">
                        ({t("usage.baseCost", "基础")})
                      </span>
                    </dt>
                    <dd className="font-mono">
                      ${parseFloat(request.cacheReadCostUsd).toFixed(6)}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-muted-foreground">
                      {t("usage.cacheCreationCost", "缓存写入成本")}
                      <span className="ml-1 text-xs">
                        ({t("usage.baseCost", "基础")})
                      </span>
                    </dt>
                    <dd className="font-mono">
                      ${parseFloat(request.cacheCreationCostUsd).toFixed(6)}
                    </dd>
                  </div>
                  {request.costMultiplier &&
                    parseFloat(request.costMultiplier) !== 1 && (
                      <div className="col-span-2 border-t pt-3">
                        <dt className="text-muted-foreground">
                          {t("usage.costMultiplier", "成本倍率")}
                        </dt>
                        <dd className="font-mono">×{request.costMultiplier}</dd>
                      </div>
                    )}
                  <div
                    className={`col-span-2 ${request.costMultiplier && parseFloat(request.costMultiplier) !== 1 ? "" : "border-t"} pt-3`}
                  >
                    <dt className="text-muted-foreground">
                      {t("usage.totalCost", "总成本")}
                      {request.costMultiplier &&
                        parseFloat(request.costMultiplier) !== 1 && (
                          <span className="ml-1 text-xs">
                            ({t("usage.withMultiplier", "含倍率")})
                          </span>
                        )}
                    </dt>
                    <dd
                      className={`text-lg font-semibold ${
                        unpriced ? "text-muted-foreground" : "text-primary"
                      }`}
                    >
                      {unpriced
                        ? t("usage.unpriced", "未定价")
                        : `$${parseFloat(request.totalCostUsd).toFixed(6)}`}
                    </dd>
                  </div>
                </dl>
              </div>

              {/* 性能信息 */}
              <div className="rounded-lg border p-4">
                <h3 className="mb-3 font-semibold">
                  {t("usage.performance", "性能信息")}
                </h3>
                <dl className="grid grid-cols-2 gap-3 text-sm">
                  <div>
                    <dt className="text-muted-foreground">
                      {t("usage.latency", "延迟")}
                    </dt>
                    <dd className="font-mono">{request.latencyMs}ms</dd>
                  </div>
                </dl>
              </div>

              {/* 错误信息 */}
              {request.errorMessage && (
                <div className="rounded-lg border border-red-200 bg-red-50 p-4">
                  <h3 className="mb-2 font-semibold text-red-800">
                    {t("usage.errorMessage", "错误信息")}
                  </h3>
                  <p className="text-sm text-red-700">{request.errorMessage}</p>
                </div>
              )}
            </div>
          </TabsContent>

          {/* 请求详情 Tab */}
          <TabsContent value="request" className="flex-1 overflow-y-auto min-h-0">
            <div className="space-y-4">
              {payload?.requestHeaders && (
                <div className="rounded-lg border p-4">
                  <h3 className="mb-3 font-semibold">
                    {t("usage.detailTabs.requestHeaders", "请求头")}
                  </h3>
                  <pre className="max-h-60 overflow-auto rounded bg-muted p-3 text-xs font-mono whitespace-pre-wrap break-all">
                    {payload.requestHeaders}
                  </pre>
                </div>
              )}
              {payload?.requestBody && (
                <div className="rounded-lg border p-4">
                  <h3 className="mb-3 font-semibold">
                    {t("usage.detailTabs.requestBody", "请求体")}
                  </h3>
                  <pre className="max-h-96 overflow-auto rounded bg-muted p-3 text-xs font-mono whitespace-pre-wrap break-all">
                    {formatJson(payload.requestBody)}
                  </pre>
                </div>
              )}
            </div>
          </TabsContent>

          {/* 响应详情 Tab */}
          <TabsContent value="response" className="flex-1 overflow-y-auto min-h-0">
            <div className="space-y-4">
              {payload?.responseHeaders && (
                <div className="rounded-lg border p-4">
                  <h3 className="mb-3 font-semibold">
                    {t("usage.detailTabs.responseHeaders", "响应头")}
                  </h3>
                  <pre className="max-h-60 overflow-auto rounded bg-muted p-3 text-xs font-mono whitespace-pre-wrap break-all">
                    {payload.responseHeaders}
                  </pre>
                </div>
              )}
              {payload?.responseBody && (
                <div className="rounded-lg border p-4">
                  <h3 className="mb-3 font-semibold">
                    {t("usage.detailTabs.responseBody", "响应体")}
                  </h3>
                  <pre className="max-h-96 overflow-auto rounded bg-muted p-3 text-xs font-mono whitespace-pre-wrap break-all">
                    {formatJson(payload.responseBody)}
                  </pre>
                </div>
              )}
            </div>
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}

/** Try to pretty-print a JSON string; fall back to raw text. */
function formatJson(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}
