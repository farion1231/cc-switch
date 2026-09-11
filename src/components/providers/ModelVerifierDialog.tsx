import { useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  Loader2,
  RefreshCw,
  ShieldCheck,
  X,
  XCircle,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import type { AppId } from "@/lib/api";
import {
  modelVerifierApi,
  type ModelVerificationResult,
  type VerificationVerdict,
} from "@/lib/api/modelVerifier";
import type { Provider } from "@/types";

interface ModelVerifierDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  appId: AppId;
  provider: Provider;
}

const verdictTone = (verdict: VerificationVerdict) => {
  switch (verdict) {
    case "officialEndpointConsistent":
    case "reportedExactMatch":
    case "reportedFamilyMatch":
      return "text-emerald-600 dark:text-emerald-400";
    case "reportedMismatch":
    case "requestFailed":
      return "text-red-600 dark:text-red-400";
    default:
      return "text-amber-600 dark:text-amber-400";
  }
};

const verdictIcon = (verdict: VerificationVerdict) => {
  switch (verdict) {
    case "officialEndpointConsistent":
    case "reportedExactMatch":
    case "reportedFamilyMatch":
      return CheckCircle2;
    case "reportedMismatch":
    case "requestFailed":
      return XCircle;
    default:
      return AlertTriangle;
  }
};

export function ModelVerifierDialog({
  open,
  onOpenChange,
  appId,
  provider,
}: ModelVerifierDialogProps) {
  const { t } = useTranslation();
  const [result, setResult] = useState<ModelVerificationResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);

  const verify = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      const next = await modelVerifierApi.verifyProvider(appId, provider);
      setResult(next);
    } catch (err) {
      setResult(null);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsLoading(false);
    }
  }, [appId, provider]);

  useEffect(() => {
    if (!open) return;
    setResult(null);
    setError(null);
    void verify();
  }, [open, provider.id, verify]);

  const verdictLabel = useMemo(() => {
    if (!result) return "";
    switch (result.verdict) {
      case "officialEndpointConsistent":
        return t("modelVerifier.verdict.official", {
          defaultValue: "官方端点证据一致",
        });
      case "reportedExactMatch":
        return t("modelVerifier.verdict.exact", {
          defaultValue: "第三方声明完全一致",
        });
      case "reportedFamilyMatch":
        return t("modelVerifier.verdict.family", {
          defaultValue: "第三方声明为同一模型系列",
        });
      case "reportedMismatch":
        return t("modelVerifier.verdict.mismatch", {
          defaultValue: "服务端声明模型不一致",
        });
      case "requestFailed":
        return t("modelVerifier.verdict.failed", {
          defaultValue: "真实请求失败",
        });
      default:
        return t("modelVerifier.verdict.insufficient", {
          defaultValue: "证据不足",
        });
    }
  }, [result, t]);

  const VerdictIcon = result ? verdictIcon(result.verdict) : ShieldCheck;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl" zIndex="alert">
        <DialogHeader className="relative pr-16">
          <DialogTitle className="flex items-center gap-2">
            <ShieldCheck className="h-5 w-5" />
            {t("modelVerifier.title", { defaultValue: "模型核验" })}
          </DialogTitle>
          <DialogDescription>
            {t("modelVerifier.description", {
              defaultValue:
                "通过 CC Switch 现有的安全请求通道发送一次极小真实 API 请求，比较配置模型与服务端返回的模型标识。",
            })}
          </DialogDescription>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="absolute right-4 top-4 h-8 w-8"
            onClick={() => onOpenChange(false)}
            aria-label={t("common.close", { defaultValue: "关闭" })}
            title={t("common.close", { defaultValue: "关闭" })}
          >
            <X className="h-4 w-4" />
          </Button>
        </DialogHeader>

        <div className="min-h-0 flex-1 space-y-4 overflow-y-auto py-2 pr-2">
          {isLoading && (
            <div className="flex min-h-48 flex-col items-center justify-center gap-3 text-muted-foreground">
              <Loader2 className="h-7 w-7 animate-spin" />
              <span>
                {t("modelVerifier.running", {
                  defaultValue: "正在核验真实请求…",
                })}
              </span>
            </div>
          )}

          {!isLoading && error && (
            <div className="rounded-lg border border-red-500/30 bg-red-500/5 p-4 text-sm">
              <div className="mb-2 flex items-center gap-2 font-medium text-red-600 dark:text-red-400">
                <XCircle className="h-4 w-4" />
                {t("modelVerifier.error", { defaultValue: "核验失败" })}
              </div>
              <div className="break-words text-muted-foreground">{error}</div>
            </div>
          )}

          {!isLoading && result && (
            <>
              <div className="flex flex-col gap-3 rounded-lg border border-border bg-muted/30 p-4 sm:flex-row sm:items-center sm:justify-between">
                <div className="flex items-center gap-3">
                  <VerdictIcon
                    className={cn(
                      "h-6 w-6 shrink-0",
                      verdictTone(result.verdict),
                    )}
                  />
                  <div>
                    <div
                      className={cn(
                        "font-semibold",
                        verdictTone(result.verdict),
                      )}
                    >
                      {verdictLabel}
                    </div>
                    <div className="mt-1 text-xs text-muted-foreground">
                      {t("modelVerifier.score", {
                        defaultValue: "证据一致性评分",
                      })}
                      :{" "}
                      <span className="font-mono font-semibold">
                        {result.confidenceScore}/100
                      </span>
                    </div>
                  </div>
                </div>
                <div className="text-left text-xs text-muted-foreground sm:text-right">
                  {result.success
                    ? t("modelVerifier.requestSuccess", {
                        defaultValue: "请求成功",
                      })
                    : t("modelVerifier.requestFailed", {
                        defaultValue: "请求失败",
                      })}
                  <br />
                  {result.responseTimeMs} ms
                </div>
              </div>

              <div className="grid gap-2 text-sm sm:grid-cols-[10rem_1fr]">
                <Field
                  label={t("modelVerifier.requestedModel", {
                    defaultValue: "配置/请求模型",
                  })}
                  value={result.requestedModel}
                  mono
                />
                <Field
                  label={t("modelVerifier.reportedModel", {
                    defaultValue: "服务端返回模型",
                  })}
                  value={result.reportedModel || "—"}
                  mono
                />
                <Field
                  label={t("modelVerifier.protocol", { defaultValue: "协议" })}
                  value={result.protocol}
                  mono
                />
                <Field
                  label={t("modelVerifier.endpoint", {
                    defaultValue: "请求端点",
                  })}
                  value={result.endpoint}
                  mono
                />
                <Field
                  label="response.id"
                  value={result.responseId || "—"}
                  mono
                />
                <Field
                  label="response.object"
                  value={result.responseObject || "—"}
                  mono
                />
                <Field
                  label="system_fingerprint"
                  value={result.systemFingerprint || "—"}
                  mono
                />
                <Field
                  label="usage"
                  value={result.hasUsage ? "present" : "—"}
                  mono
                />
              </div>

              {result.error && (
                <div className="rounded-lg border border-red-500/30 bg-red-500/5 p-3 text-sm text-red-700 dark:text-red-300">
                  {result.error}
                </div>
              )}

              <div className="space-y-2">
                <div className="text-sm font-medium">
                  {t("modelVerifier.evidence", { defaultValue: "核验证据" })}
                </div>
                <div className="divide-y divide-border rounded-lg border border-border">
                  {result.evidence.map((item) => (
                    <div
                      key={item.id}
                      className="flex gap-3 px-3 py-2.5 text-sm"
                    >
                      {item.passed ? (
                        <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0 text-emerald-500" />
                      ) : (
                        <XCircle className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
                      )}
                      <div className="min-w-0 flex-1">
                        <div className="break-words">{item.summary}</div>
                        <div className="mt-0.5 text-xs text-muted-foreground">
                          weight {item.weight}
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              </div>

              <div className="rounded-lg border border-amber-500/30 bg-amber-500/5 p-3 text-xs leading-relaxed text-amber-800 dark:text-amber-200">
                <div className="flex gap-2">
                  <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
                  <span>
                    {result.officialOpenaiEndpoint
                      ? t("modelVerifier.officialWarning", {
                          defaultValue:
                            "请求目标为 api.openai.com，证据强度较高；model 字段仍是服务端元数据，不是对底层模型权重的密码学证明。",
                        })
                      : t("modelVerifier.thirdPartyWarning", {
                          defaultValue:
                            "第三方 API 可以伪造 model、id、system_fingerprint 等响应字段，因此这里衡量的是证据一致性，不能 100% 证明底层实际模型。第三方评分最高限制为 74。",
                        })}
                  </span>
                </div>
              </div>
            </>
          )}
        </div>

        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            onClick={() => void verify()}
            disabled={isLoading}
          >
            {isLoading ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <RefreshCw className="h-4 w-4" />
            )}
            {t("modelVerifier.rerun", { defaultValue: "重新核验" })}
          </Button>
          <Button type="button" onClick={() => onOpenChange(false)}>
            {t("common.close", { defaultValue: "关闭" })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function Field({
  label,
  value,
  mono = false,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <>
      <div className="text-muted-foreground">{label}</div>
      <div className={cn("min-w-0 break-all", mono && "font-mono text-xs")}>
        {value}
      </div>
    </>
  );
}
