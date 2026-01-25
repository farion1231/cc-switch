import { useTranslation } from "react-i18next";
import { useState, useEffect } from "react";
import {
  ChevronDown,
  ChevronRight,
  FlaskConical,
  Globe,
  Coins,
  Eye,
  EyeOff,
  X,
} from "lucide-react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import type { ProviderTestConfig, ProviderProxyConfig } from "@/types";

export type PricingModelSourceOption = "inherit" | "request" | "response";

interface ProviderPricingConfig {
  enabled: boolean;
  costMultiplier?: string;
  pricingModelSource: PricingModelSourceOption;
}

interface ProviderAdvancedConfigProps {
  testConfig: ProviderTestConfig;
  proxyConfig: ProviderProxyConfig;
  pricingConfig: ProviderPricingConfig;
  onTestConfigChange: (config: ProviderTestConfig) => void;
  onProxyConfigChange: (config: ProviderProxyConfig) => void;
  onPricingConfigChange: (config: ProviderPricingConfig) => void;
}

/** 从 ProviderProxyConfig 构建完整 URL */
function buildProxyUrl(config: ProviderProxyConfig): string {
  if (!config.proxyHost) return "";

  const protocol = config.proxyType || "http";
  const host = config.proxyHost;
  const port = config.proxyPort || (protocol === "socks5" ? 1080 : 7890);

  return `${protocol}://${host}:${port}`;
}

/** 从完整 URL 解析为 ProviderProxyConfig */
function parseProxyUrl(url: string): Partial<ProviderProxyConfig> {
  if (!url.trim()) {
    return { proxyHost: undefined, proxyPort: undefined, proxyType: undefined };
  }

  try {
    const parsed = new URL(url);
    const protocol = parsed.protocol.replace(":", "") as
      | "http"
      | "https"
      | "socks5";
    const host = parsed.hostname;
    const port = parsed.port ? parseInt(parsed.port, 10) : undefined;

    return {
      proxyType: protocol,
      proxyHost: host || undefined,
      proxyPort: port,
    };
  } catch {
    // 尝试简单解析（不是标准 URL 格式）
    const match = url.match(/^(?:(\w+):\/\/)?([^:]+)(?::(\d+))?$/);
    if (match) {
      return {
        proxyType: (match[1] as "http" | "https" | "socks5") || "http",
        proxyHost: match[2] || undefined,
        proxyPort: match[3] ? parseInt(match[3], 10) : undefined,
      };
    }
    return {};
  }
}

export function ProviderAdvancedConfig({
  testConfig,
  proxyConfig,
  pricingConfig,
  onTestConfigChange,
  onProxyConfigChange,
  onPricingConfigChange,
}: ProviderAdvancedConfigProps) {
  const { t } = useTranslation();
  const [isTestConfigOpen, setIsTestConfigOpen] = useState(testConfig.enabled);
  const [isProxyConfigOpen, setIsProxyConfigOpen] = useState(
    proxyConfig.enabled,
  );
  const [isPricingConfigOpen, setIsPricingConfigOpen] = useState(
    pricingConfig.enabled,
  );
  const [showPassword, setShowPassword] = useState(false);

  // 代理 URL 输入状态（仅在初始化时从 proxyConfig 构建）
  const [proxyUrl, setProxyUrl] = useState(() => buildProxyUrl(proxyConfig));

  // 标记是否为用户主动输入（用于区分外部更新和用户输入）
  const [isUserTyping, setIsUserTyping] = useState(false);

  // 同步外部 testConfig.enabled 变化到展开状态
  useEffect(() => {
    setIsTestConfigOpen(testConfig.enabled);
  }, [testConfig.enabled]);

  // 同步外部 proxyConfig.enabled 变化到展开状态
  useEffect(() => {
    setIsProxyConfigOpen(proxyConfig.enabled);
  }, [proxyConfig.enabled]);

  // 同步外部 pricingConfig.enabled 变化到展开状态
  useEffect(() => {
    setIsPricingConfigOpen(pricingConfig.enabled);
  }, [pricingConfig.enabled]);

  // 仅在外部 proxyConfig 变化且非用户输入时同步（如：重置表单、加载数据）
  useEffect(() => {
    if (!isUserTyping) {
      const newUrl = buildProxyUrl(proxyConfig);
      if (newUrl !== proxyUrl) {
        setProxyUrl(newUrl);
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [proxyConfig.proxyType, proxyConfig.proxyHost, proxyConfig.proxyPort]);

  // 处理代理 URL 变化（用户输入时不触发 URL 重建）
  const handleProxyUrlChange = (value: string) => {
    setIsUserTyping(true);
    setProxyUrl(value);
    const parsed = parseProxyUrl(value);
    onProxyConfigChange({
      ...proxyConfig,
      ...parsed,
    });
  };

  // 输入框失焦时结束用户输入状态
  const handleProxyUrlBlur = () => {
    setIsUserTyping(false);
  };

  // 清除代理配置
  const handleClearProxy = () => {
    setProxyUrl("");
    onProxyConfigChange({
      ...proxyConfig,
      proxyType: undefined,
      proxyHost: undefined,
      proxyPort: undefined,
      proxyUsername: undefined,
      proxyPassword: undefined,
    });
  };

  return (
    <div className="space-y-4">
      {/* 模型测试配置 */}
      <div className="rounded-lg border border-border/50 bg-muted/20">
        <button
          type="button"
          className="flex w-full items-center justify-between p-4 hover:bg-muted/30 transition-colors"
          onClick={() => setIsTestConfigOpen(!isTestConfigOpen)}
        >
          <div className="flex items-center gap-3">
            <FlaskConical className="h-4 w-4 text-muted-foreground" />
            <span className="font-medium">
              {t("providerAdvanced.testConfig", {
                defaultValue: "模型测试配置",
              })}
            </span>
          </div>
          <div className="flex items-center gap-3">
            <div
              className="flex items-center gap-2"
              onClick={(e) => e.stopPropagation()}
            >
              <Label
                htmlFor="test-config-enabled"
                className="text-sm text-muted-foreground"
              >
                {t("providerAdvanced.useCustomConfig", {
                  defaultValue: "使用单独配置",
                })}
              </Label>
              <Switch
                id="test-config-enabled"
                checked={testConfig.enabled}
                onCheckedChange={(checked) => {
                  onTestConfigChange({ ...testConfig, enabled: checked });
                  if (checked) setIsTestConfigOpen(true);
                }}
              />
            </div>
            {isTestConfigOpen ? (
              <ChevronDown className="h-4 w-4 text-muted-foreground" />
            ) : (
              <ChevronRight className="h-4 w-4 text-muted-foreground" />
            )}
          </div>
        </button>
        <div
          className={cn(
            "overflow-hidden transition-all duration-200",
            isTestConfigOpen
              ? "max-h-[500px] opacity-100"
              : "max-h-0 opacity-0",
          )}
        >
          <div className="border-t border-border/50 p-4 space-y-4">
            <p className="text-sm text-muted-foreground">
              {t("providerAdvanced.testConfigDesc", {
                defaultValue:
                  "为此供应商配置单独的模型测试参数，不启用时使用全局配置。",
              })}
            </p>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div className="space-y-2">
                <Label htmlFor="test-model">
                  {t("providerAdvanced.testModel", {
                    defaultValue: "测试模型",
                  })}
                </Label>
                <Input
                  id="test-model"
                  value={testConfig.testModel || ""}
                  onChange={(e) =>
                    onTestConfigChange({
                      ...testConfig,
                      testModel: e.target.value || undefined,
                    })
                  }
                  placeholder={t("providerAdvanced.testModelPlaceholder", {
                    defaultValue: "留空使用全局配置",
                  })}
                  disabled={!testConfig.enabled}
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="test-timeout">
                  {t("providerAdvanced.timeoutSecs", {
                    defaultValue: "超时时间（秒）",
                  })}
                </Label>
                <Input
                  id="test-timeout"
                  type="number"
                  min={1}
                  max={300}
                  value={testConfig.timeoutSecs || ""}
                  onChange={(e) =>
                    onTestConfigChange({
                      ...testConfig,
                      timeoutSecs: e.target.value
                        ? parseInt(e.target.value, 10)
                        : undefined,
                    })
                  }
                  placeholder="45"
                  disabled={!testConfig.enabled}
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="test-prompt">
                  {t("providerAdvanced.testPrompt", {
                    defaultValue: "测试提示词",
                  })}
                </Label>
                <Input
                  id="test-prompt"
                  value={testConfig.testPrompt || ""}
                  onChange={(e) =>
                    onTestConfigChange({
                      ...testConfig,
                      testPrompt: e.target.value || undefined,
                    })
                  }
                  placeholder="Who are you?"
                  disabled={!testConfig.enabled}
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="degraded-threshold">
                  {t("providerAdvanced.degradedThreshold", {
                    defaultValue: "降级阈值（毫秒）",
                  })}
                </Label>
                <Input
                  id="degraded-threshold"
                  type="number"
                  min={100}
                  max={60000}
                  value={testConfig.degradedThresholdMs || ""}
                  onChange={(e) =>
                    onTestConfigChange({
                      ...testConfig,
                      degradedThresholdMs: e.target.value
                        ? parseInt(e.target.value, 10)
                        : undefined,
                    })
                  }
                  placeholder="6000"
                  disabled={!testConfig.enabled}
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="max-retries">
                  {t("providerAdvanced.maxRetries", {
                    defaultValue: "最大重试次数",
                  })}
                </Label>
                <Input
                  id="max-retries"
                  type="number"
                  min={0}
                  max={10}
                  value={testConfig.maxRetries ?? ""}
                  onChange={(e) =>
                    onTestConfigChange({
                      ...testConfig,
                      maxRetries: e.target.value
                        ? parseInt(e.target.value, 10)
                        : undefined,
                    })
                  }
                  placeholder="2"
                  disabled={!testConfig.enabled}
                />
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* 代理配置 */}
      <div className="rounded-lg border border-border/50 bg-muted/20">
        <button
          type="button"
          className="flex w-full items-center justify-between p-4 hover:bg-muted/30 transition-colors"
          onClick={() => setIsProxyConfigOpen(!isProxyConfigOpen)}
        >
          <div className="flex items-center gap-3">
            <Globe className="h-4 w-4 text-muted-foreground" />
            <span className="font-medium">
              {t("providerAdvanced.proxyConfig", {
                defaultValue: "代理配置",
              })}
            </span>
          </div>
          <div className="flex items-center gap-3">
            <div
              className="flex items-center gap-2"
              onClick={(e) => e.stopPropagation()}
            >
              <Label
                htmlFor="proxy-config-enabled"
                className="text-sm text-muted-foreground"
              >
                {t("providerAdvanced.useCustomProxy", {
                  defaultValue: "使用单独代理",
                })}
              </Label>
              <Switch
                id="proxy-config-enabled"
                checked={proxyConfig.enabled}
                onCheckedChange={(checked) => {
                  onProxyConfigChange({ ...proxyConfig, enabled: checked });
                  if (checked) setIsProxyConfigOpen(true);
                }}
              />
            </div>
            {isProxyConfigOpen ? (
              <ChevronDown className="h-4 w-4 text-muted-foreground" />
            ) : (
              <ChevronRight className="h-4 w-4 text-muted-foreground" />
            )}
          </div>
        </button>
        <div
          className={cn(
            "overflow-hidden transition-all duration-200",
            isProxyConfigOpen
              ? "max-h-[500px] opacity-100"
              : "max-h-0 opacity-0",
          )}
        >
          <div className="border-t border-border/50 p-4 space-y-3">
            <p className="text-sm text-muted-foreground">
              {t("providerAdvanced.proxyConfigDesc", {
                defaultValue:
                  "为此供应商配置单独的网络代理，不启用时使用系统代理或全局设置。",
              })}
            </p>

            {/* 代理地址输入框（仿照全局代理样式） */}
            <div className="flex gap-2">
              <Input
                placeholder="http://127.0.0.1:7890 / socks5://127.0.0.1:1080"
                value={proxyUrl}
                onChange={(e) => handleProxyUrlChange(e.target.value)}
                onBlur={handleProxyUrlBlur}
                className="font-mono text-sm flex-1"
                disabled={!proxyConfig.enabled}
              />
              <Button
                type="button"
                variant="outline"
                size="icon"
                disabled={!proxyConfig.enabled || !proxyUrl}
                onClick={handleClearProxy}
                title={t("common.clear", { defaultValue: "清除" })}
              >
                <X className="h-4 w-4" />
              </Button>
            </div>

            {/* 认证信息：用户名 + 密码（可选） */}
            <div className="flex gap-2">
              <Input
                placeholder={t("providerAdvanced.proxyUsername", {
                  defaultValue: "用户名（可选）",
                })}
                value={proxyConfig.proxyUsername || ""}
                onChange={(e) =>
                  onProxyConfigChange({
                    ...proxyConfig,
                    proxyUsername: e.target.value || undefined,
                  })
                }
                className="font-mono text-sm flex-1"
                disabled={!proxyConfig.enabled}
              />
              <div className="relative flex-1">
                <Input
                  type={showPassword ? "text" : "password"}
                  placeholder={t("providerAdvanced.proxyPassword", {
                    defaultValue: "密码（可选）",
                  })}
                  value={proxyConfig.proxyPassword || ""}
                  onChange={(e) =>
                    onProxyConfigChange({
                      ...proxyConfig,
                      proxyPassword: e.target.value || undefined,
                    })
                  }
                  className="font-mono text-sm pr-10"
                  disabled={!proxyConfig.enabled}
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="absolute right-0 top-0 h-full px-3 hover:bg-transparent"
                  onClick={() => setShowPassword(!showPassword)}
                  tabIndex={-1}
                  disabled={!proxyConfig.enabled}
                >
                  {showPassword ? (
                    <EyeOff className="h-4 w-4 text-muted-foreground" />
                  ) : (
                    <Eye className="h-4 w-4 text-muted-foreground" />
                  )}
                </Button>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* 计费配置 */}
      <div className="rounded-lg border border-border/50 bg-muted/20">
        <button
          type="button"
          className="flex w-full items-center justify-between p-4 hover:bg-muted/30 transition-colors"
          onClick={() => setIsPricingConfigOpen(!isPricingConfigOpen)}
        >
          <div className="flex items-center gap-3">
            <Coins className="h-4 w-4 text-muted-foreground" />
            <span className="font-medium">
              {t("providerAdvanced.pricingConfig", {
                defaultValue: "计费配置",
              })}
            </span>
          </div>
          <div className="flex items-center gap-3">
            <div
              className="flex items-center gap-2"
              onClick={(e) => e.stopPropagation()}
            >
              <Label
                htmlFor="pricing-config-enabled"
                className="text-sm text-muted-foreground"
              >
                {t("providerAdvanced.useCustomPricing", {
                  defaultValue: "使用单独配置",
                })}
              </Label>
              <Switch
                id="pricing-config-enabled"
                checked={pricingConfig.enabled}
                onCheckedChange={(checked) => {
                  onPricingConfigChange({ ...pricingConfig, enabled: checked });
                  if (checked) setIsPricingConfigOpen(true);
                }}
              />
            </div>
            {isPricingConfigOpen ? (
              <ChevronDown className="h-4 w-4 text-muted-foreground" />
            ) : (
              <ChevronRight className="h-4 w-4 text-muted-foreground" />
            )}
          </div>
        </button>
        <div
          className={cn(
            "overflow-hidden transition-all duration-200",
            isPricingConfigOpen
              ? "max-h-[500px] opacity-100"
              : "max-h-0 opacity-0",
          )}
        >
          <div className="border-t border-border/50 p-4 space-y-4">
            <p className="text-sm text-muted-foreground">
              {t("providerAdvanced.pricingConfigDesc", {
                defaultValue:
                  "为此供应商配置单独的计费参数，不启用时使用全局默认配置。",
              })}
            </p>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div className="space-y-2">
                <Label htmlFor="cost-multiplier">
                  {t("providerAdvanced.costMultiplier", {
                    defaultValue: "成本倍率",
                  })}
                </Label>
                <Input
                  id="cost-multiplier"
                  type="number"
                  step="0.01"
                  inputMode="decimal"
                  value={pricingConfig.costMultiplier || ""}
                  onChange={(e) =>
                    onPricingConfigChange({
                      ...pricingConfig,
                      costMultiplier: e.target.value || undefined,
                    })
                  }
                  placeholder={t("providerAdvanced.costMultiplierPlaceholder", {
                    defaultValue: "留空使用全局默认（1）",
                  })}
                  disabled={!pricingConfig.enabled}
                />
                <p className="text-xs text-muted-foreground">
                  {t("providerAdvanced.costMultiplierHint", {
                    defaultValue: "实际成本 = 基础成本 × 倍率，支持小数如 1.5",
                  })}
                </p>
              </div>
              <div className="space-y-2">
                <Label htmlFor="pricing-model-source">
                  {t("providerAdvanced.pricingModelSourceLabel", {
                    defaultValue: "计费模式",
                  })}
                </Label>
                <Select
                  value={pricingConfig.pricingModelSource}
                  onValueChange={(value) =>
                    onPricingConfigChange({
                      ...pricingConfig,
                      pricingModelSource: value as PricingModelSourceOption,
                    })
                  }
                  disabled={!pricingConfig.enabled}
                >
                  <SelectTrigger id="pricing-model-source">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="inherit">
                      {t("providerAdvanced.pricingModelSourceInherit", {
                        defaultValue: "继承全局默认",
                      })}
                    </SelectItem>
                    <SelectItem value="request">
                      {t("providerAdvanced.pricingModelSourceRequest", {
                        defaultValue: "请求模型",
                      })}
                    </SelectItem>
                    <SelectItem value="response">
                      {t("providerAdvanced.pricingModelSourceResponse", {
                        defaultValue: "返回模型",
                      })}
                    </SelectItem>
                  </SelectContent>
                </Select>
                <p className="text-xs text-muted-foreground">
                  {t("providerAdvanced.pricingModelSourceHint", {
                    defaultValue: "选择按请求模型还是返回模型进行定价匹配",
                  })}
                </p>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
