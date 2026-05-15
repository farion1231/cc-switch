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
import { CodexOAuthSection } from "./CodexOAuthSection";
import { CopilotAuthSection } from "./CopilotAuthSection";
import { EndpointField } from "./shared/EndpointField";
import { ModelDropdown } from "./shared/ModelDropdown";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import { providerSchema, type ProviderFormData } from "@/lib/schemas/provider";
import type {
  ClaudeApiFormat,
  ClaudeDesktopModelRoute,
  ProviderCategory,
  ProviderMeta,
} from "@/types";
import type { OpenClawSuggestedDefaults } from "@/config/openclawProviderPresets";
import {
  CLAUDE_DESKTOP_ROLE_ROUTE_IDS,
  claudeDesktopProviderPresets,
  type ClaudeDesktopProviderPreset,
  type ClaudeDesktopRoleId,
} from "@/config/claudeDesktopProviderPresets";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import {
  providersApi,
  type ClaudeDesktopDefaultRoute,
} from "@/lib/api/providers";
import { resolveManagedAccountId } from "@/lib/authBinding";

export type ClaudeDesktopProviderFormValues = ProviderFormData & {
  presetId?: string;
  presetCategory?: ProviderCategory;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  meta?: ProviderMeta;
  providerKey?: string;
  suggestedDefaults?: OpenClawSuggestedDefaults;
};

type ApiKeyField = "ANTHROPIC_AUTH_TOKEN" | "ANTHROPIC_API_KEY";

type PresetEntry = {
  id: string;
  preset: ClaudeDesktopProviderPreset;
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
  rowId: string;
  route: string;
  model: string;
  labelOverride: string;
  supports1m: boolean;
};

type RouteRowValues = Omit<RouteRow, "rowId">;
type RouteRole = ClaudeDesktopRoleId;

const CLAUDE_ROUTE_PREFIX = "claude-";
const ANTHROPIC_CLAUDE_ROUTE_PREFIX = "anthropic/claude-";
const LEGACY_ONE_M_MARKER = "[1m]";
const ROLE_ROUTE_IDS = CLAUDE_DESKTOP_ROLE_ROUTE_IDS;
const ROLE_ORDER: RouteRole[] = ["sonnet", "opus", "haiku"];
const NON_ANTHROPIC_ROUTE_MARKERS = [
  "ark-code",
  "astron",
  "command-r",
  "deepseek",
  "doubao",
  "gemini",
  "gemma",
  "glm",
  "gpt",
  "grok",
  "hermes",
  "hy3",
  "kimi",
  "lfm",
  "llama",
  "longcat",
  "mimo",
  "minimax",
  "mistral",
  "mixtral",
  "moonshot",
  "nemotron",
  "openai",
  "qianfan",
  "qwen",
  "stepfun",
  "seed-",
  "hunyuan",
  "nova-",
  "ernie",
  "codex",
  "abab",
  "jamba",
  "arctic",
  "solar",
  "mercury",
];

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

function routeRoleFromId(route: string): RouteRole {
  const normalized = route.trim().toLowerCase();
  if (normalized.includes("opus")) return "opus";
  if (normalized.includes("haiku")) return "haiku";
  return "sonnet";
}

function routeIdForRole(role: RouteRole, usedRoutes: Set<string>) {
  const baseRoute = ROLE_ROUTE_IDS[role];
  if (!usedRoutes.has(baseRoute)) return baseRoute;

  let index = 2;
  while (usedRoutes.has(`${baseRoute}-r${index}`)) {
    index += 1;
  }
  return `${baseRoute}-r${index}`;
}

function fallbackCatalogRouteId(usedRoutes: Set<string>) {
  const role = ROLE_ORDER.find((candidate) => {
    const route = ROLE_ROUTE_IDS[candidate];
    return !usedRoutes.has(route);
  });
  return routeIdForRole(role ?? "sonnet", usedRoutes);
}

function createRouteRow(row: RouteRowValues): RouteRow {
  return {
    rowId: crypto.randomUUID(),
    ...row,
  };
}

function initialRouteRows(
  routes: Record<string, ClaudeDesktopModelRoute> | undefined,
): RouteRow[] {
  const usedRoutes = new Set(
    Object.keys(routes ?? {}).filter((route) => isClaudeSafeRoute(route)),
  );

  return Object.entries(routes ?? {}).map(([route, value]) => {
    const routeId = isClaudeSafeRoute(route)
      ? route
      : fallbackCatalogRouteId(usedRoutes);
    usedRoutes.add(routeId);

    return createRouteRow({
      route: routeId,
      model: value.model ?? "",
      labelOverride:
        value.labelOverride ??
        (!isClaudeSafeRoute(route) ? value.model || route : ""),
      supports1m: value.supports1m ?? false,
    });
  });
}

function isClaudeSafeRoute(route: string) {
  const normalized = route.trim().toLowerCase();
  if (normalized.includes(LEGACY_ONE_M_MARKER)) return false;
  const hasAllowedShape =
    [CLAUDE_ROUTE_PREFIX, ANTHROPIC_CLAUDE_ROUTE_PREFIX].some(
      (prefix) =>
        normalized.startsWith(prefix) && normalized.length > prefix.length,
    ) ||
    ["sonnet", "opus", "haiku"].includes(normalized) ||
    normalized.startsWith("sonnet-") ||
    normalized.startsWith("opus-") ||
    normalized.startsWith("haiku-");
  return (
    hasAllowedShape &&
    !NON_ANTHROPIC_ROUTE_MARKERS.some((marker) => normalized.includes(marker))
  );
}

function defaultRouteRows(
  defaults: ClaudeDesktopDefaultRoute[],
  defaultModel: string,
): RouteRow[] {
  return defaults.map((route, index) =>
    createRouteRow({
      route: route.routeId,
      model: index === 0 ? defaultModel : "",
      labelOverride: "",
      supports1m: route.supports1m,
    }),
  );
}

function nextRouteRow(current: RouteRow[], defaults: RouteRow[]): RouteRow {
  const defaultRow =
    defaults.find(
      (route) => !current.some((existing) => existing.route === route.route),
    ) ?? null;

  if (defaultRow) {
    return createRouteRow({
      route: defaultRow.route,
      model: defaultRow.model,
      labelOverride: defaultRow.labelOverride,
      supports1m: defaultRow.supports1m,
    });
  }

  const usedRoutes = new Set(current.map((route) => route.route));
  return createRouteRow({
    route: routeIdForRole("sonnet", usedRoutes),
    model: "",
    labelOverride: "",
    supports1m: true,
  });
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
  const isOfficial = initialData?.category === "official";
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
  const [apiKeyField, setApiKeyField] = useState<ApiKeyField>(() =>
    envString(initialData?.settingsConfig, "ANTHROPIC_API_KEY")
      ? "ANTHROPIC_API_KEY"
      : "ANTHROPIC_AUTH_TOKEN",
  );
  const [selectedGitHubAccountId, setSelectedGitHubAccountId] = useState<
    string | null
  >(() => resolveManagedAccountId(initialData?.meta, "github_copilot"));
  const [selectedCodexAccountId, setSelectedCodexAccountId] = useState<
    string | null
  >(() => resolveManagedAccountId(initialData?.meta, "codex_oauth"));
  const [codexFastMode, setCodexFastMode] = useState<boolean>(
    () => initialData?.meta?.codexFastMode ?? false,
  );
  const [selectedPresetId, setSelectedPresetId] = useState<string | null>(
    "custom",
  );
  const [activePreset, setActivePreset] = useState<{
    id: string;
    category?: ProviderCategory;
    isPartner?: boolean;
    partnerPromotionKey?: string;
    providerType?: string;
    requiresOAuth?: boolean;
  } | null>(null);
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

  const presetEntries = useMemo<PresetEntry[]>(
    () =>
      claudeDesktopProviderPresets.map((preset, index) => ({
        id: `claude-desktop-${index}`,
        preset,
      })),
    [],
  );

  const presetCategoryLabels: Record<string, string> = useMemo(
    () => ({
      official: t("providerForm.categoryOfficial", { defaultValue: "官方" }),
      cn_official: t("providerForm.categoryCnOfficial", {
        defaultValue: "国内官方",
      }),
      aggregator: t("providerForm.categoryAggregation", {
        defaultValue: "聚合服务",
      }),
      third_party: t("providerForm.categoryThirdParty", {
        defaultValue: "第三方",
      }),
    }),
    [t],
  );
  const activeProviderType =
    activePreset?.providerType ?? initialData?.meta?.providerType;
  const usesManagedOAuth =
    activePreset?.requiresOAuth === true ||
    activeProviderType === "github_copilot" ||
    activeProviderType === "codex_oauth";

  const applyDesktopPreset = (preset: ClaudeDesktopProviderPreset) => {
    form.setValue("name", preset.nameKey ? t(preset.nameKey) : preset.name);
    form.setValue("websiteUrl", preset.websiteUrl);
    form.setValue("notes", "");
    form.setValue("icon", preset.icon ?? "");
    form.setValue("iconColor", preset.iconColor ?? "");

    setBaseUrl(preset.baseUrl);
    setApiKey("");
    setApiKeyField(preset.apiKeyField ?? "ANTHROPIC_AUTH_TOKEN");
    setApiFormat(preset.apiFormat ?? "anthropic");

    didSeedDefaultRoutes.current = true;
    setMode(preset.mode);
    if (preset.mode === "proxy" && preset.modelRoutes) {
      setRoutes(
        preset.modelRoutes.map((r) =>
          createRouteRow({
            route: r.routeId,
            model: r.upstreamModel,
            labelOverride: r.labelOverride ?? "",
            supports1m: r.supports1m,
          }),
        ),
      );
    } else {
      setRoutes([]);
    }
  };

  const handlePresetChange = (value: string) => {
    setSelectedPresetId(value);

    if (value === "custom") {
      setActivePreset(null);
      form.reset(defaultValues);
      setBaseUrl("");
      setApiKey("");
      setApiKeyField("ANTHROPIC_AUTH_TOKEN");
      setApiFormat("anthropic");
      didSeedDefaultRoutes.current = false;
      setMode("direct");
      setRoutes([]);
      return;
    }

    const entry = presetEntries.find((item) => item.id === value);
    if (!entry) return;

    setActivePreset({
      id: value,
      category: entry.preset.category,
      isPartner: entry.preset.isPartner,
      partnerPromotionKey: entry.preset.partnerPromotionKey,
      providerType: entry.preset.providerType,
      requiresOAuth: entry.preset.requiresOAuth,
    });
    applyDesktopPreset(entry.preset);
  };

  const updateRoute = (index: number, patch: Partial<RouteRowValues>) => {
    setRoutes((current) =>
      current.map((row, i) => (i === index ? { ...row, ...patch } : row)),
    );
  };

  const updateRouteRole = (index: number, role: RouteRole) => {
    setRoutes((current) => {
      const usedRoutes = new Set(
        current
          .filter((_, i) => i !== index)
          .map((row) => row.route)
          .filter(Boolean),
      );
      const route = routeIdForRole(role, usedRoutes);
      return current.map((row, i) => (i === index ? { ...row, route } : row));
    });
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
    if (isOfficial) {
      // 官方供应商使用 Claude Desktop 内置 1P 模式，保持空 env 占位；
      // 不写 claudeDesktopMode / claudeDesktopModelRoutes / apiFormat，
      // 与启动 seed 的 OFFICIAL_SEEDS 占位语义一致。
      const settingsConfig = clonePlainRecord(initialData?.settingsConfig);
      settingsConfig.env = {};
      const meta: ProviderMeta = { ...(initialData?.meta ?? {}) };
      delete meta.claudeDesktopMode;
      delete meta.claudeDesktopModelRoutes;
      delete meta.apiFormat;
      delete meta.endpointAutoSelect;
      delete meta.isFullUrl;
      await onSubmit({
        ...values,
        name: values.name.trim(),
        websiteUrl: values.websiteUrl?.trim() ?? "",
        notes: values.notes?.trim() ?? "",
        settingsConfig: JSON.stringify(settingsConfig, null, 2),
        meta,
        presetId: activePreset?.id,
        presetCategory: "official",
      });
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
    if (!usesManagedOAuth && !apiKey.trim()) {
      toast.error(
        t("providerForm.fetchModelsNeedApiKey", {
          defaultValue: "请先填写 API Key",
        }),
      );
      return;
    }

    const routeEntries = routes
      .map((route) => ({
        ...route,
        route: route.route.trim(),
        model: route.model.trim(),
        labelOverride: route.labelOverride.trim(),
      }))
      .filter((route) => route.route || route.model);

    if (mode === "proxy") {
      const invalid = routeEntries.find(
        (route) =>
          !route.route || !route.model || !isClaudeSafeRoute(route.route),
      );
      if (invalid) {
        toast.error(
          t("claudeDesktop.routeInvalid", {
            defaultValue: "请填写 Desktop 显示模型和实际请求模型",
          }),
        );
        return;
      }
      if (routeEntries.length === 0) {
        toast.error(
          t("claudeDesktop.routesRequired", {
            defaultValue: "至少填写一个模型映射",
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
    delete env.ANTHROPIC_AUTH_TOKEN;
    delete env.ANTHROPIC_API_KEY;
    settingsConfig.env = usesManagedOAuth
      ? {
          ...env,
          ANTHROPIC_BASE_URL: baseUrl.trim().replace(/\/+$/, ""),
        }
      : {
          ...env,
          ANTHROPIC_BASE_URL: baseUrl.trim().replace(/\/+$/, ""),
          [apiKeyField]: apiKey.trim(),
        };

    const routeMap = routeEntries.reduce<
      Record<string, ClaudeDesktopModelRoute>
    >((acc, route) => {
      acc[route.route] = {
        model: route.model || route.route,
        labelOverride:
          route.labelOverride || (mode === "proxy" ? route.model : undefined),
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
    meta.providerType = activeProviderType;
    meta.authBinding =
      activeProviderType === "github_copilot"
        ? {
            source: "managed_account",
            authProvider: "github_copilot",
            accountId: selectedGitHubAccountId ?? undefined,
          }
        : activeProviderType === "codex_oauth"
          ? {
              source: "managed_account",
              authProvider: "codex_oauth",
              accountId: selectedCodexAccountId ?? undefined,
            }
          : undefined;
    meta.codexFastMode =
      activeProviderType === "codex_oauth" ? codexFastMode : undefined;

    delete meta.endpointAutoSelect;
    delete meta.isFullUrl;

    await onSubmit({
      ...values,
      name: values.name.trim(),
      websiteUrl: values.websiteUrl?.trim() ?? "",
      notes: values.notes?.trim() ?? "",
      settingsConfig: JSON.stringify(settingsConfig, null, 2),
      meta,
      presetId: activePreset?.id,
      presetCategory: activePreset?.category,
      isPartner: activePreset?.isPartner,
      partnerPromotionKey: activePreset?.partnerPromotionKey,
    });
  };

  const renderActionButtons = (onAdd: () => void, addLabel: string) => (
    <div className="flex gap-1">
      {!usesManagedOAuth && (
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
      )}
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
        {!initialData && (
          <ProviderPresetSelector
            selectedPresetId={selectedPresetId}
            presetEntries={presetEntries}
            presetCategoryLabels={presetCategoryLabels}
            onPresetChange={handlePresetChange}
            category={activePreset?.category}
          />
        )}

        <BasicFormFields form={form} />

        {isOfficial && (
          <div className="rounded-lg border border-border-default bg-muted/20 p-3 text-sm text-muted-foreground">
            {t("claudeDesktop.officialNotice", {
              defaultValue:
                "Claude Desktop 官方供应商使用应用内置的 1P 登录，无需配置 API Key 和接口地址。",
            })}
          </div>
        )}

        {!isOfficial && (
          <>
            {usesManagedOAuth ? (
              <div className="rounded-lg border border-border-default bg-muted/20 p-3">
                {activeProviderType === "github_copilot" ? (
                  <CopilotAuthSection
                    selectedAccountId={selectedGitHubAccountId}
                    onAccountSelect={setSelectedGitHubAccountId}
                  />
                ) : (
                  <CodexOAuthSection
                    selectedAccountId={selectedCodexAccountId}
                    onAccountSelect={setSelectedCodexAccountId}
                    fastModeEnabled={codexFastMode}
                    onFastModeChange={setCodexFastMode}
                  />
                )}
              </div>
            ) : (
              <div className="space-y-1">
                <Label>{"API Key"}</Label>
                <Input
                  value={apiKey}
                  onChange={(event) => setApiKey(event.target.value)}
                  type="password"
                  placeholder="sk-..."
                />
              </div>
            )}

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
                        t("claudeDesktop.addRoute", {
                          defaultValue: "添加模型",
                        }),
                      )}
                    </div>
                    <p className="text-xs leading-relaxed text-muted-foreground">
                      {t("claudeDesktop.routeMapHint", {
                        defaultValue:
                          "选择模型角色后，CC Switch 会自动生成 Claude Desktop 兼容路由；菜单显示名可以写 DeepSeek、Kimi 等品牌模型，实际请求模型按右侧填写内容发送。",
                      })}
                    </p>
                  </div>

                  <div className="hidden grid-cols-[140px_1fr_1fr_116px_36px] gap-2 px-1 text-xs font-medium text-muted-foreground md:grid">
                    <span>
                      {t("claudeDesktop.routeModelLabel", {
                        defaultValue: "模型角色",
                      })}
                    </span>
                    <span>
                      {t("claudeDesktop.labelOverrideLabel", {
                        defaultValue: "菜单显示名",
                      })}
                    </span>
                    <span>
                      {t("claudeDesktop.upstreamModelLabel", {
                        defaultValue: "实际请求模型",
                      })}
                    </span>
                    <span>
                      {t("claudeDesktop.supports1mLabel", {
                        defaultValue: "声明支持 1M",
                      })}
                    </span>
                    <span />
                  </div>
                  {routes.map((route, index) => (
                    <div
                      key={route.rowId}
                      className="grid grid-cols-1 gap-2 md:grid-cols-[140px_1fr_1fr_116px_36px]"
                    >
                      <Select
                        value={routeRoleFromId(route.route)}
                        onValueChange={(value) =>
                          updateRouteRole(index, value as RouteRole)
                        }
                      >
                        <SelectTrigger className="h-9">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="sonnet">
                            {t("claudeDesktop.routeRoleSonnet", {
                              defaultValue: "Sonnet",
                            })}
                          </SelectItem>
                          <SelectItem value="opus">
                            {t("claudeDesktop.routeRoleOpus", {
                              defaultValue: "Opus",
                            })}
                          </SelectItem>
                          <SelectItem value="haiku">
                            {t("claudeDesktop.routeRoleHaiku", {
                              defaultValue: "Haiku",
                            })}
                          </SelectItem>
                        </SelectContent>
                      </Select>
                      <Input
                        value={route.labelOverride}
                        onChange={(event) =>
                          updateRoute(index, {
                            labelOverride: event.target.value,
                          })
                        }
                        placeholder="DeepSeek V4 Pro"
                      />
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
                            onSelect={(id) =>
                              updateRoute(index, {
                                model: id,
                                labelOverride: route.labelOverride || id,
                                route:
                                  route.route ||
                                  routeIdForRole(
                                    "sonnet",
                                    new Set(routes.map((row) => row.route)),
                                  ),
                              })
                            }
                          />
                        )}
                      </div>
                      <label className="flex h-9 items-center gap-2 text-sm text-muted-foreground">
                        <Checkbox
                          checked={route.supports1m}
                          onCheckedChange={(checked) =>
                            updateRoute(index, { supports1m: checked === true })
                          }
                        />
                        {t("claudeDesktop.supports1mShort", {
                          defaultValue: "1M",
                        })}
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
                            createRouteRow({
                              route: "",
                              model: "",
                              labelOverride: "",
                              supports1m: false,
                            }),
                          ]),
                        t("claudeDesktop.addModel", {
                          defaultValue: "添加模型",
                        }),
                      )}
                    </div>

                    {routes.length > 0 ? (
                      <div className="space-y-2">
                        {routes.map((route, index) => (
                          <div
                            key={route.rowId}
                            className="grid grid-cols-1 gap-2 md:grid-cols-[1fr_116px_36px]"
                          >
                            <div className="flex gap-1">
                              <Input
                                value={route.route}
                                onChange={(event) =>
                                  updateRoute(index, {
                                    route: event.target.value,
                                  })
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
                              {t("claudeDesktop.supports1mShort", {
                                defaultValue: "1M",
                              })}
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
          </>
        )}

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
