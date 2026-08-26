import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Save, Loader2, Info, Plus, Trash2, CircleHelp } from "lucide-react";
import { toast } from "sonner";
import { useAppProxyConfig, useUpdateAppProxyConfig } from "@/lib/query/proxy";
import type { AppProxyConfig, ProxyRetryRule } from "@/types/proxy";

export interface AutoFailoverConfigPanelProps {
  appType: string;
  disabled?: boolean;
}

interface RetryRuleDraft {
  id: number;
  enabled: boolean;
  statusCodes: string;
  errorCodes: string;
  messageContains: string;
  retryCount: string;
  backoffStrategy: "exponential" | "fixed";
  maxDelaySeconds: string;
}

let nextRetryRuleId = 0;

const emptyRetryRule = (): RetryRuleDraft => ({
  id: nextRetryRuleId++,
  enabled: true,
  statusCodes: "",
  errorCodes: "",
  messageContains: "",
  retryCount: "3",
  backoffStrategy: "exponential",
  maxDelaySeconds: "15",
});

const toRetryRuleDraft = (rule: ProxyRetryRule): RetryRuleDraft => ({
  id: nextRetryRuleId++,
  enabled: rule.enabled,
  statusCodes: rule.statusCodes.join(", "),
  errorCodes: rule.errorCodes.join(", "),
  messageContains: rule.messageContains ?? "",
  retryCount: String(rule.retryCount),
  backoffStrategy: rule.backoffStrategy ?? "exponential",
  maxDelaySeconds: String(rule.maxDelaySeconds ?? 15),
});

const createFormData = (config?: AppProxyConfig) => ({
  maxRetries: String(config?.maxRetries ?? 3),
  retryRules: (config?.retryRules ?? []).map(toRetryRuleDraft),
  streamingFirstByteTimeout: String(config?.streamingFirstByteTimeout ?? 60),
  streamingIdleTimeout: String(config?.streamingIdleTimeout ?? 120),
  nonStreamingTimeout: String(config?.nonStreamingTimeout ?? 600),
  circuitFailureThreshold: String(config?.circuitFailureThreshold ?? 5),
  circuitSuccessThreshold: String(config?.circuitSuccessThreshold ?? 2),
  circuitTimeoutSeconds: String(config?.circuitTimeoutSeconds ?? 60),
  circuitErrorRateThreshold: String(
    Math.round((config?.circuitErrorRateThreshold ?? 0.5) * 100),
  ),
  circuitMinRequests: String(config?.circuitMinRequests ?? 10),
});

const splitValues = (value: string) =>
  [...new Set(value.split(/[\s,，;；]+/).map((item) => item.trim()))].filter(
    Boolean,
  );

export function AutoFailoverConfigPanel({
  appType,
  disabled = false,
}: AutoFailoverConfigPanelProps) {
  const { t } = useTranslation();
  const { data: config, isLoading, error } = useAppProxyConfig(appType);
  const updateConfig = useUpdateAppProxyConfig();

  // 使用字符串状态以支持完全清空数字输入框
  const [formData, setFormData] = useState(createFormData);

  useEffect(() => {
    if (config) {
      setFormData(createFormData(config));
    }
  }, [config]);

  const updateRetryRule = (index: number, changes: Partial<RetryRuleDraft>) => {
    setFormData((current) => ({
      ...current,
      retryRules: current.retryRules.map((rule, ruleIndex) =>
        ruleIndex === index ? { ...rule, ...changes } : rule,
      ),
    }));
  };

  const handleSave = async () => {
    if (!config) return;
    // 解析数字，返回 NaN 表示无效输入
    const parseNum = (val: string) => {
      const trimmed = val.trim();
      // 必须是纯数字
      if (!/^-?\d+$/.test(trimmed)) return NaN;
      return parseInt(trimmed);
    };

    // 定义各字段的有效范围
    const ranges = {
      maxRetries: { min: 0, max: 10 },
      streamingFirstByteTimeout: { min: 1, max: 120 },
      streamingIdleTimeout: { min: 0, max: 600 },
      nonStreamingTimeout: { min: 60, max: 1200 },
      circuitFailureThreshold: { min: 1, max: 20 },
      circuitSuccessThreshold: { min: 1, max: 10 },
      circuitTimeoutSeconds: { min: 0, max: 300 },
      circuitErrorRateThreshold: { min: 0, max: 100 },
      circuitMinRequests: { min: 5, max: 100 },
    };

    // 解析原始值
    const raw = {
      maxRetries: parseNum(formData.maxRetries),
      streamingFirstByteTimeout: parseNum(formData.streamingFirstByteTimeout),
      streamingIdleTimeout: parseNum(formData.streamingIdleTimeout),
      nonStreamingTimeout: parseNum(formData.nonStreamingTimeout),
      circuitFailureThreshold: parseNum(formData.circuitFailureThreshold),
      circuitSuccessThreshold: parseNum(formData.circuitSuccessThreshold),
      circuitTimeoutSeconds: parseNum(formData.circuitTimeoutSeconds),
      circuitErrorRateThreshold: parseNum(formData.circuitErrorRateThreshold),
      circuitMinRequests: parseNum(formData.circuitMinRequests),
    };

    // 校验是否超出范围（NaN 也视为无效）
    const errors: string[] = [];
    const retryRules: ProxyRetryRule[] = [];
    formData.retryRules.forEach((rule, index) => {
      const statusCodeValues = splitValues(rule.statusCodes);
      const statusCodes = statusCodeValues.map(Number);
      const errorCodes = splitValues(rule.errorCodes);
      const messageContains = rule.messageContains.trim();
      const retryCount = parseNum(rule.retryCount);
      const maxDelaySeconds = parseNum(rule.maxDelaySeconds);
      const isValid =
        statusCodeValues.every((code) => /^\d+$/.test(code)) &&
        statusCodes.every(
          (code) => Number.isInteger(code) && code >= 100 && code <= 599,
        ) &&
        errorCodes.every((code) => code.length <= 100) &&
        messageContains.length <= 500 &&
        Number.isInteger(retryCount) &&
        retryCount >= 0 &&
        retryCount <= 10 &&
        Number.isInteger(maxDelaySeconds) &&
        maxDelaySeconds >= 1 &&
        maxDelaySeconds <= 15 &&
        (statusCodes.length > 0 ||
          errorCodes.length > 0 ||
          messageContains.length > 0);

      if (!isValid) {
        errors.push(
          t("proxy.autoFailover.retryRuleInvalid", {
            index: index + 1,
            defaultValue: `重试规则 ${index + 1}`,
          }),
        );
        return;
      }

      retryRules.push({
        enabled: rule.enabled,
        statusCodes,
        errorCodes,
        messageContains: messageContains || null,
        retryCount,
        backoffStrategy: rule.backoffStrategy,
        maxDelaySeconds,
      });
    });
    const checkRange = (
      value: number,
      range: { min: number; max: number },
      label: string,
    ) => {
      if (isNaN(value) || value < range.min || value > range.max) {
        errors.push(`${label}: ${range.min}-${range.max}`);
      }
    };

    checkRange(
      raw.maxRetries,
      ranges.maxRetries,
      t("proxy.autoFailover.maxRetries", "最大重试次数"),
    );
    checkRange(
      raw.streamingFirstByteTimeout,
      ranges.streamingFirstByteTimeout,
      t("proxy.autoFailover.streamingFirstByte", "流式首字节超时"),
    );
    checkRange(
      raw.streamingIdleTimeout,
      ranges.streamingIdleTimeout,
      t("proxy.autoFailover.streamingIdle", "流式静默超时"),
    );
    checkRange(
      raw.nonStreamingTimeout,
      ranges.nonStreamingTimeout,
      t("proxy.autoFailover.nonStreaming", "非流式超时"),
    );
    checkRange(
      raw.circuitFailureThreshold,
      ranges.circuitFailureThreshold,
      t("proxy.autoFailover.failureThreshold", "失败阈值"),
    );
    checkRange(
      raw.circuitSuccessThreshold,
      ranges.circuitSuccessThreshold,
      t("proxy.autoFailover.successThreshold", "恢复成功阈值"),
    );
    checkRange(
      raw.circuitTimeoutSeconds,
      ranges.circuitTimeoutSeconds,
      t("proxy.autoFailover.timeout", "恢复等待时间"),
    );
    checkRange(
      raw.circuitErrorRateThreshold,
      ranges.circuitErrorRateThreshold,
      t("proxy.autoFailover.errorRate", "错误率阈值"),
    );
    checkRange(
      raw.circuitMinRequests,
      ranges.circuitMinRequests,
      t("proxy.autoFailover.minRequests", "最小请求数"),
    );

    if (errors.length > 0) {
      toast.error(
        t("proxy.autoFailover.validationFailed", {
          fields: errors.join("; "),
          defaultValue: `以下字段超出有效范围: ${errors.join("; ")}`,
        }),
      );
      return;
    }

    try {
      await updateConfig.mutateAsync({
        appType,
        enabled: config.enabled,
        // 后端详细配置更新会保留当前开关值，避免覆盖相邻组件的修改。
        autoFailoverEnabled: config.autoFailoverEnabled,
        maxRetries: raw.maxRetries,
        retryRules,
        streamingFirstByteTimeout: raw.streamingFirstByteTimeout,
        streamingIdleTimeout: raw.streamingIdleTimeout,
        nonStreamingTimeout: raw.nonStreamingTimeout,
        circuitFailureThreshold: raw.circuitFailureThreshold,
        circuitSuccessThreshold: raw.circuitSuccessThreshold,
        circuitTimeoutSeconds: raw.circuitTimeoutSeconds,
        circuitErrorRateThreshold: raw.circuitErrorRateThreshold / 100,
        circuitMinRequests: raw.circuitMinRequests,
      });
    } catch {
      // Mutation hook 统一展示保存失败提示。
    }
  };

  const handleReset = () => {
    if (config) {
      setFormData(createFormData(config));
    }
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center p-4">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  const isDisabled = disabled || updateConfig.isPending;

  return (
    <div className="border-0 rounded-none shadow-none bg-transparent">
      <div className="space-y-4">
        {error && (
          <Alert variant="destructive">
            <AlertDescription>{String(error)}</AlertDescription>
          </Alert>
        )}

        <Alert className="border-blue-500/40 bg-blue-500/10">
          <Info className="h-4 w-4" />
          <AlertDescription className="text-sm">
            {t(
              "proxy.autoFailover.info",
              "当故障转移队列中配置了多个供应商时，系统会在请求失败时按优先级顺序依次尝试。当某个供应商连续失败达到阈值时，熔断器会打开并在一段时间内跳过该供应商。",
            )}
          </AlertDescription>
        </Alert>

        {/* 重试与超时配置 */}
        <div className="space-y-4 rounded-lg border border-white/10 bg-muted/30 p-4">
          <div className="flex items-center gap-1.5">
            <h4 className="text-sm font-semibold">
              {t("proxy.autoFailover.retrySettings", "重试与超时设置")}
            </h4>
            <Popover>
              <PopoverTrigger asChild>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7 text-muted-foreground hover:text-foreground"
                  aria-label={t(
                    "proxy.autoFailover.retryRelationshipHelp",
                    "查看重试次数说明",
                  )}
                >
                  <CircleHelp className="h-4 w-4" />
                </Button>
              </PopoverTrigger>
              <PopoverContent
                align="start"
                side="bottom"
                collisionPadding={12}
                className="w-[min(26rem,calc(100vw-2rem))] space-y-4 p-4"
              >
                <div className="space-y-1">
                  <h5 className="text-sm font-semibold">
                    {t(
                      "proxy.autoFailover.retryRelationshipTitle",
                      "两类重试如何配合",
                    )}
                  </h5>
                  <p className="text-xs text-muted-foreground">
                    {t(
                      "proxy.autoFailover.retryRelationshipIntro",
                      "它们是嵌套关系，分别控制供应商之间和单个供应商内部的重试。",
                    )}
                  </p>
                </div>

                <div className="space-y-3 text-xs leading-relaxed">
                  <div>
                    <p className="font-medium text-foreground">
                      {t(
                        "proxy.autoFailover.maxRetriesHelpTitle",
                        "最大重试次数",
                      )}
                    </p>
                    <p className="text-muted-foreground">
                      {t(
                        "proxy.autoFailover.maxRetriesHelpBody",
                        "控制供应商之间的故障转移。当前供应商最终失败后，最多再尝试多少个供应商；关闭自动故障转移时不生效。",
                      )}
                    </p>
                  </div>
                  <div>
                    <p className="font-medium text-foreground">
                      {t(
                        "proxy.autoFailover.extraRetriesHelpTitle",
                        "额外重试次数",
                      )}
                    </p>
                    <p className="text-muted-foreground">
                      {t(
                        "proxy.autoFailover.extraRetriesHelpBody",
                        "控制当前供应商内部的原地重试。只有命中特定错误规则时才执行；即使关闭自动故障转移也可以生效。",
                      )}
                    </p>
                  </div>
                </div>

                <div className="rounded-md border border-border bg-muted/40 p-3 text-xs">
                  <p className="mb-1 font-medium">
                    {t("proxy.autoFailover.retryOrderTitle", "执行顺序")}
                  </p>
                  <p className="text-muted-foreground">
                    {t(
                      "proxy.autoFailover.retryOrderBody",
                      "请求供应商 A → 命中规则后按额外重试次数继续请求 A → 仍失败后按最大重试次数切换到 B、C……",
                    )}
                  </p>
                </div>

                <div className="space-y-1 text-xs">
                  <p className="font-medium">
                    {t("proxy.autoFailover.retryExampleTitle", "计算示例")}
                  </p>
                  <p className="text-muted-foreground">
                    {t(
                      "proxy.autoFailover.retryExampleBody",
                      "最大重试次数为 2、额外重试次数为 3 时，最多尝试 3 个供应商；若每次都命中规则，每个供应商最多请求 4 次，最坏共请求 12 次。",
                    )}
                  </p>
                </div>
              </PopoverContent>
            </Popover>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor={`maxRetries-${appType}`}>
                {t("proxy.autoFailover.maxRetries", "最大重试次数")}
              </Label>
              <Input
                id={`maxRetries-${appType}`}
                type="number"
                min="0"
                max="10"
                value={formData.maxRetries}
                onChange={(e) =>
                  setFormData({ ...formData, maxRetries: e.target.value })
                }
                disabled={isDisabled}
              />
              <p className="text-xs text-muted-foreground">
                {t(
                  "proxy.autoFailover.maxRetriesHint",
                  "自动故障转移时，最多继续尝试的供应商数量（0-10）",
                )}
              </p>
            </div>

            <div className="space-y-2">
              <Label htmlFor={`failureThreshold-${appType}`}>
                {t("proxy.autoFailover.failureThreshold", "失败阈值")}
              </Label>
              <Input
                id={`failureThreshold-${appType}`}
                type="number"
                min="1"
                max="20"
                value={formData.circuitFailureThreshold}
                onChange={(e) =>
                  setFormData({
                    ...formData,
                    circuitFailureThreshold: e.target.value,
                  })
                }
                disabled={isDisabled}
              />
              <p className="text-xs text-muted-foreground">
                {t(
                  "proxy.autoFailover.failureThresholdHint",
                  "连续失败多少次后打开熔断器（建议: 3-10）",
                )}
              </p>
            </div>
          </div>

          <div className="space-y-3">
            <div className="flex items-start justify-between gap-4">
              <div className="space-y-1">
                <Label>
                  {t("proxy.autoFailover.retryRules", "特定错误重试规则")}
                </Label>
                <p className="text-xs text-muted-foreground">
                  {t(
                    "proxy.autoFailover.retryRulesHint",
                    "同一规则中填写的条件按 AND 匹配；如需匹配任一错误，请分别添加多条规则。",
                  )}
                </p>
              </div>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() =>
                  setFormData((current) => ({
                    ...current,
                    retryRules: [...current.retryRules, emptyRetryRule()],
                  }))
                }
                disabled={isDisabled || formData.retryRules.length >= 20}
              >
                <Plus className="mr-1.5 h-4 w-4" />
                {t("proxy.autoFailover.addRetryRule", "添加规则")}
              </Button>
            </div>

            {formData.retryRules.length === 0 ? (
              <div className="rounded-md border border-dashed border-border p-5 text-center text-sm text-muted-foreground">
                {t("proxy.autoFailover.noRetryRules", "暂无特定错误重试规则。")}
              </div>
            ) : (
              <div className="space-y-3">
                {formData.retryRules.map((rule, index) => (
                  <div
                    key={rule.id}
                    className="space-y-3 rounded-md border border-border bg-background/60 p-3"
                  >
                    <div className="flex items-center justify-between gap-3">
                      <div className="flex items-center gap-3">
                        <Switch
                          aria-label={t("proxy.autoFailover.ruleEnabled", {
                            index: index + 1,
                            defaultValue: `启用重试规则 ${index + 1}`,
                          })}
                          checked={rule.enabled}
                          onCheckedChange={(enabled) =>
                            updateRetryRule(index, { enabled })
                          }
                          disabled={isDisabled}
                        />
                        <span className="text-sm font-medium">
                          {t("proxy.autoFailover.retryRuleTitle", {
                            index: index + 1,
                            defaultValue: `规则 ${index + 1}`,
                          })}
                        </span>
                      </div>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        className="h-8 w-8 text-muted-foreground hover:text-destructive"
                        onClick={() =>
                          setFormData((current) => ({
                            ...current,
                            retryRules: current.retryRules.filter(
                              (_, ruleIndex) => ruleIndex !== index,
                            ),
                          }))
                        }
                        disabled={isDisabled}
                        aria-label={t("proxy.autoFailover.deleteRetryRule", {
                          index: index + 1,
                          defaultValue: `删除重试规则 ${index + 1}`,
                        })}
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </div>

                    <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
                      <div className="space-y-1.5">
                        <Label htmlFor={`retry-status-${appType}-${index}`}>
                          {t("proxy.autoFailover.statusCodes", "HTTP 状态码")}
                        </Label>
                        <Input
                          id={`retry-status-${appType}-${index}`}
                          value={rule.statusCodes}
                          onChange={(event) =>
                            updateRetryRule(index, {
                              statusCodes: event.target.value,
                            })
                          }
                          onBlur={() =>
                            updateRetryRule(index, {
                              statusCodes: splitValues(rule.statusCodes).join(
                                ", ",
                              ),
                            })
                          }
                          placeholder={t(
                            "proxy.autoFailover.statusCodesPlaceholder",
                            "例如：429, 503",
                          )}
                          disabled={isDisabled}
                        />
                      </div>
                      <div className="space-y-1.5">
                        <Label htmlFor={`retry-error-${appType}-${index}`}>
                          {t("proxy.autoFailover.errorCodes", "错误码")}
                        </Label>
                        <Input
                          id={`retry-error-${appType}-${index}`}
                          value={rule.errorCodes}
                          onChange={(event) =>
                            updateRetryRule(index, {
                              errorCodes: event.target.value,
                            })
                          }
                          onBlur={() =>
                            updateRetryRule(index, {
                              errorCodes: splitValues(rule.errorCodes).join(
                                ", ",
                              ),
                            })
                          }
                          placeholder={t(
                            "proxy.autoFailover.errorCodesPlaceholder",
                            "例如：server_is_overloaded, slow_down",
                          )}
                          disabled={isDisabled}
                        />
                      </div>
                      <div className="space-y-1.5">
                        <Label htmlFor={`retry-message-${appType}-${index}`}>
                          {t("proxy.autoFailover.messageContains", "消息包含")}
                        </Label>
                        <Input
                          id={`retry-message-${appType}-${index}`}
                          value={rule.messageContains}
                          onChange={(event) =>
                            updateRetryRule(index, {
                              messageContains: event.target.value,
                            })
                          }
                          placeholder={t(
                            "proxy.autoFailover.messageContainsPlaceholder",
                            "例如：temporarily unavailable",
                          )}
                          disabled={isDisabled}
                        />
                      </div>
                      <div className="space-y-1.5">
                        <Label htmlFor={`retry-count-${appType}-${index}`}>
                          {t(
                            "proxy.autoFailover.ruleRetryCount",
                            "额外重试次数",
                          )}
                        </Label>
                        <Input
                          id={`retry-count-${appType}-${index}`}
                          type="number"
                          min="0"
                          max="10"
                          value={rule.retryCount}
                          onChange={(event) =>
                            updateRetryRule(index, {
                              retryCount: event.target.value,
                            })
                          }
                          disabled={isDisabled}
                        />
                      </div>
                      <div className="space-y-1.5">
                        <Label htmlFor={`retry-backoff-${appType}-${index}`}>
                          {t("proxy.autoFailover.backoffStrategy", "退避算法")}
                        </Label>
                        <Select
                          value={rule.backoffStrategy}
                          onValueChange={(backoffStrategy) =>
                            updateRetryRule(index, {
                              backoffStrategy: backoffStrategy as
                                | "exponential"
                                | "fixed",
                            })
                          }
                          disabled={isDisabled}
                        >
                          <SelectTrigger
                            id={`retry-backoff-${appType}-${index}`}
                            aria-label={t(
                              "proxy.autoFailover.backoffStrategy",
                              "退避算法",
                            )}
                          >
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="exponential">
                              {t(
                                "proxy.autoFailover.exponentialBackoff",
                                "指数退避",
                              )}
                            </SelectItem>
                            <SelectItem value="fixed">
                              {t(
                                "proxy.autoFailover.fixedBackoff",
                                "固定间隔",
                              )}
                            </SelectItem>
                          </SelectContent>
                        </Select>
                      </div>
                      <div className="space-y-1.5">
                        <Label htmlFor={`retry-max-delay-${appType}-${index}`}>
                          {t(
                            "proxy.autoFailover.maxDelaySeconds",
                            "最大等待时间（秒）",
                          )}
                        </Label>
                        <Input
                          id={`retry-max-delay-${appType}-${index}`}
                          type="number"
                          min="1"
                          max="15"
                          value={rule.maxDelaySeconds}
                          onChange={(event) =>
                            updateRetryRule(index, {
                              maxDelaySeconds: event.target.value,
                            })
                          }
                          disabled={isDisabled}
                        />
                      </div>
                    </div>
                    <p className="text-xs text-muted-foreground">
                      {t(
                        "proxy.autoFailover.retryRuleConditionHint",
                        "至少填写一个条件。指数退避从约 250ms 开始逐次翻倍，等待时间不会超过设置上限。",
                      )}
                    </p>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>

        {/* 超时配置 */}
        <div className="space-y-4 rounded-lg border border-white/10 bg-muted/30 p-4">
          <h4 className="text-sm font-semibold">
            {t("proxy.autoFailover.timeoutSettings", "超时配置")}
          </h4>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <div className="space-y-2">
              <Label htmlFor={`streamingFirstByte-${appType}`}>
                {t(
                  "proxy.autoFailover.streamingFirstByte",
                  "流式首字节超时（秒）",
                )}
              </Label>
              <Input
                id={`streamingFirstByte-${appType}`}
                type="number"
                min="1"
                max="120"
                value={formData.streamingFirstByteTimeout}
                onChange={(e) =>
                  setFormData({
                    ...formData,
                    streamingFirstByteTimeout: e.target.value,
                  })
                }
                disabled={isDisabled}
              />
              <p className="text-xs text-muted-foreground">
                {t(
                  "proxy.autoFailover.streamingFirstByteHint",
                  "等待首个数据块的最大时间，范围 1-120 秒，默认 60 秒",
                )}
              </p>
            </div>

            <div className="space-y-2">
              <Label htmlFor={`streamingIdle-${appType}`}>
                {t("proxy.autoFailover.streamingIdle", "流式静默超时（秒）")}
              </Label>
              <Input
                id={`streamingIdle-${appType}`}
                type="number"
                min="0"
                max="600"
                value={formData.streamingIdleTimeout}
                onChange={(e) =>
                  setFormData({
                    ...formData,
                    streamingIdleTimeout: e.target.value,
                  })
                }
                disabled={isDisabled}
              />
              <p className="text-xs text-muted-foreground">
                {t(
                  "proxy.autoFailover.streamingIdleHint",
                  "数据块之间的最大间隔，范围 60-600 秒，填 0 禁用（防止中途卡住）",
                )}
              </p>
            </div>

            <div className="space-y-2">
              <Label htmlFor={`nonStreaming-${appType}`}>
                {t("proxy.autoFailover.nonStreaming", "非流式超时（秒）")}
              </Label>
              <Input
                id={`nonStreaming-${appType}`}
                type="number"
                min="60"
                max="1200"
                value={formData.nonStreamingTimeout}
                onChange={(e) =>
                  setFormData({
                    ...formData,
                    nonStreamingTimeout: e.target.value,
                  })
                }
                disabled={isDisabled}
              />
              <p className="text-xs text-muted-foreground">
                {t(
                  "proxy.autoFailover.nonStreamingHint",
                  "非流式请求的总超时时间，范围 60-1200 秒，默认 600 秒（10 分钟）",
                )}
              </p>
            </div>
          </div>
        </div>

        {/* 熔断器配置 */}
        <div className="space-y-4 rounded-lg border border-white/10 bg-muted/30 p-4">
          <h4 className="text-sm font-semibold">
            {t("proxy.autoFailover.circuitBreakerSettings", "熔断器配置")}
          </h4>

          <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
            <div className="space-y-2">
              <Label htmlFor={`successThreshold-${appType}`}>
                {t("proxy.autoFailover.successThreshold", "恢复成功阈值")}
              </Label>
              <Input
                id={`successThreshold-${appType}`}
                type="number"
                min="1"
                max="10"
                value={formData.circuitSuccessThreshold}
                onChange={(e) =>
                  setFormData({
                    ...formData,
                    circuitSuccessThreshold: e.target.value,
                  })
                }
                disabled={isDisabled}
              />
              <p className="text-xs text-muted-foreground">
                {t(
                  "proxy.autoFailover.successThresholdHint",
                  "半开状态下成功多少次后关闭熔断器",
                )}
              </p>
            </div>

            <div className="space-y-2">
              <Label htmlFor={`timeoutSeconds-${appType}`}>
                {t("proxy.autoFailover.timeout", "恢复等待时间（秒）")}
              </Label>
              <Input
                id={`timeoutSeconds-${appType}`}
                type="number"
                min="0"
                max="300"
                value={formData.circuitTimeoutSeconds}
                onChange={(e) =>
                  setFormData({
                    ...formData,
                    circuitTimeoutSeconds: e.target.value,
                  })
                }
                disabled={isDisabled}
              />
              <p className="text-xs text-muted-foreground">
                {t(
                  "proxy.autoFailover.timeoutHint",
                  "熔断器打开后，等待多久后尝试恢复（建议: 30-120）",
                )}
              </p>
            </div>

            <div className="space-y-2">
              <Label htmlFor={`errorRateThreshold-${appType}`}>
                {t("proxy.autoFailover.errorRate", "错误率阈值 (%)")}
              </Label>
              <Input
                id={`errorRateThreshold-${appType}`}
                type="number"
                min="0"
                max="100"
                step="5"
                value={formData.circuitErrorRateThreshold}
                onChange={(e) =>
                  setFormData({
                    ...formData,
                    circuitErrorRateThreshold: e.target.value,
                  })
                }
                disabled={isDisabled}
              />
              <p className="text-xs text-muted-foreground">
                {t(
                  "proxy.autoFailover.errorRateHint",
                  "错误率超过此值时打开熔断器",
                )}
              </p>
            </div>

            <div className="space-y-2">
              <Label htmlFor={`minRequests-${appType}`}>
                {t("proxy.autoFailover.minRequests", "最小请求数")}
              </Label>
              <Input
                id={`minRequests-${appType}`}
                type="number"
                min="5"
                max="100"
                value={formData.circuitMinRequests}
                onChange={(e) =>
                  setFormData({
                    ...formData,
                    circuitMinRequests: e.target.value,
                  })
                }
                disabled={isDisabled}
              />
              <p className="text-xs text-muted-foreground">
                {t(
                  "proxy.autoFailover.minRequestsHint",
                  "计算错误率前的最小请求数",
                )}
              </p>
            </div>
          </div>
        </div>

        {/* 操作按钮 */}
        <div className="flex justify-end gap-3 pt-2">
          <Button variant="outline" onClick={handleReset} disabled={isDisabled}>
            {t("common.reset", "重置")}
          </Button>
          <Button onClick={handleSave} disabled={isDisabled}>
            {updateConfig.isPending ? (
              <>
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                {t("common.saving", "保存中...")}
              </>
            ) : (
              <>
                <Save className="mr-2 h-4 w-4" />
                {t("common.save", "保存")}
              </>
            )}
          </Button>
        </div>
      </div>
    </div>
  );
}
