import { useEffect, useMemo, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  ChevronDown,
  ChevronRight,
  Download,
  Loader2,
  Plus,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { BasicFormFields } from "./BasicFormFields";
import { EndpointField } from "./shared/EndpointField";
import { ModelDropdown } from "./shared/ModelDropdown";
import { providerSchema, type ProviderFormData } from "@/lib/schemas/provider";
import type {
  ClaudeApiFormat,
  ClaudeDesktopModelRoute,
  ProviderCategory,
  ProviderMeta,
} from "@/types";
import type { OpenClawSuggestedDefaults } from "@/config/openclawProviderPresets";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import {
  providersApi,
  type ClaudeDesktopDefaultRoute,
} from "@/lib/api/providers";

export type ClaudeDesktopProviderFormValues = ProviderFormData & {
  presetId?: string;
  presetCategory?: ProviderCategory;
  isPartner?: boolean;
  meta?: ProviderMeta;
  providerKey?: string;
  suggestedDefaults?: OpenClawSuggestedDefaults;
};

export interface ClaudeDesktopProviderFormProps {
  submitLabel: string;
  onSubmit: (values: ClaudeDesktopProviderFormValues) => Promise<void> | void;
  onCancel: () => void;
  onSubmittingChange?: (isSubmitting: boolean) => void;
  initialData?: {
    name?: string;
    websiteUrl?: string;
    notes?: string;
    settingsConfig?: Record<string, unknown>;
    category?: ProviderCategory;
    meta?: ProviderMeta;
    icon?: string;
    iconColor?: string;
  };
  showButtons?: boolean;
}

type RouteRow = {
  route: string;
  model: string;
  displayName: string;
  supports1m: boolean;
};

function envString(
  settingsConfig: Record<string, unknown> | undefined,
  key: string,
) {
  const env = settingsConfig?.env;
  if (!env || typeof env !== "object") return "";
  const value = (env as Record<string, unknown>)[key];
  return typeof value === "string" ? value : "";
}

function clonePlainRecord(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return {};
  }
  return { ...(value as Record<string, unknown>) };
}

function initialRouteRows(
  routes: Record<string, ClaudeDesktopModelRoute> | undefined,
): RouteRow[] {
  return Object.entries(routes ?? {}).map(([route, value]) => ({
    route,
    model: value.model ?? "",
    displayName: value.displayName ?? "",
    supports1m: value.supports1m ?? false,
  }));
}

function isClaudeSafeRoute(route: string) {
  const normalized = route.trim().toLowerCase();
  return (
    normalized.startsWith("claude-") ||
    normalized.startsWith("anthropic/claude-")
  );
}

function defaultRouteRows(
  defaults: ClaudeDesktopDefaultRoute[],
  defaultModel: string,
): RouteRow[] {
  return defaults.map((route, index) => ({
    route: route.routeId,
    model: index === 0 ? defaultModel : "",
    displayName: route.displayName,
    supports1m: route.supports1m,
  }));
}

function nextRouteRow(current: RouteRow[], defaults: RouteRow[]): RouteRow {
  return (
    defaults.find(
      (route) => !current.some((existing) => existing.route === route.route),
    ) ?? {
      route: "",
      model: "",
      displayName: "",
      supports1m: true,
    }
  );
}

export function ClaudeDesktopProviderForm({
  submitLabel,
  onSubmit,
  onCancel,
  onSubmittingChange,
  initialData,
  showButtons = true,
}: ClaudeDesktopProviderFormProps) {
  const { t } = useTranslation();
  const initialMode = initialData?.meta?.claudeDesktopMode ?? "direct";
  const [mode, setMode] = useState<"direct" | "proxy">(initialMode);
  const needsModelMapping = mode === "proxy";
  const [apiFormat, setApiFormat] = useState<ClaudeApiFormat>(
    initialData?.meta?.apiFormat ?? "anthropic",
  );
  const [baseUrl, setBaseUrl] = useState(
    envString(initialData?.settingsConfig, "ANTHROPIC_BASE_URL"),
  );
  const [apiKey, setApiKey] = useState(
    envString(initialData?.settingsConfig, "ANTHROPIC_AUTH_TOKEN") ||
      envString(initialData?.settingsConfig, "ANTHROPIC_API_KEY"),
  );
  const [routes, setRoutes] = useState<RouteRow[]>(() =>
    initialRouteRows(initialData?.meta?.claudeDesktopModelRoutes),
  );
  const didSeedDefaultRoutes = useRef(
    Object.keys(initialData?.meta?.claudeDesktopModelRoutes ?? {}).length > 0,
  );
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const [directModelsExpanded, setDirectModelsExpanded] = useState(
    initialMode === "direct" &&
      Object.keys(initialData?.meta?.claudeDesktopModelRoutes ?? {}).length > 0,
  );
  const { data: defaultRoutes = [] } = useQuery({
    queryKey: ["claudeDesktopDefaultRoutes"],
    queryFn: () => providersApi.getClaudeDesktopDefaultRoutes(),
  });
  const defaultProxyRouteRows = useMemo(
    () =>
      defaultRouteRows(
        defaultRoutes,
        envString(initialData?.settingsConfig, "ANTHROPIC_MODEL"),
      ),
    [defaultRoutes, initialData?.settingsConfig],
  );

  const defaultValues: ProviderFormData = useMemo(
    () => ({
      name: initialData?.name ?? "",
      websiteUrl: initialData?.websiteUrl ?? "",
      notes: initialData?.notes ?? "",
      settingsConfig: JSON.stringify(
        initialData?.settingsConfig ?? { env: {} },
        null,
        2,
      ),
      icon: initialData?.icon ?? "",
      iconColor: initialData?.iconColor ?? "",
    }),
    [initialData],
  );

  const form = useForm<ProviderFormData>({
    resolver: zodResolver(providerSchema),
    defaultValues,
    mode: "onSubmit",
  });

  useEffect(() => {
    onSubmittingChange?.(form.formState.isSubmitting || isFetchingModels);
  }, [form.formState.isSubmitting, isFetchingModels, onSubmittingChange]);

  const updateRoute = (index: number, patch: Partial<RouteRow>) => {
    setRoutes((current) =>
      current.map((row, i) => (i === index ? { ...row, ...patch } : row)),
    );
  };

  const handleModelMappingChange = (checked: boolean) => {
    setMode(checked ? "proxy" : "direct");
    if (checked) {
      setRoutes((current) => {
        if (current.length > 0 || defaultProxyRouteRows.length === 0) {
          return current;
        }
        didSeedDefaultRoutes.current = true;
        return defaultProxyRouteRows;
      });
    }
  };

  useEffect(() => {
    if (
      didSeedDefaultRoutes.current ||
      mode !== "proxy" ||
      routes.length > 0 ||
      defaultProxyRouteRows.length === 0
    ) {
      return;
    }

    didSeedDefaultRoutes.current = true;
    setRoutes(defaultProxyRouteRows);
  }, [defaultProxyRouteRows, mode, routes.length]);

  const handleFetchModels = async () => {
    if (!baseUrl.trim() || !apiKey.trim()) {
      showFetchModelsError(null, t, {
        hasBaseUrl: Boolean(baseUrl.trim()),
        hasApiKey: Boolean(apiKey.trim()),
      });
      return;
    }

    setIsFetchingModels(true);
    try {
      const models = await fetchModelsForConfig(baseUrl.trim(), apiKey.trim());
      setFetchedModels(models);
      toast.success(
        t("providerForm.fetchModelsSuccess", {
          count: models.length,
          defaultValue: `已获取 ${models.length} 个模型`,
        }),
      );
    } catch (error) {
      showFetchModelsError(error, t, {
        hasBaseUrl: Boolean(baseUrl.trim()),
        hasApiKey: Boolean(apiKey.trim()),
      });
    } finally {
      setIsFetchingModels(false);
    }
  };

  const handleSubmit = async (values: ProviderFormData) => {
    if (!values.name.trim()) {
      toast.error(
        t("providerForm.fillSupplierName", {
          defaultValue: "请填写供应商名称",
        }),
      );
      return;
    }
    if (!baseUrl.trim()) {
      toast.error(
        t("providerForm.fetchModelsNeedEndpoint", {
          defaultValue: "请先填写接口地址",
        }),
      );
      return;
    }
    if (!apiKey.trim()) {
      toast.error(
        t("providerForm.fetchModelsNeedApiKey", {
          defaultValue: "请先填写 API Key",
        }),
      );
      return;
    }

    const routeEntries = routes
      .map((route, index) => ({
        ...route,
        route:
          route.route.trim() ||
          (mode === "proxy"
            ? `claude-${route.model.trim().replace(/[/\\]/g, "-") || `model-${index + 1}`}`
            : ""),
        model: route.model.trim(),
        displayName: route.displayName.trim(),
      }))
      .filter((route) => route.route || route.model);

    if (mode === "proxy") {
      const missing = routeEntries.find((route) => !route.model);
      if (missing) {
        toast.error(
          t("claudeDesktop.routeInvalid", {
            defaultValue: "请填写上游模型名",
          }),
        );
        return;
      }
      if (routeEntries.length === 0) {
        toast.error(
          t("claudeDesktop.routesRequired", {
            defaultValue: "至少填写一个上游模型",
          }),
        );
        return;
      }
    } else {
      const invalid = routeEntries.find(
        (route) => !route.route || !isClaudeSafeRoute(route.route),
      );
      if (invalid) {
        toast.error(
          t("claudeDesktop.directModelInvalid", {
            defaultValue:
              "直连模型必须使用 claude-* / anthropic/claude-* 模型名",
          }),
        );
        return;
      }
    }

    const settingsConfig = clonePlainRecord(initialData?.settingsConfig);
    const env = clonePlainRecord(settingsConfig.env);
    settingsConfig.env = {
      ...env,
      ANTHROPIC_BASE_URL: baseUrl.trim().replace(/\/+$/, ""),
      ANTHROPIC_AUTH_TOKEN: apiKey.trim(),
    };

    const routeMap = routeEntries.reduce<
      Record<string, ClaudeDesktopModelRoute>
    >((acc, route) => {
      acc[route.route] = {
        model: route.model || route.route,
        displayName: route.displayName || undefined,
        supports1m: route.supports1m || undefined,
      };
      return acc;
    }, {});

    const meta: ProviderMeta = {
      ...(initialData?.meta ?? {}),
      claudeDesktopMode: mode,
      apiFormat: mode === "proxy" ? apiFormat : "anthropic",
    };

    meta.claudeDesktopModelRoutes = routeMap;

    delete meta.endpointAutoSelect;
    delete meta.providerType;
    delete meta.isFullUrl;

    await onSubmit({
      ...values,
      name: values.name.trim(),
      websiteUrl: values.websiteUrl?.trim() ?? "",
      notes: values.notes?.trim() ?? "",
      settingsConfig: JSON.stringify(settingsConfig, null, 2),
      meta,
    });
  };

  const renderActionButtons = (onAdd: () => void, addLabel: string) => (
    <div className="flex gap-1">
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={handleFetchModels}
        disabled={isFetchingModels}
        className="h-7 gap-1"
      >
        {isFetchingModels ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
        ) : (
          <Download className="h-3.5 w-3.5" />
        )}
        {t("providerForm.fetchModels", { defaultValue: "获取模型" })}
      </Button>
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={onAdd}
        className="h-7 gap-1"
      >
        <Plus className="h-3.5 w-3.5" />
        {addLabel}
      </Button>
    </div>
  );

  return (
    <Form {...form}>
      <form
        id="provider-form"
        onSubmit={form.handleSubmit(handleSubmit)}
        className="space-y-6"
      >
        <BasicFormFields form={form} />

        <div className="space-y-1">
          <Label>{"API Key"}</Label>
          <Input
            value={apiKey}
            onChange={(event) => setApiKey(event.target.value)}
            type="password"
            placeholder="sk-..."
          />
        </div>

        <EndpointField
          id="baseUrl"
          label={t("providerForm.apiEndpoint")}
          value={baseUrl}
          onChange={(v) => setBaseUrl(v)}
          placeholder={t("providerForm.apiEndpointPlaceholder")}
          hint={
            needsModelMapping && apiFormat === "openai_responses"
              ? t("providerForm.apiHintResponses")
              : needsModelMapping && apiFormat === "openai_chat"
                ? t("providerForm.apiHintOAI")
                : needsModelMapping && apiFormat === "gemini_native"
                  ? t("providerForm.apiHintGeminiNative")
                  : t("providerForm.apiHint")
          }
          showManageButton={false}
        />

        <div className="space-y-3 rounded-lg border border-border-default bg-muted/20 p-4">
          <div className="flex items-center justify-between gap-4">
            <div className="space-y-1">
              <Label>
                {t("claudeDesktop.modelMappingToggle", {
                  defaultValue: "需要模型映射",
                })}
              </Label>
              <p className="text-xs leading-relaxed text-muted-foreground">
                {needsModelMapping
                  ? t("claudeDesktop.modelMappingOnHint", {
                      defaultValue:
                        "Claude Desktop 目前对模型 ID 进行了限制，如果您的供应商提供的模型不是 Claude 系列模型，则需要打开本开关，并在使用过程中保持本地路由开启。",
                    })
                  : t("claudeDesktop.modelMappingOffHint", {
                      defaultValue:
                        "适合供应商已经暴露并接受 claude-* / anthropic/claude-* 模型名的 Anthropic Messages API；请求会由 Claude Desktop 直连供应商。",
                    })}
              </p>
            </div>
            <Switch
              checked={needsModelMapping}
              onCheckedChange={handleModelMappingChange}
              aria-label={t("claudeDesktop.modelMappingToggle", {
                defaultValue: "需要模型映射",
              })}
            />
          </div>
        </div>

        {needsModelMapping && (
          <div className="space-y-4 rounded-lg border border-border-default p-4">
            <div className="space-y-2">
              <Label>
                {t("providerForm.apiFormat", { defaultValue: "API 格式" })}
              </Label>
              <Select
                value={apiFormat}
                onValueChange={(value) =>
                  setApiFormat(value as ClaudeApiFormat)
                }
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="anthropic">
                    {t("providerForm.apiFormatAnthropic", {
                      defaultValue: "Anthropic Messages (原生)",
                    })}
                  </SelectItem>
                  <SelectItem value="openai_chat">
                    {t("providerForm.apiFormatOpenAIChat", {
                      defaultValue: "OpenAI Chat Completions (需开启路由)",
                    })}
                  </SelectItem>
                  <SelectItem value="openai_responses">
                    {t("providerForm.apiFormatOpenAIResponses", {
                      defaultValue: "OpenAI Responses API (需开启路由)",
                    })}
                  </SelectItem>
                  <SelectItem value="gemini_native">
                    {t("providerForm.apiFormatGeminiNative", {
                      defaultValue:
                        "Gemini Native generateContent (需开启路由)",
                    })}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-3">
              <div className="space-y-1 border-t border-border-default pt-4">
                <div className="flex items-center justify-between">
                  <Label>
                    {t("claudeDesktop.routeMapTitle", {
                      defaultValue: "模型映射",
                    })}
                  </Label>
                  {renderActionButtons(
                    () =>
                      setRoutes((current) => [
                        ...current,
                        nextRouteRow(current, defaultProxyRouteRows),
                      ]),
                    t("claudeDesktop.addRoute", { defaultValue: "添加模型" }),
                  )}
                </div>
                <p className="text-xs leading-relaxed text-muted-foreground">
                  {t("claudeDesktop.routeMapHint", {
                    defaultValue:
                      "填写供应商实际提供的模型名，显示名为在 Claude Desktop 模型列表中展示的名称。",
                  })}
                </p>
              </div>

              <div className="hidden grid-cols-[1fr_1fr_92px_36px] gap-2 px-1 text-xs font-medium text-muted-foreground md:grid">
                <span>
                  {t("claudeDesktop.upstreamModelLabel", {
                    defaultValue: "实际请求模型",
                  })}
                </span>
                <span>
                  {t("claudeDesktop.displayNameLabel", {
                    defaultValue: "显示名",
                  })}
                </span>
                <span>
                  {t("claudeDesktop.supports1mLabel", {
                    defaultValue: "1M",
                  })}
                </span>
                <span />
              </div>
              {routes.map((route, index) => (
                <div
                  key={`${route.route}-${index}`}
                  className="grid grid-cols-1 gap-2 md:grid-cols-[1fr_1fr_92px_36px]"
                >
                  <div className="flex gap-1">
                    <Input
                      value={route.model}
                      onChange={(event) =>
                        updateRoute(index, { model: event.target.value })
                      }
                      placeholder="kimi-k2 / deepseek-chat"
                      className="flex-1"
                    />
                    {fetchedModels.length > 0 && (
                      <ModelDropdown
                        models={fetchedModels}
                        onSelect={(id) => updateRoute(index, { model: id })}
                      />
                    )}
                  </div>
                  <Input
                    value={route.displayName}
                    onChange={(event) =>
                      updateRoute(index, { displayName: event.target.value })
                    }
                    placeholder="Sonnet"
                  />
                  <label className="flex h-9 items-center gap-2 text-sm text-muted-foreground">
                    <Checkbox
                      checked={route.supports1m}
                      onCheckedChange={(checked) =>
                        updateRoute(index, { supports1m: checked === true })
                      }
                    />
                    1M
                  </label>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    onClick={() =>
                      setRoutes((current) =>
                        current.filter((_, i) => i !== index),
                      )
                    }
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
              ))}
            </div>
          </div>
        )}

        {!needsModelMapping && (
          <Collapsible
            open={directModelsExpanded}
            onOpenChange={setDirectModelsExpanded}
          >
            <CollapsibleTrigger asChild>
              <Button
                type="button"
                variant={null}
                size="sm"
                className="h-8 gap-1.5 px-0 text-sm font-medium text-foreground hover:opacity-70"
              >
                {directModelsExpanded ? (
                  <ChevronDown className="h-4 w-4" />
                ) : (
                  <ChevronRight className="h-4 w-4" />
                )}
                {t("claudeDesktop.directModelListTitle", {
                  defaultValue:
                    "手动指定 Claude Desktop 模型列表（高级，可选）",
                })}
              </Button>
            </CollapsibleTrigger>
            {!directModelsExpanded && (
              <p className="ml-1 mt-1 text-xs text-muted-foreground">
                {t("claudeDesktop.directModelListCollapsedHint", {
                  defaultValue:
                    "原生 Claude 模型供应商通常不用填写，Claude Desktop 会自动读取 /v1/models。",
                })}
              </p>
            )}
            <CollapsibleContent className="space-y-4 pt-2">
              <div className="space-y-4 rounded-lg border border-border-default p-4">
                <div className="flex flex-wrap items-start justify-between gap-3">
                  <p className="flex-1 text-xs leading-relaxed text-muted-foreground">
                    {t("claudeDesktop.directModelListHint", {
                      defaultValue:
                        "仅当供应商的 /v1/models 不可用或没有返回 Claude Desktop 可识别的 claude-* 模型名时填写；这些模型名会原样发送给供应商。",
                    })}
                  </p>
                  {renderActionButtons(
                    () =>
                      setRoutes((current) => [
                        ...current,
                        {
                          route: "",
                          model: "",
                          displayName: "",
                          supports1m: false,
                        },
                      ]),
                    t("claudeDesktop.addModel", { defaultValue: "添加模型" }),
                  )}
                </div>

                {routes.length > 0 ? (
                  <div className="space-y-2">
                    {routes.map((route, index) => (
                      <div
                        key={`${route.route}-${index}`}
                        className="grid grid-cols-1 gap-2 md:grid-cols-[1fr_92px_36px]"
                      >
                        <div className="flex gap-1">
                          <Input
                            value={route.route}
                            onChange={(event) =>
                              updateRoute(index, { route: event.target.value })
                            }
                            placeholder="claude-deepseek-chat"
                            className="flex-1"
                          />
                          {fetchedModels.length > 0 && (
                            <ModelDropdown
                              models={fetchedModels}
                              onSelect={(id) =>
                                updateRoute(index, { route: id })
                              }
                            />
                          )}
                        </div>
                        <label className="flex h-9 items-center gap-2 text-sm text-muted-foreground">
                          <Checkbox
                            checked={route.supports1m}
                            onCheckedChange={(checked) =>
                              updateRoute(index, {
                                supports1m: checked === true,
                              })
                            }
                          />
                          1M
                        </label>
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          onClick={() =>
                            setRoutes((current) =>
                              current.filter((_, i) => i !== index),
                            )
                          }
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    ))}
                  </div>
                ) : null}
              </div>
            </CollapsibleContent>
          </Collapsible>
        )}

        <FormField
          control={form.control}
          name="settingsConfig"
          render={() => (
            <FormItem className="space-y-0">
              <FormControl>
                <input type="hidden" />
              </FormControl>
              <FormMessage />
            </FormItem>
          )}
        />

        {showButtons && (
          <div className="flex justify-end gap-2">
            <Button variant="outline" type="button" onClick={onCancel}>
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={form.formState.isSubmitting}>
              {submitLabel}
            </Button>
          </div>
        )}
      </form>
    </Form>
  );
}
