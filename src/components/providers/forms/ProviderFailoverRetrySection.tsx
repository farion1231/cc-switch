import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  AlertTriangle,
  ChevronDown,
  ChevronRight,
  Plus,
  RotateCcw,
  Trash2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import type { FailoverRetryPolicy } from "@/types";
import {
  DEFAULT_FAILOVER_RETRY_POLICY,
  normalizeFailoverRetryPolicy,
  type NormalizedFailoverRetryPolicy,
} from "@/lib/failoverRetry";

interface ProviderFailoverRetrySectionProps {
  value?: FailoverRetryPolicy;
  onChange: (value: FailoverRetryPolicy) => void;
}

export function ProviderFailoverRetrySection({
  value,
  onChange,
}: ProviderFailoverRetrySectionProps) {
  const { t } = useTranslation();
  const currentPolicy = value ?? DEFAULT_FAILOVER_RETRY_POLICY;
  const normalizedValue = normalizeFailoverRetryPolicy(currentPolicy);
  const keywordValues =
    currentPolicy.nonRetryableKeywords ?? normalizedValue.nonRetryableKeywords;
  const [isOpen, setIsOpen] = useState(Boolean(value));

  useEffect(() => {
    if (value) {
      setIsOpen(true);
    }
  }, [value]);

  const updatePolicy = (
    patch:
      | Partial<FailoverRetryPolicy>
      | ((current: FailoverRetryPolicy) => Partial<FailoverRetryPolicy>),
  ) => {
    const current = value ?? DEFAULT_FAILOVER_RETRY_POLICY;
    const nextPatch = typeof patch === "function" ? patch(current) : patch;
    onChange({ ...current, ...nextPatch });
  };

  const summaryText =
    normalizedValue.mode === "infinite"
      ? t("providerRetry.summaryInfinite", {
          defaultValue: "Infinite retry · stays on the current provider",
        })
      : normalizedValue.maxRetries === 0
        ? t("providerRetry.summaryDirectFailover", {
            defaultValue: "Direct failover · no retry on the current provider",
          })
        : t("providerRetry.summaryFinite", {
            max: normalizedValue.maxRetries,
            defaultValue: "Retry {{max}} time(s) before failover",
          });

  return (
    <div className="rounded-lg border border-border/50 bg-muted/20">
      <button
        type="button"
        className="flex w-full items-center justify-between p-4 transition-colors hover:bg-muted/30"
        onClick={() => setIsOpen((open) => !open)}
      >
        <div className="flex items-center gap-3">
          <RotateCcw className="h-4 w-4 text-muted-foreground" />
          <div className="text-left">
            <div className="font-medium">
              {t("providerRetry.title", {
                defaultValue: "Failover Retry Policy",
              })}
            </div>
            <div className="text-xs text-muted-foreground">{summaryText}</div>
          </div>
        </div>
        {isOpen ? (
          <ChevronDown className="h-4 w-4 text-muted-foreground" />
        ) : (
          <ChevronRight className="h-4 w-4 text-muted-foreground" />
        )}
      </button>

      <div
        className={cn(
          "overflow-hidden transition-all duration-200",
          isOpen ? "max-h-[640px] opacity-100" : "max-h-0 opacity-0",
        )}
      >
        <div className="space-y-4 border-t border-border/50 p-4">
          <p className="text-sm text-muted-foreground">
            {t("providerRetry.description", {
              defaultValue:
                "Configure provider-level retries before moving to the next failover provider. Set retry count to 0 to fail over immediately without retrying the current provider. This policy is independent from proxy max retries.",
            })}
          </p>

          <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
            <div className="space-y-2">
              <Label htmlFor="provider-retry-mode">
                {t("providerRetry.mode", {
                  defaultValue: "Retry mode",
                })}
              </Label>
              <Select
                value={normalizedValue.mode}
                onValueChange={(nextMode) =>
                  updatePolicy({
                    mode: nextMode as NormalizedFailoverRetryPolicy["mode"],
                  })
                }
              >
                <SelectTrigger id="provider-retry-mode">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="finite">
                    {t("providerRetry.modeFinite", {
                      defaultValue: "Finite retries",
                    })}
                  </SelectItem>
                  <SelectItem value="infinite">
                    {t("providerRetry.modeInfinite", {
                      defaultValue: "Infinite retries",
                    })}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>

            {normalizedValue.mode === "finite" ? (
              <div className="space-y-2">
                <Label htmlFor="provider-retry-max">
                  {t("providerRetry.maxRetries", {
                    defaultValue: "Retry count",
                  })}
                </Label>
                <Input
                  id="provider-retry-max"
                  type="number"
                  min={0}
                  value={normalizedValue.maxRetries}
                  onChange={(event) =>
                    updatePolicy({
                      maxRetries: parseNumber(
                        event.target.value,
                        normalizedValue.maxRetries,
                        0,
                      ),
                    })
                  }
                />
                <p className="text-xs text-muted-foreground">
                  {t("providerRetry.maxRetriesHint", {
                    defaultValue:
                      "Set to 0 to fail over immediately without retrying the current provider.",
                  })}
                </p>
              </div>
            ) : (
              <div className="rounded-md border border-amber-200 bg-amber-50/80 p-3 text-sm text-amber-900 dark:border-amber-500/30 dark:bg-amber-500/10 dark:text-amber-100">
                <div className="flex items-start gap-2">
                  <AlertTriangle className="mt-0.5 h-4 w-4 flex-shrink-0" />
                  <p>
                    {t("providerRetry.infiniteWarning", {
                      defaultValue:
                        "Infinite retry will stay on this provider until success. It will not auto-fallback to the next provider.",
                    })}
                  </p>
                </div>
              </div>
            )}

            <div className="space-y-2">
              <Label htmlFor="provider-retry-base-delay">
                {t("providerRetry.baseDelaySeconds", {
                  defaultValue: "Initial delay (seconds)",
                })}
              </Label>
              <Input
                id="provider-retry-base-delay"
                type="number"
                min={1}
                value={normalizedValue.baseDelaySeconds}
                onChange={(event) =>
                  updatePolicy((current) => {
                    const baseDelaySeconds = parseNumber(
                      event.target.value,
                      current.baseDelaySeconds ??
                        normalizedValue.baseDelaySeconds,
                      1,
                    );

                    return {
                      baseDelaySeconds,
                      maxDelaySeconds: Math.max(
                        current.maxDelaySeconds ??
                          normalizedValue.maxDelaySeconds,
                        baseDelaySeconds,
                      ),
                    };
                  })
                }
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="provider-retry-max-delay">
                {t("providerRetry.maxDelaySeconds", {
                  defaultValue: "Delay cap (seconds)",
                })}
              </Label>
              <Input
                id="provider-retry-max-delay"
                type="number"
                min={normalizedValue.baseDelaySeconds}
                value={normalizedValue.maxDelaySeconds}
                onChange={(event) =>
                  updatePolicy({
                    maxDelaySeconds: parseNumber(
                      event.target.value,
                      normalizedValue.maxDelaySeconds,
                      normalizedValue.baseDelaySeconds,
                    ),
                  })
                }
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="provider-retry-multiplier">
                {t("providerRetry.backoffMultiplier", {
                  defaultValue: "Backoff multiplier",
                })}
              </Label>
              <Input
                id="provider-retry-multiplier"
                type="number"
                min={1}
                step="0.1"
                inputMode="decimal"
                value={normalizedValue.backoffMultiplier}
                onChange={(event) =>
                  updatePolicy({
                    backoffMultiplier: parseDecimal(
                      event.target.value,
                      normalizedValue.backoffMultiplier,
                      1,
                    ),
                  })
                }
              />
              <p className="text-xs text-muted-foreground">
                {t("providerRetry.backoffHint", {
                  defaultValue:
                    "Each retry waits longer than the last one until the delay cap is reached.",
                })}
              </p>
            </div>

            <div className="space-y-3 md:col-span-2 rounded-lg border border-destructive/20 bg-destructive/5 p-3">
              <div className="space-y-1">
                <Label htmlFor="provider-retry-non-retryable-0">
                  {t("providerRetry.nonRetryableKeywords", {
                    defaultValue: "Non-retryable keywords",
                  })}
                </Label>
                <p className="text-xs text-muted-foreground">
                  {t("providerRetry.nonRetryableKeywordsHint", {
                    defaultValue:
                      "Matches error message, type, code, and status after case-insensitive normalization. If a keyword matches, this provider is skipped and failover moves to the next one. Each provider keeps its own list.",
                  })}
                </p>
              </div>

              <div className="space-y-2">
                {keywordValues.map((keyword, index) => (
                  <div
                    key={`${index}-${keyword}`}
                    className="flex items-center gap-2"
                  >
                    <Input
                      id={`provider-retry-non-retryable-${index}`}
                      aria-label={t("providerRetry.nonRetryableKeyword", {
                        index: index + 1,
                        defaultValue: "Non-retryable keyword {{index}}",
                      })}
                      value={keyword}
                      placeholder={t(
                        "providerRetry.nonRetryableKeywordPlaceholder",
                        {
                          defaultValue: "invalid_api_key",
                        },
                      )}
                      onChange={(event) =>
                        updatePolicy({
                          nonRetryableKeywords: keywordValues.map(
                            (item, itemIndex) =>
                              itemIndex === index ? event.target.value : item,
                          ),
                        })
                      }
                    />
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      onClick={() =>
                        updatePolicy({
                          nonRetryableKeywords: keywordValues.filter(
                            (_, itemIndex) => itemIndex !== index,
                          ),
                        })
                      }
                      aria-label={t("providerRetry.removeNonRetryableKeyword", {
                        index: index + 1,
                        defaultValue: "Remove keyword {{index}}",
                      })}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                ))}

                {keywordValues.length === 0 && (
                  <p className="text-xs text-muted-foreground">
                    {t("providerRetry.nonRetryableKeywordsEmpty", {
                      defaultValue:
                        "No keywords yet. Add one to skip this provider when a matching error appears.",
                    })}
                  </p>
                )}

                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="w-fit"
                  onClick={() =>
                    updatePolicy({
                      nonRetryableKeywords: [...keywordValues, ""],
                    })
                  }
                >
                  <Plus className="h-4 w-4" />
                  {t("providerRetry.addNonRetryableKeyword", {
                    defaultValue: "Add keyword",
                  })}
                </Button>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function parseNumber(value: string, fallback: number, min: number): number {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isFinite(parsed)) {
    return fallback;
  }
  return Math.max(parsed, min);
}

function parseDecimal(value: string, fallback: number, min: number): number {
  const parsed = Number.parseFloat(value);
  if (!Number.isFinite(parsed)) {
    return fallback;
  }
  return Math.max(parsed, min);
}
