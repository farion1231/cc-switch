import { useTranslation } from "react-i18next";
import { useState, useEffect } from "react";
import {
  ChevronDown,
  ChevronRight,
  Route,
  Plus,
  Trash2,
  Copy,
  ArrowRight,
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
import type {
  ModelRoutingConfig,
  ModelRoute,
  RouteTarget,
  RouteFallback,
} from "@/types";

interface ModelRoutingConfigPanelProps {
  config: ModelRoutingConfig;
  onConfigChange: (config: ModelRoutingConfig) => void;
}

export function ModelRoutingConfigPanel({
  config,
  onConfigChange,
}: ModelRoutingConfigPanelProps) {
  const { t } = useTranslation();
  const [isOpen, setIsOpen] = useState(config.enabled);

  useEffect(() => {
    setIsOpen(config.enabled);
  }, [config.enabled]);

  const handleEnabledChange = (enabled: boolean) => {
    onConfigChange({ ...config, enabled });
    if (enabled) setIsOpen(true);
  };

  const handleAddRoute = () => {
    const newRoute: ModelRoute = {
      sourceModel: "",
      target: {
        baseUrl: "",
        apiFormat: "anthropic",
        modelName: "",
      },
    };
    onConfigChange({
      ...config,
      routes: [...config.routes, newRoute],
    });
  };

  const handleUpdateRoute = (index: number, route: ModelRoute) => {
    const newRoutes = [...config.routes];
    newRoutes[index] = route;
    onConfigChange({ ...config, routes: newRoutes });
  };

  const handleDeleteRoute = (index: number) => {
    onConfigChange({
      ...config,
      routes: config.routes.filter((_, i) => i !== index),
    });
  };

  const handleDuplicateRoute = (index: number) => {
    const routeToDuplicate = config.routes[index];
    const newRoutes = [...config.routes];
    newRoutes.splice(index + 1, 0, { ...routeToDuplicate });
    onConfigChange({ ...config, routes: newRoutes });
  };

  const handleFallbackChange = (apiFormat: string) => {
    onConfigChange({
      ...config,
      fallback: { apiFormat: apiFormat as RouteFallback["apiFormat"] },
    });
  };

  return (
    <div className="rounded-lg border border-border/50 bg-muted/20">
      <button
        type="button"
        className="flex w-full items-center justify-between p-4 hover:bg-muted/30 transition-colors"
        onClick={() => setIsOpen(!isOpen)}
      >
        <div className="flex items-center gap-3">
          <Route className="h-4 w-4 text-muted-foreground" />
          <span className="font-medium">
            {t("providerAdvanced.modelRoutingConfig", {
              defaultValue: "模型路由配置",
            })}
          </span>
        </div>
        <div className="flex items-center gap-3">
          <div
            className="flex items-center gap-2"
            onClick={(e) => e.stopPropagation()}
          >
            <Label
              htmlFor="model-routing-enabled"
              className="text-sm text-muted-foreground"
            >
              {t("providerAdvanced.enableModelRouting", {
                defaultValue: "启用路由",
              })}
            </Label>
            <Switch
              id="model-routing-enabled"
              checked={config.enabled}
              onCheckedChange={handleEnabledChange}
            />
          </div>
          {isOpen ? (
            <ChevronDown className="h-4 w-4 text-muted-foreground" />
          ) : (
            <ChevronRight className="h-4 w-4 text-muted-foreground" />
          )}
        </div>
      </button>
      <div
        className={cn(
          "overflow-hidden transition-all duration-200",
          isOpen ? "max-h-[2000px] opacity-100" : "max-h-0 opacity-0",
        )}
      >
        <div className="border-t border-border/50 p-4 space-y-4">
          <p className="text-sm text-muted-foreground">
            {t("providerAdvanced.modelRoutingConfigDesc", {
              defaultValue:
                "配置模型路由规则，根据请求中的模型名称自动切换到不同的 API 格式和目标端点。例如：将 Claude Opus 路由到 Vertex Gemini，将 Claude Sonnet 路由到 Gemini Flash。",
            })}
          </p>

          {/* 路由规则列表 */}
          <div className="space-y-4">
            <div className="flex items-center justify-between">
              <Label className="text-base font-semibold">
                {t("providerAdvanced.routingRules", {
                  defaultValue: "路由规则",
                })}
              </Label>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={handleAddRoute}
                disabled={!config.enabled}
              >
                <Plus className="h-4 w-4 mr-1" />
                {t("providerAdvanced.addRoute", {
                  defaultValue: "添加规则",
                })}
              </Button>
            </div>

            {config.routes.length === 0 && (
              <div className="text-center py-8 text-muted-foreground">
                <Route className="h-12 w-12 mx-auto mb-2 opacity-20" />
                <p className="text-sm">
                  {t("providerAdvanced.noRoutesYet", {
                    defaultValue: "暂无路由规则，点击上方按钮添加",
                  })}
                </p>
              </div>
            )}

            {config.routes.map((route, index) => (
              <RouteItem
                key={index}
                route={route}
                index={index}
                disabled={!config.enabled}
                onUpdate={(updated) => handleUpdateRoute(index, updated)}
                onDelete={() => handleDeleteRoute(index)}
                onDuplicate={() => handleDuplicateRoute(index)}
              />
            ))}
          </div>

          {/* 兜底配置 */}
          <div className="border-t border-border/50 pt-4 space-y-3">
            <Label className="text-base font-semibold">
              {t("providerAdvanced.fallbackConfig", {
                defaultValue: "兜底配置",
              })}
            </Label>
            <p className="text-sm text-muted-foreground">
              {t("providerAdvanced.fallbackConfigDesc", {
                defaultValue:
                  "当请求模型没有匹配到任何路由规则时，使用兜底配置。留空则使用供应商默认 API 格式。",
              })}
            </p>
            <div className="flex items-center gap-2">
              <Label htmlFor="fallback-api-format" className="whitespace-nowrap">
                {t("providerAdvanced.fallbackApiFormat", {
                  defaultValue: "兜底 API 格式",
                })}
              </Label>
              <Select
                value={config.fallback?.apiFormat || "anthropic"}
                onValueChange={handleFallbackChange}
                disabled={!config.enabled}
              >
                <SelectTrigger id="fallback-api-format" className="w-48">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="anthropic">
                    {t("providerAdvanced.apiFormatAnthropic", {
                      defaultValue: "Anthropic 原生",
                    })}
                  </SelectItem>
                  <SelectItem value="openai_chat">
                    {t("providerAdvanced.apiFormatOpenAIChat", {
                      defaultValue: "OpenAI Chat",
                    })}
                  </SelectItem>
                  <SelectItem value="openai_responses">
                    {t("providerAdvanced.apiFormatOpenAIResponses", {
                      defaultValue: "OpenAI Responses",
                    })}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

interface RouteItemProps {
  route: ModelRoute;
  index: number;
  disabled: boolean;
  onUpdate: (route: ModelRoute) => void;
  onDelete: () => void;
  onDuplicate: () => void;
}

function RouteItem({
  route,
  index,
  disabled,
  onUpdate,
  onDelete,
  onDuplicate,
}: RouteItemProps) {
  const { t } = useTranslation();

  return (
    <div className="border border-border/50 rounded-lg p-4 space-y-3 bg-background/50">
      <div className="flex items-center justify-between">
        <Label className="text-sm font-medium">
          {t("providerAdvanced.route", { defaultValue: "规则" })} #{index + 1}
        </Label>
        <div className="flex items-center gap-1">
          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={onDuplicate}
            disabled={disabled}
            title={t("common.duplicate", { defaultValue: "复制" })}
          >
            <Copy className="h-4 w-4" />
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={onDelete}
            disabled={disabled}
            className="text-destructive hover:text-destructive"
            title={t("common.delete", { defaultValue: "删除" })}
          >
            <Trash2 className="h-4 w-4" />
          </Button>
        </div>
      </div>

      {/* 源模型 */}
      <div className="space-y-2">
        <Label htmlFor={`source-model-${index}`}>
          {t("providerAdvanced.sourceModel", { defaultValue: "源模型" })}
        </Label>
        <Input
          id={`source-model-${index}`}
          value={route.sourceModel}
          onChange={(e) =>
            onUpdate({ ...route, sourceModel: e.target.value })
          }
          placeholder={t("providerAdvanced.sourceModelPlaceholder", {
            defaultValue: "例如：claude-opus-4-5",
          })}
          disabled={disabled}
          className="font-mono"
        />
        <p className="text-xs text-muted-foreground">
          {t("providerAdvanced.sourceModelHint", {
            defaultValue: "不区分大小写匹配，例如 claude-opus-4-5 可以匹配 Claude Opus 4.5",
          })}
        </p>
      </div>

      {/* 箭头指示 */}
      <div className="flex items-center justify-center py-2">
        <ArrowRight className="h-5 w-5 text-muted-foreground rotate-90" />
      </div>

      {/* 目标配置 */}
      <div className="space-y-3">
        <Label className="text-sm font-medium">
          {t("providerAdvanced.targetConfig", { defaultValue: "目标配置" })}
        </Label>

        <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
          <div className="space-y-2">
            <Label
              htmlFor={`target-base-url-${index}`}
              className="text-xs text-muted-foreground"
            >
              {t("providerAdvanced.targetBaseUrl", {
                defaultValue: "目标 Base URL",
              })}
            </Label>
            <Input
              id={`target-base-url-${index}`}
              value={route.target.baseUrl}
              onChange={(e) =>
                onUpdate({
                  ...route,
                  target: { ...route.target, baseUrl: e.target.value },
                })
              }
              placeholder="https://api.vertexai.example.com"
              disabled={disabled}
              className="font-mono text-xs"
            />
          </div>

          <div className="space-y-2">
            <Label
              htmlFor={`target-api-format-${index}`}
              className="text-xs text-muted-foreground"
            >
              {t("providerAdvanced.targetApiFormat", {
                defaultValue: "目标 API 格式",
              })}
            </Label>
            <Select
              value={route.target.apiFormat}
              onValueChange={(value) =>
                onUpdate({
                  ...route,
                  target: {
                    ...route.target,
                    apiFormat: value as RouteTarget["apiFormat"],
                  },
                })
              }
              disabled={disabled}
            >
              <SelectTrigger id={`target-api-format-${index}`}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="anthropic">
                  {t("providerAdvanced.apiFormatAnthropic", {
                    defaultValue: "Anthropic 原生",
                  })}
                </SelectItem>
                <SelectItem value="openai_chat">
                  {t("providerAdvanced.apiFormatOpenAIChat", {
                    defaultValue: "OpenAI Chat",
                  })}
                </SelectItem>
                <SelectItem value="openai_responses">
                  {t("providerAdvanced.apiFormatOpenAIResponses", {
                    defaultValue: "OpenAI Responses",
                  })}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-2">
            <Label
              htmlFor={`target-model-name-${index}`}
              className="text-xs text-muted-foreground"
            >
              {t("providerAdvanced.targetModelName", {
                defaultValue: "目标模型名称",
              })}
            </Label>
            <Input
              id={`target-model-name-${index}`}
              value={route.target.modelName}
              onChange={(e) =>
                onUpdate({
                  ...route,
                  target: { ...route.target, modelName: e.target.value },
                })
              }
              placeholder="google/gemini-2.0-flash"
              disabled={disabled}
              className="font-mono text-xs"
            />
          </div>
        </div>
      </div>
    </div>
  );
}
